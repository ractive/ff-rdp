---
title: "Iteration 43: Pre-Release DX Fixes"
date: 2026-04-13
type: iteration
status: planned
branch: iter-43/dx-fixes
tags: [iteration, dx, eval, screenshot, console, navigate, reload, computed]
---

# Iteration 43: Pre-Release DX Fixes

Last iteration before public release ([[iterations/iteration-44-public-release]]). Addresses the concrete friction captured in [[dogfooding/dogfooding-session-nova-template-jsonforms-index]]. Everything here is cheap, scoped, and removes the "I kept reinventing this" reflexes a first-time public user would hit within minutes.

## Source of Truth

[[dogfooding/dogfooding-session-nova-template-jsonforms-index]] is the ground truth. The user did a real styling task, hit these blockers in order. Ship fixes that make the same session smoother.

## Tasks

### 1. `eval`: accept scripts from file or stdin

**Why:** Optional chaining (`?.`) and multi-statement JS fail when passed as a shell argument (SyntaxError at col 1). Root cause is almost certainly shell quoting, not the JS engine — but the workaround (rewriting without `?.`) wastes time and is unreasonable to ask of users. A `--file` / `--stdin` path sidesteps all shell-level gotchas.

- [ ] `ff-rdp eval --file script.js` reads the file and sends its contents as the JS source.
- [ ] `ff-rdp eval --stdin` reads from stdin until EOF.
- [ ] Make `<SCRIPT>` positional optional when `--file` or `--stdin` is used; error if multiple sources are provided.
- [ ] e2e test covering: `echo 'getComputedStyle(document.body)?.display' | ff-rdp eval --stdin` against a recorded fixture.
- [ ] Update `eval --help` to list the three input modes with one-line examples.

### 2. `screenshot --full-page` and `--viewport-height`

**Why:** Long pages are cut off at the first fold. The dogfooder worked around it; a first-time user will just consider the command broken.

- [ ] Implement `--full-page`: take the screenshot at `document.scrollingElement.scrollHeight` (or the Firefox RDP equivalent if the screenshot actor exposes one directly).
- [ ] Implement `--viewport-height N` as a lower-level escape hatch if `--full-page` isn't always desired.
- [ ] Record a live fixture of a tall page (use the WohnungsDirekt fixture from [[iterations/iteration-42-site-audit-skill]] if it's been landed) and assert returned PNG height ≥ 2000 px.
- [ ] Error if both flags are given together.

### 3. `computed` command

**Why:** The dogfooder wrote `getComputedStyle(sel)[prop]` four times in one session. This is *the* most reused eval snippet in CSS debugging — it should be a first-class command.

- [ ] New subcommand: `ff-rdp computed <SELECTOR> [--prop <NAME>]`.
- [ ] With `--prop`: return a single string value. Without: return the full resolved-style object (filtered to non-default values to keep output readable — or behind `--all` to dump everything).
- [ ] Multi-match behaviour: mirror `dom` — return an array, one entry per match, each with `{selector, index, computed: {...}}`.
- [ ] Bypass the daemon (per iter-40 pattern — this is a one-shot eval wrapper, not a stream).
- [ ] Unit test + e2e test against a recorded fixture.
- [ ] Document in `README.md` usage table.

### 4. `console` summary + honour `--limit`

**Why:** `--limit 20` appeared to return 495 messages. Either the flag is ignored, capped elsewhere, or the user was reading a different count (total vs. shown). Either way, output needs a summary so users know at a glance *"did the filter catch what I wanted?"*.

- [ ] Audit: verify `--limit N` actually truncates the results array in `console`. Fix if broken.
- [ ] Add a `summary` field to the response: `{ "summary": { "total": N, "by_level": {"error": X, "warn": Y, ...}, "shown": Z } }`.
- [ ] e2e test: populate console with ≥100 messages, run `console --limit 10`, assert `results.length == 10` and `summary.total` reflects the true total.

### 5. `reload --wait-idle`

**Why:** Dev loops currently need `sleep 5` after reload. Unreliable, platform-dependent, slow. Mirrors hyalo's wait-for-ready patterns.

- [ ] `ff-rdp reload --wait-idle [--idle-ms N] [--timeout N]`: reload, then block until either no network activity for `--idle-ms` (default 500) or the timeout expires.
- [ ] Plumb through the existing network watcher (iter-27 streaming).
- [ ] Return `{results: {reloaded: true, idle_at_ms: N, requests_observed: M}}`.
- [ ] e2e test against a page that fires a delayed fetch.

### 6. `navigate` help: fix the misleading `-- --url` hint

**Why:** clap's default "did you mean `-- --url`?" misleads users toward a flag that doesn't exist. Positional `<URL>` is the only form we accept.

- [ ] Add a one-line `long_about` on `navigate` pointing to the positional form with an example.
- [ ] If trivial, override clap's error message for unknown `--url` / `--uri` / `--href` to "URL is positional: `ff-rdp navigate <URL>`".

### 7. Defer / document as non-goals

The dogfooding session also suggested `wait --console-includes` and `screenshot --annotate`. Both are interesting but bigger — track in the backlog, out of scope for this iteration.

- [ ] Add backlog entries: `kb/backlog/wait-console-includes.md`, `kb/backlog/screenshot-annotate.md`, each 3–5 lines linking back to the dogfooding session.

## Acceptance Criteria

- [ ] Re-run the nova-template session from the dogfooding note end to end. Each of the 5 numbered issues is gone (or tracked in backlog for 7).
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` all green.
- [ ] `README.md` command table updated to include `computed` and the new flags on `eval` / `screenshot` / `console` / `reload`.
- [ ] `ff-rdp llm-help` (if that's a thing in the current codebase) reflects the new surface.

## Out of Scope

- Adding `wait --console-includes` (backlog).
- `screenshot --annotate` (backlog).
- Reworking the daemon or connection model — keep this purely additive/surface-level.
- Any release-pipeline work — that's [[iterations/iteration-44-public-release]].
