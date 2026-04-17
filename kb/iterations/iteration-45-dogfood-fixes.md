---
title: "Iteration 45: Dogfooding Fixes"
type: iteration
date: 2026-04-16
status: completed
branch: iter-45/dogfood-fixes
tags:
  - iteration
  - bugfix
  - screenshot
  - format
  - scroll
  - a11y
  - sources
---

# Iteration 45: Dogfooding Fixes

Fix issues found in [[dogfooding/dogfooding-session-37]] on sbb.ch. Prioritized by severity.

## Source of Truth

All findings from dogfooding session 37. Focus on the high/medium items that affect real-world usage.

## Tasks

### 1. Fix `screenshot --full-page` [0/4]

The `--full-page` flag is accepted but silently ignored — output is viewport-sized (683px height) instead of the full scroll height (3841px on sbb.ch). This was the headline feature of iter-43.

- [ ] Investigate why `--full-page` doesn't change capture behavior (likely the RDP `screenshot` actor call isn't receiving the full-page param, or the param name differs in this Firefox version)
- [ ] Fix the implementation to capture the entire page content
- [ ] Add an e2e test that verifies full-page screenshots are taller than viewport height
- [ ] Record updated test fixture if needed

### 2. Fix `--format text` inconsistency [0/3]

`perf summary`, `a11y`, and `responsive` output JSON even when `--format text` is specified. Other commands (`network`, `cookies`, `a11y contrast`, `styles`) correctly produce tabular text.

- [ ] Add text formatter for `perf summary` (domain breakdown table)
- [ ] Add text formatter for `a11y` tree output
- [ ] Add text formatter for `responsive` breakpoint results

### 3. Fix `--limit` ignored by `a11y` and `sources` [0/2]

Both commands return all results regardless of `--limit`. Other commands respect it.

- [ ] Wire `--limit` through to `a11y` tree output (limit depth or node count)
- [ ] Wire `--limit` through to `sources` list output

### 4. Move debug messages from stdout to stderr [0/2]

`a11y` emits "accessibility walker root methods unrecognized" and `sources` emits "sources thread actor failed" to stdout. These pollute JSON output and break piping to `--jq`.

- [ ] Route fallback debug messages to stderr for `a11y`
- [ ] Route fallback debug messages to stderr for `sources`

### 5. Fix `scroll by --dy` negative number parsing [0/2]

`scroll by --dy -99999` fails because clap parses `-9` as a flag. Only `--dy=-99999` (equals syntax) works.

- [ ] Add `allow_negative_numbers = true` to the scroll `by` subcommand in clap
- [ ] Add a test with negative `--dy` and `--dx` values

### 6. Add `scroll --top` / `--bottom` shortcuts [0/2]

Currently scrolling to top/bottom requires `--dy=-99999` / `--dy=99999` workarounds.

- [ ] Add `scroll top` and `scroll bottom` subcommands (or `--top`/`--bottom` flags on `scroll by`)
- [ ] Update help text and llm-help

### 7. Investigate `reload --wait-idle` reporting 0 requests [0/1]

`reload --wait-idle` returns `requests_observed: 0` — the watcher may not attach before the reload fires.

- [ ] Investigate timing of watcher attachment vs reload; fix if possible, or document the limitation

## Acceptance Criteria

- [ ] `screenshot --full-page` produces an image taller than viewport on a page with scroll content
- [ ] `perf summary --format text`, `a11y --format text`, and `responsive --format text` produce tabular output
- [ ] `a11y --limit 5` and `sources --limit 5` respect the limit
- [ ] `a11y` and `sources` fallback messages go to stderr, not stdout
- [ ] `scroll by --dy -500` works without equals syntax
- [ ] `scroll top` and `scroll bottom` work
- [ ] All quality gates pass: `cargo fmt`, `cargo clippy`, `cargo test`
