---
title: "Iteration 255: give infobox facts a ref, and stop --query missing Formation for \"formed\""
type: iteration
date: 2026-09-01
status: in-progress
branch: iter-255/infobox-facts-refs-and-query-matching
depends_on:
  - 230
dogfood_path: >-
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask --
  check-dogfood-script kb/iterations/iteration-255-infobox-facts-refs-and-query-matching.md;
  the script owns its browser, obtains the Developer fact links entry on the real
  Python article, clicks its daemon ref with --with-page --query founded formed,
  verifies Formation, and confirms the PSF URL.
tags:
  - iteration
  - agent-ergonomics
  - page-view
  - act-and-see
  - carry-over
takeover_sequencing_2026_09_09: >-
  Iteration 255 retains its own branch and PR and all original acceptance
  criteria. Before its Theme C measurement, prepare and verify the iteration-256-owned
  harness on its own branch, checkpoint it after ordered gates, and use that exact
  revision from a run-owned external export. Merge 255 first, then complete the final
  256 measurement and its single PR. This explicitly changes the historical
  land-first scheduling phrase to prepare-and-verify-first; harness ownership and
  benchmark model/prompt requirements are unchanged.
takeover_reconciliation_252: "2026-09-09, reconciliation after252: recoverable harness pin d28c5e79aa7ee7a59a386fc34125f8cd1470fbeb and exact agent/judge model claude-sonnet-4-6 established from original transcripts. Preserve historical ff-rdp append-system prompt byte-for-byte for this six-run comparison; keep256 ambient treatment separate. Two minimal same-model capability probes failed HTTP401 invalid API key before tokens; this proves authentication failure, not model entitlement failure. ThemeC remains mandatory and unticked pending successful auth and actual runs. Previously recorded256A-export-before255 sequencing remains binding."
benchmark_auth_resolution_252: "2026-09-09: the two earlier HTTP401 probes are retained as history. A third exact same-model probe succeeded with CAPABILITY_OK, exit0, claude-sonnet-4-6 modelUsage and 900ms duration, using the already cached claude.ai Team session with only the stale ANTHROPIC_API_KEY omitted per process. No login, global configuration or model/prompt change. Use that scoped environment for actual harness runs. This resolves account capability only; actual benchmark acceptance criteria remain unticked until measured. Evidence: .git/ralph-loop/20260909-takeover/harness-prep/auth-resolution.md and capability-cached-session.stdout."
iteration_252_reconciliation_2026_09_12: "2026-09-12 pending-plan reconciliation from iteration252: the out-of-scope note that a missed click selector still waits the full --timeout is historical. Iteration237 PartB already short-circuits a still-absent selector once the page is settled and the minimum observation floor is met (js_helpers.rs::autowait_element; default timeout floor about2s). Unsettled pages or unavailable settle probes can still wait the full budget. This does not implement infobox refs/query matching or complete any255 benchmark AC; the residual per-read deadline survey belongs to258. Original AC wording and the recorded255/256 harness sequencing remain intact."
iteration_254_source_reconciliation: "2026-09-12: Actual existing facts payload is an array of {key:string,value:string}, not a JSON key-to-string map. Preserve its existing fields, types, normalization, ordering and caps; original plan wording remains intact as compatibility intent. Collection currently drops the source value/link elements; refs are daemon-only and generation-guarded. QueryFilter uses one whole case-insensitive substring, not tokenized terms. Decide an additive handle shape and fact-key matching/explanation rule before code; preserve direct-route and other output/query semantics. The exact256ThemeA harness prerequisite still precedes the six comparable measurements. Source map: .git/ralph-loop/20260912-validation-efficiency/iter255-source-preparation/report.md."
first_call_sites:
  - primitive: QueryFilter::matches_fact_key
    site: crates/ff-rdp-cli/src/commands/page_view.rs::filter_page_view
dogfood_script: iteration-255-infobox-facts-refs-and-query-matching.dogfood.sh
---

# Iteration 255: the two costs left in `wikipedia_infobox_hop`

> **Renumbered 231 → 255 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 231.

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

### A. Refs on infobox facts [3/3]
- [x] Choose and document the payload shape in [[decision-log]] before writing it
- [x] Emit the handle from the infobox collection pass; live Firefox test on the Python article
- [x] Confirm `click --ref <that handle> --with-page` lands on the PSF article

### B. Query near-match [3/3]
- [x] Decide match rule; unit-test it against a corpus of real infobox keys, positives *and*
      negatives (a rule that matches everything is worse than one that matches nothing)
- [x] `matches: 0` reports what was compared against
- [x] Live test: `--query 'founded formed'` on the PSF article reaches `Formation`

### C. Re-measure [0/2]
- [ ] `matrix --condition ff-rdp --task wikipedia_infobox_hop,wikipedia_link_follow --repeat 3`
      on a browser this run owns
- [ ] Record per-run turns and per-run ref-hunt command counts in [[axi-benchmark-comparison]]

## Acceptance Criteria [2/4]

- [x] A `--query`-matched infobox fact whose value is a link exposes a handle that `click` accepts
- [x] `--query 'founded formed'` on the PSF article either matches `Formation` or reports the keys
      it compared against
- [ ] `wikipedia_infobox_hop` re-measured at `--repeat 3`, per-run numbers recorded whatever they are
- [ ] `wikipedia_infobox_hop` ≤ 5 turns average — or the measured number recorded and this
      criterion left unticked, never reworded

## Out of scope

- Changing `--with-page`'s default. Still gated on [[iteration-256-act-and-see-benchmark-rerun]]
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

## Implementation evidence — 2026-09-13

DEC-055 in [[decision-log]] was recorded before product edits. Actual `facts` remains
an array of key/value string rows. Each linked row adds a `links` array containing
each value anchor's `name`, `href`, and daemon-registered `ref` when available.
The existing strings, order, normalization, deduplication and row caps are unchanged.
Query-retained fact links register independently of the interactive list; direct
connections and failed allocation/registration expose names/hrefs without invalid
refs. Scratch resolvers never appear in the emitted facts. Text output prints the
unchanged fact line followed by its links and registered handles.

Fact keys alone accept the bounded `formed`/`founded`/`formation` vocabulary. Other
fields and regex filters retain the existing semantics. Query output additionally
records `query_fact_keys` and `query_facts_truncated`, explicitly describing the
collected candidates rather than claiming full-page coverage. Zero-match text
output shows those candidates even with `--page-chars 0`.

Validation on real Firefox: all three `live_255_fact_links` tests passed, including
both routes and the actual Python → PSF navigation. The Python `Developer` fact
returned `Python Software Foundation` with `ref: e4`; `click --ref e4 --with-page
--query 'founded formed'` returned `Formation: March 6, 2001`, `matches: 1`,
`query_source: facts`, and `location.href` confirmed the PSF article. Five existing
`live_225_reader_facts` tests also passed, including unchanged fact text and DOM.
Three new unit tests cover the key corpus, collected/capped zero-match reporting,
multi-link registration, no daemon, allocation refusal, registration refusal and
generation mismatch. Original failed compile/test attempts remain in the run logs.

Source/validation artifacts are under
`.git/ralph-loop/20260912-validation-efficiency/iter255-implementation/`.
Theme C and both benchmark ACs remain pending: no paid measurement runs against
dirty source. The supervisor will use the reviewed clean product checkpoint and
the immutable iteration-256 Theme A export at
`5786329668c95479a4b91710764c7cee0e782f4a` for exactly the original two tasks × three
runs. No harness code is included in this iteration's product diff.

## Closing sweep and carry-over — 2026-09-13

The following records the original implementation-phase sweep before independent
review; the repair's final-source sweep is recorded separately below.

Final product source passed `rustup update stable` (unchanged Rust 1.98.1), then
`cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` (Clippy
0.1.98), then `cargo test --workspace -q`: 2,466 passed, zero failed, 413 ignored
across 36 result summaries. The ignored count is not live-Firefox evidence.

This iteration's own command was
`FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep`.
It exited 1, with the actual summaries:

```text
LIVE_SWEEP_SUMMARY executed=341 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=341
LIVE_SWEEP_PROFILES leaked=0 unattributed=0 root=/Users/james/Library/Application Support/ff-rdp/profiles
```

CLI: 322 passed + 10 failed = 332. The four core tiers passed 1 + 3 + 3 + 2 = 9.
All-tier total: 331 passed + 10 failed = 341. Every expected qualified name has
exactly one observed verdict, with zero missing/extra/duplicate names. All five
per-tier profile scans were clean. The six non-ignored CLI tests are outside the
live partition and were exercised by the workspace run, not silently omitted.
All three new 255 tests passed in the closing sweep. Each failed test was then
run by exact name in isolation; the original failures are retained below.

## Carry-over

| Observed result / unmet requirement | Disposition |
|---|---|
| `live_135_screenshot_ff153::live_135_screenshot_full_page_taller`: `drawSnapshot` fourth-argument dictionary TypeError; exact isolation also failed | **Fold — [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]]**, mandatory later fix; no screenshot source changed here |
| `live_144_session_hygiene_followup::live_144_full_page_no_duplicate_header`: same dictionary TypeError; exact isolation failed | **Fold — iteration 257**, preserving its seven-green-in-one-sweep AC |
| `live_61l::live_screenshot_full_page`: same dictionary TypeError; exact isolation failed | **Fold — iteration 257** |
| `live_61r_screenshot::live_screenshot_full_page`: same dictionary TypeError; exact isolation failed | **Fold — iteration 257** |
| `live_92_screenshot_full_page::live_screenshot_full_page_md5_differs_from_viewport`: same dictionary TypeError; exact isolation failed | **Fold — iteration 257** |
| `live_92_screenshot_full_page::pre_fix_repro_screenshot_full_page_taller_than_viewport`: same dictionary TypeError; exact isolation failed | **Fold — iteration 257** |
| `live_screenshot_shim::live_screenshot_unchanged_after_shim`: same dictionary TypeError; exact isolation failed | **Fold — iteration 257** |
| `live_137_daemon_mode_parity::live_137_consent_accept_via_daemon`: target promotion failed after 15,125 ms / 48 polls, target 1/live 0, healthy dispatcher 84/84, no in-flight work or RPC owner | **Fold — [[iteration-262-daemon-live-target-never-promoted]]**. Exact isolation reached targets in 50 ms / one poll and accepted Sourcepoint in 5.75 s. This does not supply a green sweep or close either promotion or historical distinct ready-target consent-action failure |
| `live_140_element_targeting::live_140_frame_error_bounded`: missing-selector error reported 0 of 0 frames tried/of 0 total | **Fold — iteration 262**, as the separately recorded zero-frame family. Exact isolation passed; failed-occurrence target counters and a common cause with promotion remain unproved |
| `live_224_with_page_connection_reset::live_repeated_hop_never_loses_the_connection`: hop 7/12 failed `User: daemon auth failed: recv failed: Connection reset by peer (os error 54)` | **File — [[iteration-268-daemon-pre-auth-connection-loss]]**. The pre-auth connection-loss recurrence trigger in [[iteration-203-live-sweep-watch-conditions-third-holder]] fired. Exact isolation passed all 12 hops with zero reconnects in 15.76 s; attributable failed-occurrence auth/dispatcher timing remains required. Do not classify this as post-auth 267 or claim a common mechanism with the historical EOF |
| Theme C two-task × three-run comparison, per-run turns/ref-hunts, and the two unticked benchmark ACs | **Fold — this iteration's remaining measurement phase** after the supervisor's reviewed clean checkpoint. No paid run happened in this implementation phase; no target claim or replacement AC |
| Original failed local development commands: nonexistent `--lib` target, mock framing-helper signature mismatch, and corpus expectation that conflicted with an existing literal substring | **Closed in this phase**: corrected invocation/helper and expectation; all original logs retained, final three unit tests and ordered workspace gates passed. No product matcher semantics were weakened |
| All five profile scans: no leaks or unattributed profiles; all qualified names present | **No plan, measured clean**. New leaks or missing/duplicate verdicts require fresh diagnosis; this is not a claim that the ten failed tests passed |

The existing post-auth Timeout owner 267 and styles watch in 203 did not recur in
this sweep and remain open. No canary audit or 10-second PID/load sampling is
claimed. The supervisor owns reconciliation of every upcoming pending plan and
the independent review, checkpoint, benchmark phase, and publication.

All nine enumerated xtask `check-*` commands passed. Actor/KB sync used exact
iteration base `f18a876866fd5fa047d4ca7e3a71dd88018b8824`; no actor source changed.
The Firefox-reference gate found no `firefox_refs` key, so it made no source-range
verification claim. `check-dogfood-script` ran with both live gates, verified a
fresh sentinel, and followed the real Python fact handle to PSF/Formation. The
full plan directory validated (272 plans, zero failures, 91 warning-only), and
Hyalo HYALO005 checked 463 files without issues. No product edits followed the
closing sweep. All run-owned processes stopped; port 6000 is free and unrelated
desktop Firefox PID 37270 was preserved. The stopped raw profile is retained in
the external run artifact directory, separate from the managed-profile root.

## Independent-review repair — 2026-09-13

The initial independent review found that short-valued facts could still export
arbitrarily many image links, and microdata's `content` attribute could hide a long
anchor name from the fact-value cap. The repair adds independent bounds before
serialization and registration: eight links per row, 32 across collected rows,
and 8,192 UTF-16 code units across names, hrefs and private selectors. Names are
limited to 256 units; hrefs and selectors to 2,048 each. Links exceeding a limit
are omitted whole, preserving exact destinations and resolvers on retained links.
Any omitted link sets its row's `links_truncated: true`, including when none fit;
text output explicitly says some fact links were omitted. DEC-055 records this
additive contract. Fact text/order/caps and query behavior are unchanged.

Two new live regressions cover both routes, exact/over-limit link counts,
definition-list sources, global count and text budgets, exact/oversized names,
hrefs and selectors, short microdata overrides, and visible text indicators.
The daemon test clicks the retained maximum-length selector to the expected
destination. The fail-closed unit test includes partially and completely omitted
rows through direct/allocation/registration/generation-error paths. Restoring the
original collector from tree `cf2d52647f72172da783ebe555f10d438bd6311f` makes the new
regression fail: observed row link counts `[8,9,10,8,1]` instead of
`[8,8,8,8,0]`. Repaired source passes all five iteration-255 live tests, including
the actual Python fact → PSF click and `founded formed` → `Formation` check.
The first fixture attempt's empty maximum-selector anchor was hidden; it failed
the click's visibility precondition and was made visibly clickable. That failed
attempt remains in `.git/ralph-loop/20260912-validation-efficiency/iter255-repair1/`.
No assertion or timeout was weakened.

### Repair closing evidence and carry-over

The repaired source passed `rustup update stable`, `cargo fmt`, strict workspace
Clippy, then `cargo test --workspace -q`, in order. The fresh closing command was
`FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep`.
It exited 1 with:

```text
LIVE_SWEEP_SUMMARY executed=343 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=343
LIVE_SWEEP_PROFILES leaked=0 unattributed=0 root=/Users/james/Library/Application Support/ff-rdp/profiles
```

CLI: 325 passed + 9 failed = 334. Four core tiers passed 1 + 3 + 3 + 2 = 9;
all tiers total 334 passed + 9 failed = 343. Independent ignored-test enumeration
of each actual binary exactly matches the observed qualified names: no missing,
extra or duplicate name and no reclassification. All five profile scans were
clean. All five iteration-255 live tests passed in this final-source sweep.

| Repair sweep observation | Disposition |
|---|---|
| `live_135_screenshot_ff153::live_135_screenshot_full_page_taller`: fourth-argument dictionary TypeError, also failed exact isolation | **Fold — iteration 257**, mandatory fix remains pending |
| `live_144_session_hygiene_followup::live_144_full_page_no_duplicate_header`: same TypeError, isolation failed | **Fold — iteration 257** |
| `live_61l::live_screenshot_full_page`: same TypeError, isolation failed | **Fold — iteration 257** |
| `live_61r_screenshot::live_screenshot_full_page`: same TypeError, isolation failed | **Fold — iteration 257** |
| `live_92_screenshot_full_page::live_screenshot_full_page_md5_differs_from_viewport`: same TypeError, isolation failed | **Fold — iteration 257** |
| `live_92_screenshot_full_page::pre_fix_repro_screenshot_full_page_taller_than_viewport`: same TypeError, isolation failed | **Fold — iteration 257** |
| `live_screenshot_shim::live_screenshot_unchanged_after_shim`: same TypeError, isolation failed | **Fold — iteration 257** |
| `live_137_daemon_mode_parity::live_137_consent_accept_via_daemon`: promotion failed after 15,210 ms/49 polls, target 1/live 0, healthy dispatcher 102/102 | **Fold — iteration 262**, updated with this failure and the exact isolated failure: 15,252 ms/49 polls, target 1/live 0, healthy dispatcher 79/79. This occurrence also fails outside the parallel sweep |
| `live_240_daemon_frame_desync_and_wedge::live_240_sustained_hops_never_desynchronise`: hop 16/40 pre-auth connection reset, zero reconnects | **Fold — iteration 268**, updated with Firefox PID 68682/debug 59766, proxy 59843 and shared-log daemon PID 68787. Exact isolation passed 40 hops in 27.46 s; missing failed-auth/dispatcher timing and the unresolved relationship to earlier EOF/reset remain explicit |
| Prior 140 zero-frame and 224 pre-auth reset failures passed this sweep | **Fold — existing 262 and 268 records remain open**; passing here does not erase the original implementation sweep |
| Review's unbounded links finding and initial hidden-anchor fixture failure | **Closed in repair** by the bounded collector/visible fixture and meaningful passing regressions, with the original-collector mutation failing |
| Five clean profile scans, zero coverage omissions | **No plan, measured clean**; a new leak or missing/duplicate verdict requires fresh diagnosis |
| Theme C and benchmark ACs remain unticked | **Fold — remaining 255 measurement phase** after the supervisor's reviewed clean checkpoint; no paid run or target claim in this repair |

All nine enumerated xtask gates passed after the sweep. The dogfood gate actually
executed Python → PSF/Formation and verified its fresh sentinel. Actor sync used
the exact base `f18a876866fd5fa047d4ca7e3a71dd88018b8824`; no actor changes. Firefox
refs found no source citations to check. Product hashes stayed unchanged through
the sweep and isolations. The raw unmanaged Firefox PID 52411 was stopped and
its owned profile retained in `iter255-repair1/raw-profile-stopped`; the original
implementation's stopped profile was preserved separately. The supervisor owns
fresh repair review, all-upcoming-plan reconciliation, checkpoint and measurement.
