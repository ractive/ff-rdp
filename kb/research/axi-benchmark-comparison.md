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
