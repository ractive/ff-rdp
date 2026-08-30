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
