---
title: axi.md browser benchmark — ff-rdp vs chrome-devtools-axi
status: completed
created: 2026-08-29
tags:
  - research
  - benchmark
  - agent-ergonomics
type: research
---

# axi.md browser benchmark: ff-rdp vs chrome-devtools-axi

Run on 2026-08-29 with the upstream harness (`kunchenguid/axi`, `bench-browser/`), unmodified
except for an added `ff-rdp` condition. Agent and judge: `claude-sonnet-4-6`. 14 tasks ×
**3 repeats** per condition (84 runs; an earlier 1-repeat run gave the same picture).
ff-rdp 0.3.0 (0a87d1d, `main`); chrome-devtools-axi 0.1.32. Same one-paragraph system prompt
for both ("you have the `X` CLI, run `X --help`").

## Exact historical condition paragraphs (recovered 2026-09-13)

Pinned upstream: `kunchenguid/axi@d28c5e79aa7ee7a59a386fc34125f8cd1470fbeb`.
The unchanged runner SHA-256 is
`11659d64e71fa116744f6b837d0b8b246c8623eb0f3cf796a2342a7567a24a8b`.
These are append paragraphs, not replacements for Claude Code's default system
prompt. Both agent and judge request `claude-sonnet-4-6`.

```text
# Tools

You have the `ff-rdp` CLI installed for browser automation.
Use it for all browsing tasks. Do NOT use curl, wget, or WebFetch.

Run `ff-rdp --help` for available commands and usage.
```

```text
# Tools

You have the `chrome-devtools-axi` CLI installed for browser automation.
Use it for all browsing tasks. Do NOT use curl, wget, or WebFetch.

Run `chrome-devtools-axi --help` for available commands and usage.
```

Including each trailing newline, their SHA-256 hashes are respectively
`758dfb37452f8099cfb46460ee417ccc73839cf0ee3553f7da0558229be49c8b` and
`576f9835b62ad24111d2d6cbc78bd1b58ac2d1e6546570e90a7c0a0f0578e12a`.
The historical CLI was 2.1.241; preparation observed 2.1.259 and the actual
2026-09-13 matrices used 2.1.270, so default-system-prompt parity is unproved. Baseline settings/hooks are disabled with
`--setting-sources ""`; a later ambient payload or installed-hook treatment must
be identified separately. Actual model usage can include auxiliary Haiku and
must accompany costs; neither subscription billing nor exclusive Sonnet usage
can be inferred from the requested model alone.

## Summary (n = 42 runs per condition)

| Condition | Success | Avg turns | Median turns | Avg cost | Avg input tokens | Cache% | Avg duration |
|---|---|---|---|---|---|---|---|
| chrome-devtools-axi | 42/42 | **6.0** | **4** | $0.160 | 162 k | 88% | 32.5 s |
| ff-rdp | 41/42 (98%) | 7.1 | 7 | **$0.134** | 181 k | 93% | 31.8 s |

ff-rdp is ~16% cheaper per task; axi needs ~16% fewer turns on average and its median is
4 vs 7. Both are within a few seconds on wall clock. Turn count is what axi optimises for and is
the right KPI for ff-rdp's agent ergonomics; the gap sits in one task class.

## Per task — avg turns / avg cost / passes (n = 3)

| Task | axi | ff-rdp |
|---|---|---|
| read_static_page | 3.0 / $0.067 / 3 | 3.0 / $0.081 / 3 |
| github_repo_stars | 3.0 / $0.100 / 3 | 4.0 / $0.096 / 3 |
| github_navigate_to_file | 4.7 / $0.135 / 3 | 4.0 / $0.094 / 3 |
| wikipedia_table_read | 5.0 / $0.138 / 3 | 4.7 / $0.084 / 3 |
| wikipedia_fact_lookup | 4.0 / $0.117 / 3 | 4.7 / $0.099 / 3 |
| multi_page_comparison | 5.7 / $0.160 / 3 | 5.3 / $0.126 / 3 |
| wikipedia_infobox_hop | 4.0 / $0.186 / 3 | **8.0** / $0.139 / 3 |
| wikipedia_link_follow | 4.0 / $0.156 / 3 | **7.3** / $0.135 / 3 |
| wikipedia_search_click | 6.7 / $0.197 / 3 | 8.3 / $0.136 / 3 |
| tabular_data_analysis | 4.0 / $0.118 / 3 | **9.3** / $0.159 / 3 |
| wikipedia_deep_extraction | 7.0 / $0.168 / 3 | **10.7** / $0.182 / 3 |
| github_issue_investigation | 9.0 / $0.204 / 3 | 10.3 / $0.194 / **2** |
| navigate_404 | **8.3** / $0.133 / 3 | 4.3 / $0.118 / 3 |
| multi_site_research | 16.0 / $0.357 / 3 | 15.0 / $0.232 / 3 |

## What the trajectories show

- **Single-page reads and multi-page-by-URL are a wash** (3–5 turns either way, ff-rdp
  usually cheaper).
- **Click-through tasks cost ff-rdp ~2× the turns, consistently across repeats.**
  `wikipedia_link_follow`: axi runs `open <url>` → `click @g10:1_186` (the ref came back inside
  `open`'s snapshot). ff-rdp runs `navigate` → `click "a[href=…]"` (guessed selector, misses)
  → `dom "a[href*=Babbage]"` → `click` → `wait … && dom ".infobox td"`. No ff-rdp command
  returns the page after acting on it, and refs only come from `dom <selector>`, which the
  agent must guess first.
- **Extraction tasks (`tabular_data_analysis` 9.3 vs 4, `deep_extraction` 10.7 vs 7,
  `github_issue_investigation`)**: the ff-rdp agent cycles through `page-text | head`,
  `dom <selector>`, and 3–6 `eval` scripts before it finds the right element. axi's agent does
  `open` + one `eval`. The one ff-rdp failure is this pattern taken to the end: on
  `github_issue_investigation` run 1 the agent's selectors returned only the `Bug:` label prefix
  of four titles and it reported those. ff-rdp has no "find the element/text matching X"
  surface — `--query` on `snapshot`/`page-text` would be one turn.
- **Non-idempotent `launch` wasted a turn in 3 of 42 runs:** first browser command
  `ff-rdp launch --headless` → exit 1 "port 6000 already in use … --replace".
- **Every run in both conditions spends turn 1 on `--help`.** Ambient context (SessionStart
  hook) or a content-first no-args view would remove it for ff-rdp; today it's symmetric.
- **Where ff-rdp wins:** `navigate_404` (4.3 vs 8.3, all three repeats) — the typed
  navigation error with HTTP status tells the agent what happened in one shot;
  `multi_site_research` (cheaper by a third) — axi's agent tangles itself in
  `CHROME_DEVTOOLS_AXI_SESSION` juggling and leaves orphaned bridges.
- ff-rdp's **cost advantage** comes from smaller uncached input per task: agents pipe ff-rdp
  JSON through `head`/`grep`, whereas axi's `open` returns a full snapshot plus hints every time.

## Conclusions for ff-rdp

Ranked by measured impact on turns:

1. `navigate --snapshot` / `click --snapshot` (or a compact snapshot in every state-changing
   result) plus refs *in* the snapshot with a generation prefix — the 8-vs-4 gap is entirely
   this.
2. `--query <text>` on `snapshot`, `a11y summary`, `page-text` — the extraction gap and the one
   failure.
3. Idempotent `launch` (no-op, exit 0, when the port owner is an ff-rdp-launched Firefox).
4. Hints on by default in JSON output; a content-first no-args view / SessionStart hook to
   remove the `--help` turn.
5. Keep the compact-output behaviour — it is why ff-rdp is cheaper despite more turns; do not
   copy axi's "always return the full snapshot" wholesale.

## Reproducing

```sh
# needs node ≥20 + pnpm (brew install node; npm i -g pnpm chrome-devtools-axi)
git clone https://github.com/kunchenguid/axi && cd axi && pnpm install --frozen-lockfile
# add the ff-rdp condition (conditions.yaml, types.ts ConditionId, lifecycle.ts health cmd
# `ff-rdp tabs`) and a start/stop script that launches one headless Firefox and kills only
# that PID — see the scratchpad copy referenced in the session memory.
cd bench-browser && npx tsx src/cli.ts matrix --condition ff-rdp,chrome-devtools-axi --repeat 3
```

Gotcha: a Firefox launched from a sandboxed Claude Code Bash tool cannot resolve DNS
(`nav_dns_fail`), while `curl` in the same shell works. Run the harness unsandboxed.

## Re-measurement 2026-08-30 — ff-rdp after iter-210 + iter-211

Same harness, same 14 tasks × 3 repeats, same agent and judge (`claude-sonnet-4-6`), same
one-paragraph system prompt ("You have the `ff-rdp` CLI … Run `ff-rdp --help`"). ff-rdp 0.3.0
built from `28695d3` (main after [[iteration-211-find-not-guess]]; [[iteration-212-ambient-context]]
was not yet merged and is not exercised — the harness runs `--help`, never bare `ff-rdp`, and
uses `--setting-sources ""` so no SessionStart hook is loaded). chrome-devtools-axi numbers are the
2026-08-29 reference, not re-run. Baseline artifacts kept in `results-baseline-0a87d1d/`.

| Condition | Success | Avg turns | Avg cost | Avg input tokens | Cache% | Avg duration |
|---|---|---|---|---|---|---|
| chrome-devtools-axi (2026-08-29) | 42/42 | 6.0 | $0.160 | 162 k | 88% | 32.5 s |
| ff-rdp @ `0a87d1d` (2026-08-29) | 41/42 | 7.1 | $0.134 | 181 k | 93% | 31.8 s |
| **ff-rdp @ `28695d3` (2026-08-30)** | **42/42** | **6.0** | **$0.121** | 152 k | 92% | 29.0 s |

ff-rdp now matches axi on turns and is 24% cheaper per task; 100% pass.

### Per task — avg turns / avg cost / passes (n = 3)

| Task | axi | ff-rdp @0a87d1d | ff-rdp @28695d3 |
|---|---|---|---|
| github_issue_investigation | 9.0 / $0.204 / 3 | 10.3 / $0.194 / **2** | **4.3** / $0.095 / 3 |
| github_navigate_to_file | 4.7 / $0.135 / 3 | 4.0 / $0.094 / 3 | 4.0 / $0.094 / 3 |
| github_repo_stars | 3.0 / $0.100 / 3 | 4.0 / $0.096 / 3 | 5.7 / $0.110 / 3 |
| multi_page_comparison | 5.7 / $0.160 / 3 | 5.3 / $0.126 / 3 | 6.3 / $0.142 / 3 |
| multi_site_research | 16.0 / $0.357 / 3 | 15.0 / $0.232 / 3 | **7.7** / $0.170 / 3 |
| navigate_404 | 8.3 / $0.133 / 3 | 4.3 / $0.118 / 3 | 5.0 / $0.112 / 3 |
| read_static_page | 3.0 / $0.067 / 3 | 3.0 / $0.081 / 3 | 3.0 / $0.073 / 3 |
| tabular_data_analysis | 4.0 / $0.118 / 3 | 9.3 / $0.159 / 3 | 6.7 / $0.128 / 3 |
| wikipedia_deep_extraction | 7.0 / $0.168 / 3 | 10.7 / $0.182 / 3 | 10.0 / $0.174 / 3 |
| wikipedia_fact_lookup | 4.0 / $0.117 / 3 | 4.7 / $0.099 / 3 | 5.3 / $0.099 / 3 |
| wikipedia_infobox_hop | 4.0 / $0.186 / 3 | 8.0 / $0.139 / 3 | 8.3 / $0.123 / 3 |
| wikipedia_link_follow | 4.0 / $0.156 / 3 | 7.3 / $0.135 / 3 | 8.3 / $0.174 / 3 |
| wikipedia_search_click | 6.7 / $0.197 / 3 | 8.3 / $0.136 / 3 | **5.3** / $0.125 / 3 |
| wikipedia_table_read | 5.0 / $0.138 / 3 | 4.7 / $0.084 / 3 | 4.0 / $0.080 / 3 |

n = 3 per task: a ±1-turn per-task change is noise. The aggregate 7.1 → 6.0 and the two large
drops are not.

### Which change paid, and which did not

- **iter-210's acceptance criterion is not met.** It asked for ≤ 5 turns on `infobox_hop`,
  `link_follow`, `search_click`; measured 8.3, 8.3, 5.3 (were 8.0, 7.3, 8.3). One of three.
- **The gains came from iter-211's extraction surface.** `page-text --query` was used 17 times
  across the 42 runs and is where `multi_site_research` (15 → 7.7) and
  `github_issue_investigation` (10.3 → 4.3, and now 3/3 passes — the baseline's one failure was
  this task) recovered their turns. `tabular_data_analysis` 9.3 → 6.7 is the same mechanism;
  `deep_extraction` 10.7 → 10.0 did not move.
- **Flag adoption** (per trajectory): `--with-page` in **13/42** runs, `--query` in **18/42**.
  Every run (42/42) opens with `ff-rdp --help`; several pipe it through `head -50`. `launch` was
  called in 2 runs, both no-ops (iter-210's idempotent launch worked; no wasted turn this time).
- **Per-run adoption map** (turns, `P` = used `--with-page`, `Q` = used `--query`):

  | Task | run 1 | run 2 | run 3 |
  |---|---|---|---|
  | github_issue_investigation | 5 PQ | 4 -- | 4 -- |
  | github_repo_stars | 4 -- | 8 -- | 5 -Q |
  | multi_page_comparison | 6 PQ | 7 PQ | 6 PQ |
  | multi_site_research | 8 -- | 8 -- | 7 -Q |
  | navigate_404 | 5 PQ | 6 -- | 4 PQ |
  | tabular_data_analysis | 5 -Q | 5 -Q | 10 -Q |
  | wikipedia_deep_extraction | 14 -- | 6 -- | 10 -- |
  | wikipedia_infobox_hop | 9 -- | 8 -- | 8 -- |
  | wikipedia_link_follow | 8 PQ | 7 PQ | 10 PQ |
  | wikipedia_search_click | 5 PQ | 5 PQ | 6 PQ |
  | (read_static_page, navigate_to_file, fact_lookup, table_read: 3–6 turns, flags mostly unused) |

- **`--with-page` did not reduce turns where it was used.** Runs with it: n = 13, 5.9 turns,
  $0.137, 164 k input tokens. Runs without: n = 29, 6.0 turns, $0.114, 147 k. Correlational, but
  the direction is clear: +12% tokens, +20% cost, no turn benefit.
- **Why — `wikipedia_link_follow` is the diagnostic case.** All three runs used `--with-page`
  and still took 8 / 7 / 10 turns: `navigate --with-page` → `click --with-page` → `dom` →
  `page-text --query "born"`. The page view (headings, landmarks, interactive refs) did come back
  after the click, but the answer — a birth date — lives in body text, which the view does not
  carry, so the agent fetched the page again. `--with-page` answers "where can I click next";
  these tasks ask "what does the page say now". axi's `open`/`click` return both.
- **`wikipedia_infobox_hop` (8.3, flag never used):** `dom '.infobox'` → `click` → `dom` →
  `navigate <url>` → `dom`. The agent clicked, could not tell from `click`'s result that it had
  arrived, and re-navigated by URL — the feedback gap `--with-page` is for, unreached because
  `--help | head -50` never shows the flag.

### Conclusions (supersede the 2026-08-29 ranking for items 1–2)

1. **Do not make `--with-page` the default yet.** No measured turn benefit, measurable token
   cost, and the view lacks what the click-through tasks need. First make the post-action view
   carry a short **text excerpt** (first ~1500 chars of `innerText`, or the `--query` window when
   given) next to the refs, then re-measure `link_follow` alone (3 runs, < $1) before deciding
   the default.
2. **Put `--query` and `--with-page` in top-level `--help`'s Quick start.** 42/42 runs read
   `--help` and nothing else; `--query` is invisible there today (the `page-text` one-liner still
   says "document.body.innerText"). Source the block from iter-212's `IDIOMS` table so
   `check-skill-drift` keeps `--help`, `SKILL.md` and the home view in sync. The home view itself
   is unreachable from this harness (`--help`, not bare `ff-rdp`; no settings sources), so it
   cannot be credited or blamed here.
3. `deep_extraction` (10.0) is the remaining extraction outlier — the agents did not use
   `--query` in any of its three runs; worth a trajectory read before adding mechanism.

## Cross-check 2026-08-30 — plain Chrome DevTools MCP vs the axi.md homepage numbers

Same harness, `chrome-devtools-mcp` 1.8.0 condition (`npx chrome-devtools-mcp@latest --headless
--isolated`, no ToolSearch), 14 × 3, `claude-sonnet-4-6`, run right after the ff-rdp
re-measurement on the same machine. Purpose: the homepage claims axi needs 4.5 turns vs 6.2 for
plain MCP; our local axi number was 6.0, so which side of that claim reproduces?

| Condition | Success | Avg turns | Avg cost | Avg input tokens | Cache% | Avg duration | **published** (70 runs) |
|---|---|---|---|---|---|---|---|
| chrome-devtools-mcp | 41/42 | **6.4** | $0.157 | 272 k | 96% | 27.9 s | 6.2 turns / $0.101 / 99% |
| chrome-devtools-axi | 42/42 | **6.0** | $0.160 | 162 k | 88% | 32.5 s | 4.5 turns / $0.074 / 100% |
| ff-rdp @ `28695d3` | 42/42 | **6.0** | $0.121 | 152 k | 92% | 29.0 s | — |

- **The MCP number reproduces** (6.4 vs 6.2 published). **The axi number does not** (6.0 vs 4.5).
  The homepage's 1.7-turn axi-over-MCP gap is 0.4 turns here, inside n = 3 noise. Costs are
  ~1.5–2× the published figures for both conditions (harness pricing table or Claude Code
  version; it affects both sides equally and does not change the ranking).
- ff-rdp is the cheapest of the three by 23–24% and ties axi on turns.

| Task | chrome-devtools-mcp | chrome-devtools-axi | ff-rdp @28695d3 |
|---|---|---|---|
| github_issue_investigation | 4.0 / $0.171 / 3 | 9.0 / $0.204 / 3 | 4.3 / $0.095 / 3 |
| github_navigate_to_file | 5.0 / $0.124 / 3 | 4.7 / $0.135 / 3 | 4.0 / $0.094 / 3 |
| github_repo_stars | 9.3 / $0.216 / **2** | 3.0 / $0.100 / 3 | 5.7 / $0.110 / 3 |
| multi_page_comparison | 7.0 / $0.146 / 3 | 5.7 / $0.160 / 3 | 6.3 / $0.142 / 3 |
| multi_site_research | 12.3 / $0.305 / 3 | 16.0 / $0.357 / 3 | 7.7 / $0.170 / 3 |
| navigate_404 | 6.0 / $0.137 / 3 | 8.3 / $0.133 / 3 | 5.0 / $0.112 / 3 |
| read_static_page | 4.0 / $0.092 / 3 | 3.0 / $0.067 / 3 | 3.0 / $0.073 / 3 |
| tabular_data_analysis | 4.3 / $0.107 / 3 | 4.0 / $0.118 / 3 | 6.7 / $0.128 / 3 |
| wikipedia_deep_extraction | 5.0 / $0.126 / 3 | 7.0 / $0.168 / 3 | 10.0 / $0.174 / 3 |
| wikipedia_fact_lookup | 4.0 / $0.094 / 3 | 4.0 / $0.117 / 3 | 5.3 / $0.099 / 3 |
| wikipedia_infobox_hop | 9.0 / $0.251 / 3 | 4.0 / $0.186 / 3 | 8.3 / $0.123 / 3 |
| wikipedia_link_follow | 8.0 / $0.173 / 3 | 4.0 / $0.156 / 3 | 8.3 / $0.174 / 3 |
| wikipedia_search_click | 7.0 / $0.151 / 3 | 6.7 / $0.197 / 3 | 5.3 / $0.125 / 3 |
| wikipedia_table_read | 4.3 / $0.104 / 3 | 5.0 / $0.138 / 3 | 4.0 / $0.080 / 3 |

- **The click-through gap is real and axi-specific.** On `infobox_hop` / `link_follow`, plain
  MCP takes 9.0 / 8.0 — the same ~8 as ff-rdp (8.3 / 8.3) — while axi takes 4.0 / 4.0 in every
  repeat. axi's act-and-return-the-page-with-refs design is what buys those four turns; neither
  ff-rdp's opt-in `--with-page` (as currently shaped) nor MCP's tool set does.
- **Extraction is where MCP beats ff-rdp**: `deep_extraction` 5.0 vs 10.0, `tabular` 4.3 vs 6.7 —
  the two tasks where ff-rdp agents did not reach for `--query`.
- ff-rdp wins the multi-site and error-path tasks (`multi_site_research` 7.7 vs 12.3 / 16.0,
  `navigate_404` 5.0 vs 6.0 / 8.3).


## 2026-08-30 — what iteration 219 changed, and what is still unmeasured

[[iteration-219-reader-view-page]] acted on the `link_follow` / `infobox_hop` diagnosis above.
**No new benchmark numbers were taken**, so the 8.3 / 8.3 figures in the table stand as the
current record for ff-rdp on those two tasks. What changed is the two defects the diagnosis
named, both verified by hand on `en.wikipedia.org/wiki/Ada_Lovelace`:

| defect (2026-08-30, `main`) | state after iteration 219 |
|---|---|
| `--with-page`'s `interactive` was the first 50 links in DOM order — all site chrome; `interactive_total: 1659`, the article's own links truncated away | Readability.js runs on the live page; entries carry `zone: "content" \| "chrome"`, content sorts first. "Charles Babbage" is now in the top 50 with a usable ref; "Jump to content" is not. `chrome_omitted: 530` reports the nav the cap dropped |
| the view carried no page text, so an agent needing "what does it say" spent a `page-text` turn anyway | `page.excerpt` carries the article text (`--page-chars`, default 1500), opening at the lede; `page.readerable` and `page.source` say what kind of page it is |
| `--query` was invisible in `ff-rdp --help`, which all 42 runs read and several piped through `head -50` | the top-level `--help` Quick start is rendered from the same `IDIOMS` table `SKILL.md` uses, so `--query` and `--with-page` are both in the first 50 lines; `xtask check-help-idioms` keeps them there |

**Adoption count for the six runs is not available**: the re-measurement was not re-run. The
`--with-page` adoption figure in the section above (used in all three `link_follow` runs, and
still 8 / 7 / 10 turns) remains the last measurement.

**A blocker for the re-run.** `click --ref <link> --with-page` — the exact trajectory
`wikipedia_link_follow` measures — times out on Wikipedia (`phase: recv`), 3 runs out of 3.
A binary built from `main` at `0a87d1d` fails identically, so it is an iter-210 defect rather
than anything iteration 219 introduced; it is filed as
[[iteration-220-with-page-after-navigating-click]]. Re-measuring the click-through tasks before
that lands would measure the timeout, not the design.

## Re-measurement 2026-08-31 — two click-through tasks after iter-219 + iter-220

ff-rdp `5a0071d` (reader view + the navigating-click fix), same harness/model/prompt,
`wikipedia_link_follow` and `wikipedia_infobox_hop` only, `--repeat 3` (6 runs, $1.16).

| Task | axi | ff-rdp @28695d3 (08-30) | **ff-rdp @5a0071d (08-31)** | per run |
|---|---|---|---|---|
| wikipedia_link_follow | 4.0 | 8.3 | **7.7** / $0.185 / 3 | 5, 10, 8 |
| wikipedia_infobox_hop | 4.0 | 8.3 | **10.3** / $0.202 / 3 | 9, 10, 12 |

- **Discoverability is solved**: `--with-page` and `--query` were used in 6/6 runs (2–5 and 3–9
  times per run); `--help | head -50` shows the idioms on lines 14–16. **No run timed out** —
  220's fix holds: `click --ref … --with-page` returned the destination in every hop that
  completed.
- **The 5-turn trajectory exists** (`link_follow` run 1): `navigate --with-page --query` →
  `click --ref e1 --with-page --query` → `page-text --query born` → answer. Four commands; the
  one `page-text` is there because the birth date is an infobox row.
- **Why the rest are 8–12**: the reader excerpt does not contain the infobox. On the Python
  article `--with-page --page-chars 4000` never mentions "Stable release" (`page-text --query`
  finds it 3×); `--with-page --query X` searches reader text only, so a miss is `matches: 0` and
  the agent's next turn is `page-text --query X` on the page it just fetched — 2–6 `page-text`
  calls per run. Filed as [[iteration-225-reader-excerpt-infobox]] (facts pass + innerText
  fallback for `--query`).
- **A new intermittent failure**: `infobox_hop` run 3's `click --ref e51 --with-page` returned
  `recv failed: Connection reset by peer` (Transport, exit 6, ~0.45 s) and the agent re-navigated
  by URL — 12 turns. Reproduced by hand at **1 in 5** on the Python → PSF hop, daemon route.
  Filed as [[iteration-224-with-page-daemon-connection-reset]].
- Verdict for 219's AC 6 and 213 Task E: measured, target (≤ 5) **not met**, recorded, not
  re-run. The mechanism works; the payload still lacks the tabular facts these tasks ask for.

## 2026-08-31 — what iteration 225 changed, and what is still unmeasured

[[iteration-225-reader-excerpt-infobox]] answers the *mechanism* half of the section above: the
reason the six runs paid 2–6 `page-text --query` round trips was that the fact each task asked for
was not in the view at all.

Shipped, and verified by unit and live tests:

- `results.page.facts` — up to 40 `{key, value}` rows read straight off the page's infobox tables,
  `<dl>` definition lists and `[itemprop]` microdata in one document-order pass. Readability is not
  consulted (forcing tables *through* it would drag navboxes back into the article element, which
  is the chrome iter-219 removed). On the Python article "Stable release" is now a fact; the
  excerpt still does not contain it, which is the point.
- `--query` searches three places, cheapest first — article text, then `facts`, then a window over
  `document.body.innerText` fetched *only* on a miss in both — and `page.query_source`
  (`readability` | `facts` | `innertext`) says which answered. A miss everywhere returns
  `matches: 0`, an empty excerpt, and a `hint` naming `page-text --full --query`. So the
  "`matches: 0` → spend a turn on `page-text --query` on the page you just fetched" loop no longer
  has a reason to happen.
- `page.matches` now counts matching excerpt lines as well as matching entries. Under iter-219 it
  counted entries only, so a hit in the prose reported `matches: 0` beside a good excerpt window.

**Not measured.** Iteration 225's Theme C — the same two tasks at `--repeat 3` — did not run: the
harness drives its agent through a browser on port 6000, and that port was held by a headless
Firefox this run had not launched (`CLAUDE.md` forbids tearing down someone else's browser, and
sharing one makes the turn counts meaningless). So the 7.7 / 10.3 above is still the current
number of record, and 225's acceptance criterion for it is unticked rather than reworded. The
re-measurement is [[iteration-228-two-task-benchmark-after-facts]]; the thing to read there is not
only the average but the count of `page-text` calls per run, which is the quantity 225 set out to
drive to zero.

## Re-measurement 2026-09-01 — the same two tasks after iteration 225's facts pass

[[iteration-228-two-task-benchmark-after-facts]]. ff-rdp `cf99c84` (`main` at the merge of
[[iteration-225-reader-excerpt-infobox]]), same harness, same two tasks, `--repeat 3`, same agent
and judge (`claude-sonnet-4-6`) and the same one-paragraph system prompt. Port 6000 was verified
free before the run (`lsof -ti :6000` empty) and the browser was started and torn down by
`ffrdp-bench.sh` alone, so this is the exclusive-browser run 225 could not get. Preconditions were
hand-checked first: on the Python article `--with-page` returns
`facts[0] = {"Stable release": "3.14.7[3] / 5 August 2026…"}`, and `--query 'Filename extensions'`
answers with `query_source: "facts"`. The facts pass works.

| Task | axi | ff-rdp @28695d3 (08-30) | ff-rdp @5a0071d (08-31) | **ff-rdp @cf99c84 (09-01)** | per run |
|---|---|---|---|---|---|
| wikipedia_link_follow | 4.0 | 8.3 | 7.7 | **9.0** / $0.164 / 3 | 13, 10, **4** |
| wikipedia_infobox_hop | 4.0 | 8.3 | 10.3 | **11.3** / $0.229 / 3 | 14, 8, 12 |

6/6 pass. Averaged over both tasks: 10.2 turns, $0.196, 41.8 s. Against the 08-31 run the averages
moved +1.3 and +1.0 — at n = 3 that is inside noise, so **the honest reading is "unchanged", not
"worse"**. The target for both ([[iteration-219-reader-view-page]] AC 6, 225's AC 3, and this
iteration's AC 3) was ≤ 5 turns. **Not met.**

### The `page-text` count — the quantity 225 set out to drive to zero

| | run 1 | run 2 | run 3 |
|---|---|---|---|
| `link_follow` — `page-text` calls | 2 | 2 | **0** |
| `infobox_hop` — `page-text` calls | 5 | 2 | 2 |

Mean 2.2 per run, range 0–5, against the 2–6 the 08-31 run showed. Essentially unmoved.
`query_source` appears in the tool output of **1 of 6 runs** (`link_follow` run 3, twice, both
`readability`). The facts pass is not being exercised.

### Why: `navigate --with-page` was used in 1 run of 6

| task | run | turns | first command | used `navigate --with-page` |
|---|---|---|---|---|
| link_follow | 1 | 13 | `ff-rdp --help \| head -50` | no |
| link_follow | 2 | 10 | `ff-rdp --help \| head -50` | no |
| link_follow | 3 | **4** | `ff-rdp --help` (whole file) | **yes** |
| infobox_hop | 1 | 14 | `ff-rdp --help \| head -50` | no |
| infobox_hop | 2 | 8 | `ff-rdp --help \| head -50` | no |
| infobox_hop | 3 | 12 | `ff-rdp --help \| head -50` | no |

The one run that used it is the whole trajectory, verbatim:

```
ff-rdp --help
ff-rdp navigate "https://en.wikipedia.org/wiki/Ada_Lovelace" --with-page --query "Charles Babbage"
ff-rdp click --ref e1 --with-page --query "born"
```

**Four turns, three commands, zero `page-text` calls — target met, axi matched.** The mechanism
225 shipped does exactly what it was built to do. It was reached once.

The other five runs share one shape: bare `navigate` → `page-text --query "<thing>"` (which
answers, but returns no ref) → a ref hunt across `a11y summary | grep`, `dom <guessed selector>`
and 1–3 `eval` scripts → `click --ref` → `page-text --query` again on the destination. That hunt
is where the 4–9 extra turns live. `infobox_hop` run 1 (14 turns) never used `--with-page` at all.

**The cause is the first 50 lines of `--help`.** Five of six runs read `ff-rdp --help | head -50`
and nothing else. In that window the `navigate` line (line 12) is bare —
`ff-rdp navigate <URL>  blocks until the document commits` — and `--with-page` appears only on
line 16, attached to `click --ref e3`. An agent that has not navigated yet has no ref, so line 16
is not yet actionable and line 15 (`page-text --query`) is. The idiom that wins,
`ff-rdp navigate <URL> --with-page --query "…"`, is at **line 333 of a 441-line `--help`** — and
the single run that read the whole file is the single run that used it.

So the 08-31 conclusion ("discoverability is solved") was measured on `--with-page` usage anywhere
in a run. Split by *where* it is used, discoverability is solved for `click` and not for
`navigate`, and `navigate` is the turn that decides whether the ref hunt happens at all.

Filed as [[iteration-230-quickstart-navigate-with-page]].

### Two smaller observations

- **A missed selector costs ten seconds of wall clock, silently.** `link_follow` runs 1 and 2 both
  guessed `click … 'a[href="/wiki/Charles_Babbage"]'` and got
  `selector … not ready — 0 elements matched (not found) after 10000ms`. Verified by hand: this
  Wikipedia render writes the attribute absolute
  (`https://en.wikipedia.org/wiki/Charles_Babbage`), so zero elements really do match — an
  agent-side guess, not an ff-rdp defect. But `click` polls the full `--timeout` before saying so,
  which is where `link_follow` run 1's 70 s went. Turn cost is 1 either way; wall-clock cost is not.
- **No transport failures.** The `recv failed: Connection reset by peer` that cost `infobox_hop`
  run 3 four turns on 08-31 did not recur in these six runs.
  [[iteration-224-with-page-daemon-connection-reset]] shipped the fix for it and is in `cf99c84`,
  so the absence is *consistent with* that fix — but six runs against a hand-reproduced 1-in-5
  would miss a surviving defect about a third of the time, so it is corroboration, not proof.

### Verdict

The measurement 225 was gated on is now taken, on a browser this run owned. `results.page.facts`
is correct and, when reached, produces the four-turn trajectory. It is reached once in six runs.
**The remaining gap on these two tasks is not the payload any more — it is which line of `--help`
the agent reads.** That is a one-line change to the Quick start block, and it must be measured the
same way before anyone claims it worked.

---

## Re-measurement 2026-09-01 (second) — after the Quick-start `navigate` line

[[iteration-230-quickstart-navigate-with-page]]. ff-rdp `30f7e7a` (branch
`iter-230/quickstart-navigate-with-page`, release build). Same harness, same two tasks,
`--repeat 3`, same agent and judge (`claude-sonnet-4-6`), same one-paragraph system prompt.
Port 6000 was verified free before the run and the browser was started and torn down by
`ffrdp-bench.sh` alone, so this is comparable to the run above. The only product change between
`cf99c84` and `30f7e7a` is which text `--help` shows: the Quick start's `navigate` row went from
`ff-rdp navigate <URL>` to `ff-rdp navigate <URL> --with-page --query "<text>"`, and
`xtask check-help-idioms` now fails if it leaves. No behaviour changed.

| Task | axi | @cf99c84 (09-01, pre) | **@30f7e7a (09-01, post)** | per run |
|---|---|---|---|---|
| wikipedia_link_follow | 4.0 | 9.0 | **4.7** / $0.109 / 3 | 5, 5, 4 |
| wikipedia_infobox_hop | 4.0 | 11.3 | **8.0** / $0.199 / 3 | 6, 9, 9 |

6/6 pass. Averaged over both tasks: **6.3 turns** (was 10.2), **$0.154** (was $0.196), 30.6 s
(was 41.8 s). `link_follow` meets the ≤ 5 target and matches axi within noise. `infobox_hop`
does not — 8.0 against a target of 5. **The target is met on one task of two; iteration 230's
AC 4 is left unticked** rather than reworded.

### Adoption — the quantity this iteration set out to move

| task | run | turns | first command | used `navigate --with-page` |
|---|---|---|---|---|
| link_follow | 1 | 5 | `ff-rdp --help \| head -50` | **yes** |
| link_follow | 2 | 5 | `ff-rdp --help \| head -50` | **yes** |
| link_follow | 3 | 4 | `ff-rdp --help` (whole file) | **yes** |
| infobox_hop | 1 | 6 | `ff-rdp --help` (whole file) | **yes** |
| infobox_hop | 2 | 9 | `ff-rdp --help \| head -50` | **yes** |
| infobox_hop | 3 | 9 | `ff-rdp --help \| head -50` | **yes** |

**6 of 6, against 1 of 6 before.** Every run that read only `head -50` now uses the idiom, which
is the mechanism the change predicted. `page-text` calls fell from a mean of 2.2 per run (range
0–5) to **1.5** (range 0–3). The hypothesis that adoption rather than payload was the gap is
confirmed on `link_follow` and only partly on `infobox_hop`.

### Why `infobox_hop` is still 8 turns — a different defect

`link_follow`'s winning shape is now the norm — three commands, no ref hunt:

```
ff-rdp --help | head -50
ff-rdp navigate ".../Ada_Lovelace" --with-page --query "Charles Babbage"
ff-rdp click --ref e1 --with-page --query "born birth"
```

`infobox_hop` runs 2 and 3 are byte-for-byte the same trajectory as each other, and they show
two costs the Quick-start line cannot touch:

```
ff-rdp navigate ".../Python_(programming_language)" --with-page --query "stable release"
ff-rdp page-text --query "Python Software Foundation"        # ref hunt, turn 1
ff-rdp a11y summary | grep -i "python software foundation"   # ref hunt, turn 2
ff-rdp dom "a[href*='Python_Software_Foundation']"           # ref hunt, turn 3 → e52
ff-rdp click --ref e52 --with-page --query "founded formed"
ff-rdp page-text --query "founded formed"                    # query miss, turn 1
ff-rdp page-text --full | head -100                          # query miss, turn 2
```

1. **Infobox `facts` carry no `ref`.** `--query "stable release"` answers from `facts`, and the
   same infobox holds `Developer: Python Software Foundation` — but `facts` is key→*text*, so the
   link the task must follow has no handle. Three turns go to recovering a ref that the payload
   that just answered the question already knew about. This is exactly the ref hunt 230 removed
   for body links (`interactive[0].ref` = `e1` on Ada Lovelace), still present for infobox links.
2. **`--query` misses a morphological near-match.** The PSF infobox key is `Formation`;
   `--query "founded formed"` matches nothing and the agent falls through to `page-text --full`.
   Two more turns.

Run 1 avoids the second cost only because it queried `"Formation"` by luck.

### Verdict

The Quick-start line did what it was measured to do: adoption 1/6 → 6/6, overall 10.2 → 6.3 turns,
`link_follow` at target and level with axi. It did **not** get `infobox_hop` to target, and the
reason is now specific and measured rather than suspected. Filed as
[[iteration-255-infobox-facts-refs-and-query-matching]].


## Re-measurement 2026-09-13 — iteration 255 facts refs and fact-key matching

Exactly two tasks × three repetitions, ff-rdp only; no smoke, ambient treatment,
axi rerun, or repeat selected for a better result. Product
`7045195f66d0663e70fc68c2364b8003f7d5318c`, tree
`47fc9414db8cbe2f5269b4aa67a4aea0f0f70cd0`, was clean and independently reviewed.
The immutable iteration-256 Theme A export
`5786329668c95479a4b91710764c7cee0e782f4a` built that exact product in a private target.
The 10 exported file hashes, Git blobs and read-only executable/data modes matched
its manifest; no harness code entered iteration 255. Binary SHA-256:
`41189615eff8b66c93a85ea51362650d3bc092390a5b9987a0b15ee6062eb7d0`.

Requested agent and judge: `claude-sonnet-4-6`. The recovered append-system-prompt
is unchanged, including its trailing newline; SHA-256
`758dfb37452f8099cfb46460ee417ccc73839cf0ee3553f7da0558229be49c8b`.
The upstream runner remains pinned at `d28c5e79aa7ee7a59a386fc34125f8cd1470fbeb`.
Actual Claude Code was **2.1.270**, not the preparation observation 2.1.259;
historical runs used **2.1.241**. Default system-prompt equivalence is therefore
unproved. Firefox 155.0.1, Node 24.19.0, pnpm launcher 11.24.0 / effective pinned
11.1.1. Existing cached account; stale `ANTHROPIC_API_KEY` omitted only in the
child environment, settings/hooks disabled. No account, model or global-setting
change was made. Actual modelUsage includes auxiliary Haiku and is counted below.

Evidence root (repository-relative):
`.git/ralph-loop/20260912-validation-efficiency/iter255-measurement/`.
`paid-evidence/` preserves 195 read-only files: exact task/condition prompts and
argv, per-invocation streams and exits, source/binary provenance, upstream results,
grades, lifecycle identities and cleanup verdict. `paid-evidence-sha256.json`
records every file hash; `audit-classified.json` records every raw terminal result,
full modelUsage, tool result, prompt hash and run-to-agent/judge identity.
`per-run-analysis.md` gives every exact command, tool-use ID and classification.
No historical table or metric was recalculated.

### Six-run result

Turns are terminal `result.num_turns`, equal to upstream `usage.turn_count` in all
six runs; commands are a separate count of unique Bash tool-use IDs. Costs are
CLI-reported list/API equivalents, **not subscription amounts billed**. Judges
are shown separately from the historical agent-cost metric.

| Task | Run | Grade | Turns | Commands | Agent USD | Judge USD | Seconds | Handle hunts | Query recovery |
|---|---:|---|---:|---:|---:|---:|---:|---:|---:|
| wikipedia_infobox_hop | 1 | PASS | 7 | 6 | 0.2345684 | 0.0864730 | 26.614 | 3 | 0 |
| wikipedia_infobox_hop | 2 | PASS | 12 | 11 | 0.1703477 | 0.0664584 | 41.780 | 4 | 4 |
| wikipedia_infobox_hop | 3 | PASS | 6 | 5 | 0.1443404 | 0.0996574 | 27.895 | 2 | 0 |
| wikipedia_link_follow | 1 | PASS | 4 | 3 | 0.1042030 | 0.0872614 | 20.979 | 0 | 0 |
| wikipedia_link_follow | 2 | PASS | 4 | 3 | 0.0705151 | 0.0533304 | 16.302 | 0 | 0 |
| wikipedia_link_follow | 3 | PASS | 4 | 3 | 0.0703827 | 0.0530924 | 16.639 | 0 | 0 |

| Task | Previous iteration 230 turns | Current mean turns | Mean agent USD | Mean judge USD | Mean seconds | Grades |
|---|---:|---:|---:|---:|---:|---|
| wikipedia_infobox_hop | 8.0 | 8.333 | 0.1830855 | 0.0841963 | 32.096 | 3/3 |
| wikipedia_link_follow | 4.7 | 4.000 | 0.0817003 | 0.0645614 | 17.973 | 3/3 |

Overall: 37 turns / 6 = **6.167**, 31 commands, mean agent cost **$0.1323929**,
mean duration **25.035 s**. Agent total **$0.7943573**, judge total **$0.4462730**,
combined list cost **$1.2406303**. Agent modelUsage splits into Sonnet
$0.7882203 + Haiku $0.0061370; judge splits into Sonnet $0.4023420 + Haiku
$0.0439310. Full per-run token/cache/thinking/model records remain in the audit;
the upstream reasoning_tokens field remains zero even though raw modelUsage
records thinkingTokens, so it is not relabeled as a measured absence of thinking.

All 12 agent/judge invocations have one terminal `subtype: success`,
`is_error: false`, child exit 0 and recorder exit 0; all six agent stream exits
are 0, with no missing result, timeout, interruption, malformed JSON, duplicated
run or unmatched invocation. All six per-run provenance copies match. Matrix,
cleanup and external shell each independently exited **0**. The lifecycle log
records six private Firefox starts and the corresponding daemon/Firefox stops;
all owned PIDs are gone, port 6000 is free, and desktop Firefox PID 37270 survives.
Execution wrapper: 2026-09-13 18:49:39–18:53:23 UTC (20:49:39–20:53:23 CEST).

### Adoption and the remaining work

All six initial navigations used `--with-page --query`; all six destination
actions did too. Every first tool was `ff-rdp --help` (whole help in infobox run 3
and link-follow run 1; `head -50` in the other four). All three link-follow runs
used initial interactive ref `e1`, then `click --ref e1 --with-page --query born`:
4/4/4 turns, zero ref hunts, zero recovery commands. No regression is observed
in this three-run control; it does not establish statistical equivalence.

Infobox runs 1 and 3 queried `stable release`, so only that fact and its links
survived filtering; the Developer fact was outside the query. Run 2 queried
`stable release infobox developer` as one literal string and got zero matches,
with `query_fact_keys` explicitly listing Developer and Stable release. No run
consumed a Developer fact ref. The new handle exists when that fact is retained,
as the live tests and dogfood demonstrate, but these trajectories never requested
it through a page view. This is an observed discovery gap, not evidence that a
returned fact ref failed.

Handle hunts below count commands seeking a handle for the task's already named
PSF target, including unsuccessful text/a11y attempts and the URL recovery in run
2. They do not count all post-navigation commands as hunts. Run 3's DOM hunt
ended in a CSS-selector click, not a numeric-ref click. Run 2 ended in URL
navigation, not a click. Exact commands and tool-use IDs:

**Infobox run 1 — 3 handle hunts.**

- Command 3, `toolu_012TwpKuaYe7TR1D3CGoY2r8`: `ff-rdp page-text --query "Python Software Foundation" 2>&1`
- Command 4, `toolu_01JGsWgVtxZGC1dwPab1bQ1k`: `ff-rdp a11y summary 2>&1 | grep -i "Python Software Foundation" | head -20`
- Command 5, `toolu_014dDQyoaUFyhcksKDfcAwBh`: `ff-rdp dom "a[href*='Python_Software_Foundation']" 2>&1 | head -40`

**Infobox run 2 — 4 handle hunts.**

- Command 7, `toolu_01GJ7XxuEKUM6nPo9xWpi5Qm`: `ff-rdp page-text --query "Python Software Foundation" 2>&1`
- Command 8, `toolu_011a28jmFpMjVzEVLGU1r2bA`: `ff-rdp a11y summary 2>&1 | grep -i "python software foundation" | head -20`
- Command 9, `toolu_01VX3ZqQ4Sz7y2G5iYZ9Exat`: `ff-rdp eval "Array.from(document.querySelectorAll('.infobox a')).map(a => ({text: a.textContent.trim(), href: a.href})).filter(a => a.text.includes('Python Software Foundation'))" 2>&1`
- Command 10, `toolu_01LAqf4JPaGHLoyHBnLqySAJ`: `ff-rdp eval "JSON.stringify(Array.from(document.querySelectorAll('.infobox a')).map(a => ({text: a.textContent.trim(), href: a.href})).filter(a => a.text.includes('Python Software Foundation')))" 2>&1`

**Infobox run 3 — 2 handle hunts.**

- Command 3, `toolu_01MDks1Qj9osHEq1j5UCRjBA`: `ff-rdp page-text --query "Python Software Foundation" --format text`
- Command 4, `toolu_01PSsW3WCvBhFHmroSGM5u7N`: `ff-rdp dom "a[href*='Python_Software_Foundation']" --text-attrs --format text`

Run 1: text returned no handle, a11y grep returned nothing, DOM supplied e54.
Run 2: text returned no handle, a11y grep returned nothing, the first eval returned
object grips without href values, and JSON.stringify recovered the URL. Run 3:
text returned no handle; DOM text/attrs plus its hint supplied the selector used
for the click. Thus ref/handle hunting persists at 3/4/2 commands, mean **3.0**.

Separately, infobox run 2 commands 3–6 recover the initial compound-query miss:
`page-text --query "Stable release Developer"` (`toolu_01DmSadgTpkpzJtj51zbJ2VJ`)
also misses; `snapshot` filtered through grep
(`toolu_01KccfrawC4orjZfF4mj9qPN`) returns nothing;
`dom --selector ".infobox"` (`toolu_01F4L2x4Jg63GngQ8Eb4T2jp`) is a CLI
argument error; corrected `dom ".infobox"` (`toolu_012AEyNaURtzxVRiamFyvQD1`)
recovers the information. These four commands are query recovery, not ref hunts.
The full pipelines are in the exact command trace. The error is preserved at
`paid-evidence/results/ff-rdp/wikipedia_infobox_hop/run2/agent_output.txt:18`:
`error: unexpected argument '--selector' found`. Its `2>&1 | head -100`
pipeline returned tool_result.is_error false; upstream error_count is 0 in all
runs. Neither that zero nor the passing judge grade means no CLI error occurred;
the inner CLI exit status was not separately recorded.

The final PSF action returned `Formation: March 6, 2001` immediately in all three
runs (`formed`, `formed founded`, `formed` respectively), with no destination
query-recovery commands. JSON runs report matches 1/query_source facts; the text
run prints the Formation fact. This directly exercises the bounded key vocabulary.
Run 2's judge passed the correct answer despite URL navigation replacing the
requested click; preserve that raw grade and this task-fidelity caveat separately.
The three-run result does not prove all required task actions were followed.

**Verdict:** infobox **8.333** versus previous **8.0**, target ≤5 **not met**.
The small mean difference and changed CLI do not support a causal regression
claim. Formation recovery disappeared in this sample; the fact-handle mechanism
was not adopted. Iteration 255's measurement criterion is fulfilled and its
original numeric target remains unticked. Remaining discovery/query-recovery and
task-fidelity work is filed in
[[iteration-269-infobox-discovery-after-fact-refs]] before merge. No prompt,
query/default/help behavior or historical metric was changed to improve the score.

## Outcome — final frozen-source measurement, 2026-09-13

Exactly one baseline matrix and one separately labeled **real private SessionStart**
matrix ran, each 14 task definitions × 3 repetitions =42 runs. All 84 result rows
and168 agent/judge invocations are retained and uniquely associated by actual
agent session/output and the judge's exact formatted trajectory. No missing,
duplicate, unmatched, timed-out, interrupted or replaced invocation was found.
No axi rerun, smoke, preparation-only call or new capability probe was added.
Earlier 255/smoke/probe records remain separate and are not pooled into these tables.

Both conditions used clean product/harness HEAD
`8c400376aa1d8ee85bc0bcee2a7bb2a898a9e277`, tree
`06b2ea52b06cee1ed28d172ac7be229ab11e5d7a`. The source stayed clean through both
matrices and their cleanup; KB changes began afterward. Each setup built its own
private debug binary: baseline SHA-256
`dd9d838db6bb32bdfb4452515a2da219cde9059db90faec67608564274135de9`, treatment
`6eada0b705cbaaee7a2069f281b1968b0eb17bf12f8612d87a2fcbc3a94bb778`.
These are distinct build artifacts of the same source, not a claim of identical
binary bytes. Both report ff-rdp 0.3.0 /8c400376aa1d /2026-09-13.

Task definitions, conditions, harness hashes, every per-task prompt hash and the
append paragraph are byte-identical across conditions. The append hash remains
`758dfb37452f8099cfb46460ee417ccc73839cf0ee3553f7da0558229be49c8b`, including
its newline; it still explicitly says to run `ff-rdp --help`. Both request agent
and judge `claude-sonnet-4-6`, with actual Claude Code **2.1.270**, Firefox 155.0.1,
Node 24.19.0, pnpm launcher 11.24.0 / pinned effective 11.1.1. Historical CLI 2.1.241's
default prompt is **not proven identical**. Cached-account selection was retained;
the stale API key was omitted only for child processes. Costs below are reported
**list/API equivalents, not subscription billing**.

### Delivery and execution evidence

The treatment uses the exact binary's installed `ff-rdp home --hook` command,
wrapped for recording in a private per-agent `--settings` file while retaining
`--setting-sources ""`. All 42 treated agents have hook input, stdout, stderr,
exit 0 and exactly one matching successful SessionStart runtime response. This is
installed-hook execution, not hook text pasted into the append prompt. Baseline
agents and all 84 judges have no treatment settings or hook events. Every original
and delivered argv was checked; only treatment settings and the documented judge
JSON-output instrumentation differ.

All 168 invocations have one terminal success, `is_error:false`, child/recorder/
interruption exits 0; all 84 agent stream exits are 0. Both matrix/cleanup/driver
triples are 0/0/0. This does **not** mean every task or command passed.
Each lifecycle records 42 private Firefox starts and84 matching daemon/browser
terminations; no owned process identity survives, port 6000 is free and desktop
Firefox PID 37270 is preserved. Baseline driver ran20:25:13–20:48:09 UTC;
SessionStart ran20:48:45–21:12:25 UTC. These wall periods include setup and judges;
per-task seconds below are the unchanged upstream agent wall metric.

Evidence root: `.git/ralph-loop/20260912-validation-efficiency/iter256-measurement/`.
`baseline/` and `session-start/` retain 3587 raw files, hashes in
`raw-evidence-sha256.json`; `evidence-verification.json` records coverage,
provenance, argv/prompt equality, hook delivery and cleanup. The two
`*-audit-classified.json` files retain every actual top-level first ID/name/command,
automatic and adjudicated categories, its own result/errors, all raw tool results,
modelUsage and agent/judge associations. `per-run.csv` and `per-run-analysis.md`
cover every run's turns, Bash command count, grade, separate/combined costs,
seconds, errors, adoption and task-fidelity caveats. Upstream `turn_count` equals
the terminal `num_turns` in all 84 runs. Bash tools are counted by unique top-level
IDs; baseline's two additional Read tools explain why186 Bash tools are fewer
than230−42. No nested-agent tool was observed. Reasoning-token zero in upstream
usage is not proof of no thinking; raw modelUsage remains authoritative.

### Aggregate results (42 runs per condition)

| Condition | Passes | Turns / mean | Bash tools | Agent USD total | Judge USD total | Combined USD total | Agent seconds/run | P/Q runs |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| Baseline (B) | 41/42 | 230 /5.476 | 186 | 4.0562285 | 2.7425790 | 6.7988075 | 22.636 | 40/38 |
| SessionStart (S) | 41/42 | 293 /6.976 | 251 | 4.3384146 | 2.3922554 | 6.7306700 | 25.585 | 5/38 |

Combined final-matrix list cost: **$13.5294775** (agents 8.3946431,
judges 5.1348344). Average agent costs are B $0.0965769 / S $0.1032956;
average combined costs B $0.1618764 / S $0.1602540. The small combined-cost decrease
comes from judges, while agent turns/cost increased. No statistical or billing
claim follows from this small sequential sample.

| Condition / role | Sonnet list USD | Auxiliary Haiku list USD |
|---|---:|---:|
| B agent | 4.0139295 | 0.0422990 |
| B judge | 2.4909000 | 0.2516790 |
| S agent | 4.2961356 | 0.0422790 |
| S judge | 2.1775854 | 0.2146700 |

Actual modelUsage keys are `claude-sonnet-4-6` and
`claude-haiku-4-5-20251001`; requested Sonnet does not imply exclusive Sonnet use.

### Per-task results

B is unchanged baseline; S is real SessionStart. P/Q counts runs that actually
used `--with-page` / `--query`, not strings printed by help or hooks. Each row has
three runs; no failed grade is removed from its means or denominator.

| Task | Condition | Turns per run | Mean turns | Agent USD/run | Judge USD/run | Combined USD/run | Seconds/run | Passes | P/Q runs |
|---|---|---|---:|---:|---:|---:|---:|---:|---|
| github_issue_investigation | B | 5/4/5 | 4.667 | 0.1238554 | 0.1057307 | 0.2295860 | 45.662 | 2/3 | 3/3 |
| github_issue_investigation | S | 9/6/4 | 6.333 | 0.1069458 | 0.0729174 | 0.1798632 | 25.646 | 3/3 | 0/3 |
| github_navigate_to_file | B | 5/5/5 | 5.000 | 0.0882014 | 0.0646423 | 0.1528437 | 18.578 | 3/3 | 3/3 |
| github_navigate_to_file | S | 4/4/4 | 4.000 | 0.0710173 | 0.0566461 | 0.1276634 | 13.557 | 3/3 | 0/3 |
| github_repo_stars | B | 5/5/5 | 5.000 | 0.0731537 | 0.0476857 | 0.1208394 | 17.097 | 3/3 | 3/3 |
| github_repo_stars | S | 4/7/4 | 5.000 | 0.0801519 | 0.0555887 | 0.1357406 | 15.556 | 3/3 | 0/3 |
| multi_page_comparison | B | 4/4/4 | 4.000 | 0.0848350 | 0.0694580 | 0.1542930 | 16.005 | 3/3 | 3/3 |
| multi_page_comparison | S | 6/6/6 | 6.000 | 0.0868371 | 0.0508027 | 0.1376398 | 20.884 | 3/3 | 0/3 |
| multi_site_research | B | 11/8/9 | 9.333 | 0.1186796 | 0.0702250 | 0.1889046 | 30.855 | 3/3 | 3/3 |
| multi_site_research | S | 10/12/8 | 10.000 | 0.1456267 | 0.0697021 | 0.2153288 | 34.032 | 3/3 | 1/3 |
| navigate_404 | B | 4/6/4 | 4.667 | 0.0904887 | 0.0698680 | 0.1603567 | 25.275 | 3/3 | 2/0 |
| navigate_404 | S | 5/5/5 | 5.000 | 0.0695257 | 0.0410037 | 0.1105294 | 20.781 | 3/3 | 0/0 |
| read_static_page | B | 4/5/5 | 4.667 | 0.0712805 | 0.0493270 | 0.1206075 | 13.406 | 3/3 | 2/2 |
| read_static_page | S | 4/4/4 | 4.000 | 0.0496229 | 0.0303967 | 0.0800196 | 12.071 | 3/3 | 0/3 |
| tabular_data_analysis | B | 7/7/6 | 6.667 | 0.1042654 | 0.0609807 | 0.1652461 | 23.819 | 3/3 | 3/3 |
| tabular_data_analysis | S | 7/4/5 | 5.333 | 0.0830323 | 0.0524929 | 0.1355253 | 21.101 | 3/3 | 0/3 |
| wikipedia_deep_extraction | B | 8/6/9 | 7.667 | 0.1493763 | 0.0968153 | 0.2461916 | 29.779 | 3/3 | 3/3 |
| wikipedia_deep_extraction | S | 11/6/6 | 7.667 | 0.1161492 | 0.0653791 | 0.1815283 | 25.664 | 3/3 | 0/3 |
| wikipedia_fact_lookup | B | 3/3/3 | 3.000 | 0.0492921 | 0.0418760 | 0.0911681 | 11.608 | 3/3 | 3/3 |
| wikipedia_fact_lookup | S | 3/3/3 | 3.000 | 0.0415150 | 0.0328664 | 0.0743814 | 10.131 | 3/3 | 0/3 |
| wikipedia_infobox_hop | B | 7/7/6 | 6.667 | 0.1497300 | 0.0718503 | 0.2215804 | 25.178 | 3/3 | 3/3 |
| wikipedia_infobox_hop | S | 11/9/7 | 9.000 | 0.1271686 | 0.0610274 | 0.1881960 | 26.112 | 3/3 | 1/3 |
| wikipedia_link_follow | B | 4/4/4 | 4.000 | 0.0910577 | 0.0748993 | 0.1659570 | 16.725 | 3/3 | 3/3 |
| wikipedia_link_follow | S | 15/9/7 | 10.333 | 0.1559121 | 0.0718567 | 0.2277689 | 53.474 | 2/3 | 1/3 |
| wikipedia_search_click | B | 7/6/6 | 6.333 | 0.0866817 | 0.0465080 | 0.1331897 | 25.434 | 3/3 | 3/3 |
| wikipedia_search_click | S | 10/12/16 | 12.667 | 0.1808988 | 0.0788957 | 0.2597945 | 45.543 | 3/3 | 2/2 |
| wikipedia_table_read | B | 5/5/5 | 5.000 | 0.0711787 | 0.0443267 | 0.1155054 | 17.478 | 3/3 | 3/3 |
| wikipedia_table_read | S | 9/8/11 | 9.333 | 0.1317348 | 0.0578427 | 0.1895775 | 33.636 | 3/3 | 0/3 |

### First actual tool, adoption and targets

All 84 first **top-level assistant tool_use** blocks were inspected, excluding
hook/system/stream events, judge/lifecycle calls and nested tools. The conservative
recorder automatically returned B:6help/36other; S:2browser/40other. Manual review
resolves quoted URLs, redirection, help pipelines and the exact private binary path
from each original command and its own result; no later success replaces that result.

| First-call classification (full42 denominator) | Baseline | SessionStart |
|---|---:|---:|
| Help | 40 (95.238%) | 0 |
| Recognized browser-command attempts | 2 (4.762%) | 40 (95.238%) |
| Other (nonexistent `nav`) | 0 | 2 (4.762%) |
| Bare ff-rdp / missing / missing own result | 0 /0 /0 | 0 /0 /0 |
| First calls with actual usage errors | 1 | 5 |
| **Successful browser-command-first** | **1 (2.381%)** | **37 (88.095%)** |

B static-page1 begins `navigate … && ff-rdp content`; navigate succeeds, but the
nonexistent second command makes the first tool fail with exit 2. S multi-site1
and search 1/2 attempt `navigate --url` and fail with exit 2; S search 3 and infobox 2
attempt nonexistent `nav` and remain other. All five S errors are visible even
though final invocations succeeded. Thus212's ≥80% target is met by **37/42
successful browser-first calls with an installed hook**, not by counting errors
or merely the40 recognized attempts. It is an adoption result, not evidence of
better task efficiency.

- **210 click-through ≤ 5 on all three is still not met.** Baseline infobox/link/
  search means6.667/4.000/6.333; treatment 9.000/10.333/12.667. Only B link meets it.
- **211 extraction targets remain partly unmet.** B tabular 6.667 and deep 7.667
  miss≤ 6, issue investigation 2/3 misses 3/3. S tabular 5.333 meets≤ 6, deep 7.667
  misses it, issue investigation 3/3 meets its pass target. All 18 extraction
  trajectories across those three tasks and both conditions used --query.
- B uses --with-page in 40/42 runs and S in 5/42; --query is38/42 in each.
  Every run's binary adoption is recorded in the per-run artifacts. Initial
  navigation uses --with-page in 40/42 B runs and **0/42 S runs**. The five S
  adopters discover it later; skipping help also skips its working navigation idiom.
- B infobox 7/7/6 still hunts PSF handles3/3/2 commands and consumes no Developer
  fact ref, although all three eventual clicks and Formation views succeed.
  S infobox 11/9/7 consumes none either; only run 1 clicks successfully, while2/3
  navigate directly and recover page-text's "founded formed" miss via "2001".
  These are additional observations under269, not replacements for255's6 runs.

### Failures, action fidelity and decision

B issue-investigation 2 fails for omitting issue37617's full title; no recovery
was attempted. S link-follow2 fails for substituting direct URL navigation for
the requested click. Both failures stay in the42-run denominators. Raw grade
PASS does not certify all actions: all six Makefile runs skip the repository-root
step; all three S link-follow runs replace unsuccessful clicks with URLs (grades
PASS/FAIL/PASS); S infobox 2/3 do likewise (PASS/PASS); B search 1 types into the
box but uses a Special:Search URL after a hidden-button timeout (PASS).

Observed CLI/tool errors total **B6 in 5 runs / S19 in 14 runs**, versus upstream
error_count totals5/18. Every error has an exact tool ID and raw result in the
audit. B search 1's hidden-button timeout is masked by a pipe; S infobox 3's empty
ref is masked by `||` recovery. Other errors include invented commands, unsupported
options and invalid ref/selector/type syntax. Help text mentioning error fields
is excluded from the observed-error count. German console messages from this
raw unmanaged Firefox do not reproduce147's separate managed-launch locale AC.

**Keep --with-page opt-in; do not enable the current hook by default.** The hook
moves the first decision to browsing, but this sample adds 1.500 mean turns and
loses navigation-with-page adoption. Its actual533-byte welcome-page payload
suggests a11y/page-text/console, with no navigate-with-page or click/type syntax.
The next surface to improve is the compact **home/SessionStart next-step guidance**,
with correct act-and-see and ref/type examples, then a separately authorized
controlled validation. This is a measured design recommendation, not a default-on
experiment: neither matrix tests default-on --with-page, n=3 per task is small,
conditions ran sequentially in different upstream-randomized task orders, and
historical CLI prompt parity is unproved. No product/help/default behavior changed.

## Carry-over — final measurement and closing state

| Observation or unmet requirement | Disposition |
|---|---|
| Infobox≤ 5 miss, unconsumed fact refs, query/handle recovery and URL substitution | Folded into [[iteration-269-infobox-discovery-after-fact-refs]] with both256 conditions; original255 evidence and ACs unchanged |
| Remaining click/extraction targets, incomplete issue title, ambient action fluency, invalid commands/options and strict task-action/error audit | Filed as [[iteration-270-benchmark-ambient-action-and-extraction-gaps]]; no broader backlog execution |
| Default-on efficacy unmeasured; historical default-prompt equivalence unproved; sequential/n=3 limits | No claim or product change;270 requires a controlled design and separate authorization before any new paid comparison |
| Prior harness smokes/probe and255 measurement | Preserved separately; not pooled or erased by the84-run result |
| Prior255 sweep334passed/9failed =343 across all tiers, zero profiles | Separate255 evidence only: seven screenshot failures remain 257, promotion137 remains262, pre-auth240 remains268; this harness/KB-only 256 delta requires no product sweep |
| Independent final documentation review, exact-head CI and GitHub merge | Review `01a09cab-c897-7211-a705-70c8db47c143` completed with zero findings after 15 successful inspection calls; all iteration ACs fulfilled. Supervisor verifies final-head CI and the GitHub merge separately before queue advancement |

### Per-occurrence dispositions

Every row below remains in its original run; line numbers refer to that run's `agent_output.txt`. Full commands/results remain in the classified audit.

| Condition/task/run | Non-green observation | Evidence | Disposition |
|---|---|---|---|
| B/github_issue_investigation/2 | Upstream FAIL: incomplete issue title | grade.json and terminal answer | Filed 270; no replacement |
| B/github_navigate_to_file/1 | Skipped initial repository-root visit; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| B/github_navigate_to_file/2 | Skipped initial repository-root visit; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| B/github_navigate_to_file/3 | Skipped initial repository-root visit; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| B/navigate_404/2 | unrecognized subcommand 'content' | line 10, toolu_011KoPZZdkE6bPxc3chyYGGx | Filed/folded 270; preserve actual error |
| B/read_static_page/1 | unrecognized subcommand 'content' | line 6, toolu_01SyjZRdTdfbL9vkGVwc53U6 | Filed/folded 270; preserve actual error |
| B/wikipedia_search_click/1 | the argument '--ref <REF_ID>' cannot be used with '[SELECTOR_POS]' | line 10, toolu_01LxuiLD8ZM1iFTpKF8GcD9r | Filed/folded 270; preserve actual error |
| B/wikipedia_search_click/1 | selector '#vector-sticky-header > div:nth-child(1) > div:nth-child(1) > button:nth-child(1)' not ready — the 1 matching element is hidden after 10000ms; masked by shell | line 16, toolu_017rLwxYYs6sfCbnCRALjevc | Filed/folded 270; preserve actual error |
| B/wikipedia_search_click/1 | No successful required click/submission; URL recovery substituted; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| B/wikipedia_search_click/2 | the argument '--ref <REF_ID>' cannot be used with '[SELECTOR_POS]' | line 12, toolu_01UUb2V4WRvAu4x32KSrPhrb | Filed/folded 270; preserve actual error |
| B/wikipedia_search_click/3 | the argument '--ref <REF_ID>' cannot be used with '[SELECTOR_POS]' | line 12, toolu_01HkDRcRpr9Tti6krv8LqWP8 | Filed/folded 270; preserve actual error |
| S/github_issue_investigation/1 | unrecognized subcommand 'js' | line 20, toolu_01QBNpD3bpWWNFBQZrkek1Cq | Filed/folded 270; preserve actual error |
| S/github_navigate_to_file/1 | Skipped initial repository-root visit; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| S/github_navigate_to_file/2 | Skipped initial repository-root visit; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| S/github_navigate_to_file/3 | Skipped initial repository-root visit; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| S/multi_site_research/1 | unexpected argument '--url' found | line 10, toolu_01Wsg1yWobgWFb1KL7mRooCj | Filed/folded 270; preserve actual error |
| S/multi_site_research/2 | unrecognized subcommand 'tab-new' | line 19, toolu_01R2m8B2ioyv7KzWwjVAhQDJ | Filed/folded 270; preserve actual error |
| S/multi_site_research/2 | unrecognized subcommand 'tab-new' | line 21, toolu_01AVJBzxZe3h9msctTLViFCu | Filed/folded 270; preserve actual error |
| S/wikipedia_infobox_hop/1 | unrecognized subcommand 'find-refs' | line 21, toolu_01DHsyKDZkKXoGrP3kssHvRM | Filed/folded 269; preserve actual error |
| S/wikipedia_infobox_hop/2 | unrecognized subcommand 'nav' | line 8, toolu_01JoZQ9sDCZxiGxr6NT3waQA | Filed/folded 269; preserve actual error |
| S/wikipedia_infobox_hop/2 | No successful required click/submission; URL recovery substituted; raw grade PASS | Full command/result trace | Filed/folded 269; strict action caveat retained |
| S/wikipedia_infobox_hop/3 | resolve-ref requires a non-empty id field; masked by shell | line 15, toolu_011iTA4jH2nuZfPPxSwdigB8 | Filed/folded 269; preserve actual error |
| S/wikipedia_infobox_hop/3 | No successful required click/submission; URL recovery substituted; raw grade PASS | Full command/result trace | Filed/folded 269; strict action caveat retained |
| S/wikipedia_link_follow/1 | unrecognized subcommand 'find-ref' | line 13, toolu_01Vjpti2WS8hhJDkk7927Kqy | Filed/folded 270; preserve actual error |
| S/wikipedia_link_follow/1 | selector 'Charles Babbage' not ready — 0 elements matched (not found) after 2009ms — the page is idle (document complete, no network in flight, no DOM mutations), so the rest of the 10000ms auto-wait budget could not have changed  | line 15, toolu_014H5qPnaCDixZQZDVNUZyGJ | Filed/folded 270; preserve actual error |
| S/wikipedia_link_follow/1 | selector 'a[href='/wiki/Charles_Babbage']' not ready — 0 elements matched (not found) after 2098ms — the page is idle (document complete, no network in flight, no DOM mutations), so the rest of the 10000ms auto-wait budget could n | line 32, toolu_01L7GSPCS9VUdwPBf3fMoXQ6 | Filed/folded 270; preserve actual error |
| S/wikipedia_link_follow/1 | No successful required click/submission; URL recovery substituted; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| S/wikipedia_link_follow/2 | ref Charles Babbage not found (not registered in this daemon session) | line 18, toolu_01McbxG7ifuZJbAFNBSqeyHx | Filed/folded 270; preserve actual error |
| S/wikipedia_link_follow/2 | Upstream FAIL: URL substituted for click | grade.json and terminal answer | Filed 270; no replacement |
| S/wikipedia_link_follow/2 | No successful required click/submission; URL recovery substituted; raw grade FAIL | Full command/result trace | Filed/folded 270; strict action caveat retained |
| S/wikipedia_link_follow/3 | selector 'e209' not ready — 0 elements matched (not found) after 2009ms — the page is idle (document complete, no network in flight, no DOM mutations), so the rest of the 10000ms auto-wait budget could not have changed the answer | line 14, toolu_013KK1KkRL4NYUZ6JLvJyhgm | Filed/folded 270; preserve actual error |
| S/wikipedia_link_follow/3 | No successful required click/submission; URL recovery substituted; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| S/wikipedia_search_click/1 | unexpected argument '--url' found | line 8, toolu_01W2548LvTNGtygtjZUEyWfx | Filed/folded 270; preserve actual error |
| S/wikipedia_search_click/2 | unexpected argument '--url' found | line 8, toolu_01RSCfFj6gPGy4RYDHUucnNK | Filed/folded 270; preserve actual error |
| S/wikipedia_search_click/3 | unrecognized subcommand 'nav' | line 8, toolu_01Bxe6meQkjXpvBmcpxX6wkJ | Filed/folded 270; preserve actual error |
| S/wikipedia_search_click/3 | unexpected argument '--value' found | line 24, toolu_01JmJnsLi5vLLN2baNxfooqU | Filed/folded 270; preserve actual error |
| S/wikipedia_search_click/3 | selector '#searchform button[type=submit]' not ready — 0 elements matched (not found) after 2132ms — the page is idle (document complete, no network in flight, no DOM mutations), so the rest of the 10000ms auto-wait budget could n | line 30, toolu_01LFgqS3VbBXrUoTd2oDvWyb | Filed/folded 270; preserve actual error |
| S/wikipedia_table_read/1 | unexpected argument '--expression' found | line 20, toolu_01Vspx3rqH7eNk9q2rjdBZus | Filed/folded 270; preserve actual error |
| S/wikipedia_table_read/3 | unexpected argument '--expression' found | line 20, toolu_01Xkg1CXi2YM9Z6GgWN1hQ7S | Filed/folded 270; preserve actual error |

Original acceptance wording and historical tables are unchanged. The record-only
measurement ACs are fulfilled despite numeric target misses; no predecessor's
unticked numeric criterion is silently ticked. Fresh ordered stable update,
fmt, strict workspace clippy and workspace tests passed on 2026-09-13 for the
final measurement documentation: 2466 passed, 0 failed, 415 ignored over 36
summaries; clippy 0.1.98 (48a229ceae 2026-09-01). Evidence is retained in
`iter256-measurement-review/ordered-gates.json` beneath the same run root.
All nine xtask gates and actual dogfood remain applicable to unchanged harness
inputs, with prior reuse proofs retained. Affected documentation/plan checks
are rerun after completion-only status and evidence updates. All upcoming
planned iterations, including outside 252–257, have property/body snapshots,
hashes and dispositions in `upcoming-reconciliation.json`; inspecting them does
not execute them. Earlier preparation records describe their then-pending
state and are superseded by this final measurement record.
