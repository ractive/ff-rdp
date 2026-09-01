---
title: "Iteration 231: give infobox facts a ref, and stop --query missing Formation for \"formed\""
type: iteration
date: 2026-09-01
status: planned
branch: iter-231/infobox-facts-refs-and-query-matching
depends_on:
  - 230
dogfood_path: |
  # Both defects are visible in one command on the article the benchmark uses.
  ff-rdp launch --headless
  ff-rdp navigate 'https://en.wikipedia.org/wiki/Python_(programming_language)' --with-page \
    --query 'Developer' --jq '.results.page.facts'
  # TODAY: {"Developer": "Python Software Foundation"} — the value IS a link on the page and the
  # payload knows the element, but the fact carries no `ref`, so the next hop needs a ref hunt.
  # AFTER: the fact carries a ref (or a sibling handle) the agent can click straight through.
  ff-rdp navigate 'https://en.wikipedia.org/wiki/Python_Software_Foundation' --with-page \
    --query 'founded formed' --jq '.results.page | {matches, query_source}'
  # TODAY: matches 0 — the infobox key is `Formation`, and neither term is a substring of it.
  # AFTER: a near-match reaches the `Formation` fact, or `matches: 0` says what it did try.
  ff-rdp daemon stop
tags:
  - iteration
  - agent-ergonomics
  - page-view
  - act-and-see
  - carry-over
---

# Iteration 231: the two costs left in `wikipedia_infobox_hop`

## Why

[[iteration-230-quickstart-navigate-with-page]] put `navigate <URL> --with-page --query "<text>"`
into the first 16 lines of `--help` and re-measured. Adoption of the idiom went **1 of 6 runs to
6 of 6**, `wikipedia_link_follow` went 9.0 → **4.7** turns (target ≤ 5 met, axi's 4.0 matched), and
the two-task average went 10.2 → 6.3. `wikipedia_infobox_hop` went 11.3 → **8.0** and did not
reach target, so 230's AC 4 is unticked.

Runs 2 and 3 of `infobox_hop` are byte-for-byte the same trajectory, and it isolates two costs the
Quick-start line cannot touch. Full trace in [[axi-benchmark-comparison]]'s second 2026-09-01
section:

```
ff-rdp navigate ".../Python_(programming_language)" --with-page --query "stable release"
ff-rdp page-text --query "Python Software Foundation"        # ref hunt, turn 1
ff-rdp a11y summary | grep -i "python software foundation"   # ref hunt, turn 2
ff-rdp dom "a[href*='Python_Software_Foundation']"           # ref hunt, turn 3 → e52
ff-rdp click --ref e52 --with-page --query "founded formed"
ff-rdp page-text --query "founded formed"                    # query miss, turn 1
ff-rdp page-text --full | head -100                          # query miss, turn 2
```

Five of eight commands are recovering things the payload already had.

- **A — infobox facts carry no `ref`.** `results.page.facts` is key → *text*
  ([[iteration-225-reader-excerpt-infobox]]). `Developer: Python Software Foundation` is rendered
  as an `<a>` in the infobox, and the same collection pass that produced the fact walked that
  element — but the fact drops the handle, so an agent that just read the answer still has to
  find the link three commands later. This is precisely the ref hunt 230 removed for *body*
  links, where `interactive[0].ref` answers it; infobox links never entered `interactive` at a
  rank the agent looked at.
- **B — `--query` misses a morphological near-match.** The PSF infobox key is `Formation`.
  `--query "founded formed"` matches nothing, in `facts` or in the readability text, and the run
  falls through to `page-text --full | head -100`. Whether the right answer is stemming, a prefix
  match on fact keys, or simply reporting *what was searched* when `matches: 0`, the present
  behaviour — silent zero — costs two turns every time it fires.

## Themes

- **A — a handle on every fact whose value is a link.** Decide the shape first: an optional
  `ref` on the fact entry, a parallel `facts_links` map, or promoting infobox links into
  `interactive` ahead of body links. Whichever it is, `--query`-matched facts must be clickable
  without a second command, and the JSON must stay backward compatible for consumers that read
  `facts` as key → string.
- **B — make a `--query` miss either match or explain itself.** A `matches: 0` that does not say
  what it compared against is unactionable. Minimum bar: report the candidate keys considered.
  Better bar: a near-match rule (case-insensitive stem/prefix over fact keys) that reaches
  `Formation` from `formed` without inventing false positives — which needs a test corpus of
  keys, not one example.
- **C — re-measure `wikipedia_infobox_hop` only.** `--repeat 3`, exclusive browser, per-run
  adoption and per-run ref-hunt command counts. The number to beat is **8.0**; the target is ≤ 5.
  `link_follow` is at target and should be re-run only to confirm no regression.

## Tasks

### A. Refs on infobox facts [0/3]
- [ ] Choose and document the payload shape in [[decision-log]] before writing it
- [ ] Emit the handle from the infobox collection pass; live Firefox test on the Python article
- [ ] Confirm `click --ref <that handle> --with-page` lands on the PSF article

### B. Query near-match [0/3]
- [ ] Decide match rule; unit-test it against a corpus of real infobox keys, positives *and*
      negatives (a rule that matches everything is worse than one that matches nothing)
- [ ] `matches: 0` reports what was compared against
- [ ] Live test: `--query 'founded formed'` on the PSF article reaches `Formation`

### C. Re-measure [0/2]
- [ ] `matrix --condition ff-rdp --task wikipedia_infobox_hop,wikipedia_link_follow --repeat 3`
      on a browser this run owns
- [ ] Record per-run turns and per-run ref-hunt command counts in [[axi-benchmark-comparison]]

## Acceptance Criteria [0/4]

- [ ] A `--query`-matched infobox fact whose value is a link exposes a handle that `click` accepts
- [ ] `--query 'founded formed'` on the PSF article either matches `Formation` or reports the keys
      it compared against
- [ ] `wikipedia_infobox_hop` re-measured at `--repeat 3`, per-run numbers recorded whatever they are
- [ ] `wikipedia_infobox_hop` ≤ 5 turns average — or the measured number recorded and this
      criterion left unticked, never reworded

## Out of scope

- Changing `--with-page`'s default. Still gated on [[iteration-213-act-and-see-benchmark-rerun]]
  Theme C; 230 made adoption real without it, which is evidence *against* needing the default flip,
  not for it.
- `click`'s not-found poll (a missed selector waits the full `--timeout`). Carried unfixed from
  [[iteration-228-two-task-benchmark-after-facts]]; it is wall-clock, not turns, and it did not
  fire in 230's six runs.
- Any further change to `--help`. 230's Quick start is measured and working; touching it again
  without a measurement would undo the only controlled comparison this line has.

## References

- [[iteration-230-quickstart-navigate-with-page]] — the measurement this comes out of
- [[axi-benchmark-comparison]] — second 2026-09-01 section, the six post-230 trajectories
- [[iteration-225-reader-excerpt-infobox]] — where `results.page.facts` came from
