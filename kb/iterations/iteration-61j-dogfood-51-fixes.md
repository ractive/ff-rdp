---
title: "Iteration 61j: Dogfood-51 fixes — page-text dup, --stringify object, computed multi-prop, locale, watcher engage, --full-page, skill drift"
type: iteration
date: 2026-05-23
status: planned
branch: iter-61j/dogfood-51-fixes
depends_on:
  - iteration-61i-dogfood-49-fixes
tags:
  - iteration
  - dogfood-fix
  - page-text
  - eval-stringify
  - computed
  - network-watcher
  - screenshot-fullpage
  - skill-drift
  - locale
---

# Iteration 61j: Dogfood-51 fixes

Follow-up to [[dogfooding-session-51]]. iter-61i landed three regression fixes (same-URL navigate, dom array shape, --stringify hint). This iteration tackles the remainder: items deferred from 61i (`computed --prop` repeatable) plus new session-51 findings. Most are small ergonomic fixes; two (watcher engage, `--full-page`) are real engineering.

Themes:

- **A — `page-text` output deduplication.** Drop the duplicate `text` key; `results` is enough.
- **B — `eval --stringify` returns a real JSON object, not a string.** Parse server-side.
- **C — `computed` multi-prop.** Carried over from iter-61i E (deferred). Repeatable `--prop` + CSS custom-property names.
- **D — `dom` single-match parity.** Single-match results register refs the same way multi-match does.
- **E — `navigate` UX polish.** Bump default timeout, unify timeout error wording, detect dead Firefox before reporting success.
- **F — Firefox locale pin.** Force `intl.accept_languages=en-US` (and `LANG=C` env) in `launch` so console / error strings are predictable for LLMs.
- **G — `--with-network` actually engages the WatcherActor.** Currently `network` always falls back to performance-api → no response headers → cannot inspect CSP/HSTS/Set-Cookie. Major gap for security workflows.
- **H — `network` format parity.** JSON and `--format text` must agree on which fields are populated.
- **I — `screenshot --full-page` fix.** Three sessions running. Finish the chrome-scope prepareCapture-rect plumbing deferred from iter-61h PR #73.
- **J — Skill/doc drift.** The dogfood skill mentions `ff-rdp llm-help` and `ff-rdp recipes` (neither exists). Either add them as thin wrappers or update the skill. Also fix the `computed` example.

Out of scope (own iteration):
- **Cookie decoder hints** (auto-detect base64 cookie values and annotate) — nice-to-have, not core.
- **`ff-rdp headers <url>`** dedicated subcommand — if G is fixed, the data is reachable through `network --detail --headers`.

## Tasks

### A. `page-text` output deduplication

#### A1. Remove the `text` key from `page-text` output [0/3]
- [ ] In `crates/ff-rdp-cli/src/commands/page_text.rs` (or wherever the response is shaped), drop the `text` field. `results` already holds the string.
- [ ] Update the help text / "Output:" stanza in `--help` so it documents the single-field shape.
- [ ] Snapshot test the new shape.

#### A2. Migration note [0/1]
- [ ] Mention in the README / kb/ release notes that `.text` is gone — `--jq .results` is the replacement. (`.text` was undocumented in `--help`; risk is small.)

### B. `eval --stringify` returns a real JSON object

#### B1. Parse server-side [0/3]
- [ ] After Firefox returns the JSON.stringified value, `serde_json::from_str` it on the ff-rdp side and embed the parsed value directly under `results`.
- [ ] If parsing fails (e.g. caller wrapped it in another `JSON.stringify`), fall back to the current string output and add a `meta.stringify_parsed: false` flag.
- [ ] Update `eval --help` to document that `--stringify` now returns a parsed object.

#### B2. Tests [0/2]
- [ ] Unit: `--stringify '({a:1, b:[2,3]})'` returns `results: {a:1, b:[2,3]}` (object, not string).
- [ ] Unit: malformed-stringified case keeps the string and sets `meta.stringify_parsed = false`.

### C. `computed --prop` repeatable + custom properties

(Lifted verbatim from iter-61i §C, which deferred this.)

#### C1. Multi-`--prop` repeatable [0/2]
- [ ] `clap` annotation: `--prop` becomes `Vec<String>`. Each occurrence appends one property name.
- [ ] When multiple `--prop` are passed, return `computed: {name1: value1, name2: value2}` instead of the single-string short-circuit.

#### C2. Accept CSS custom-property names like `--bg-color` [0/2]
- [ ] `clap` `--` argument separator handling: support `ff-rdp computed h1 --prop -- --bg-color` *or* document an alternative (e.g. `--prop=color,--bg-color` if a separator-safe form exists).
- [ ] If clap can't carry leading-dash values cleanly, accept a positional comma-list as a second argument: `ff-rdp computed h1 color,font-size,--bg-color`.

#### C3. Snapshot-test the new shape [0/1]
- [ ] Multi-prop and custom-prop cases both covered.

### D. `dom` single-match parity

#### D1. Always register refs [0/2]
- [ ] For single-element results, register the ref the same way multi-element results do — emit `ref: "e0"` on the entry and set `meta.refs_registered: true`.
- [ ] Unit: `dom 'title'` returns `[{tag: "title", ref: "e0", …}]` with `refs_registered: true`.

### E. `navigate` UX polish

#### E1. Default timeout bump [0/1]
- [ ] Raise the global default `--timeout` from 5000ms to 10000ms. (Or push only `navigate`'s effective default while keeping the global 5s — implementer's call. Document in `--help`.)

#### E2. Unify timeout error wording [0/2]
- [ ] Audit the two messages observed in session 51:
  - "page did not commit within Xms — use --no-wait …"
  - "operation timed out — try increasing --timeout"
- [ ] Pick one canonical message for commit-wait timeouts (the first is more actionable). Use it everywhere commit-wait fires.

#### E3. Detect dead Firefox before claiming success [0/2]
- [ ] In `navigate --no-wait`, do a cheap pre-flight RDP ping (or wrap the send in a 200ms timeout) and surface "Firefox not reachable" if the socket is gone — instead of returning `{"navigated":"…"}` success-shaped.
- [ ] Unit / live test that kills Firefox before navigate and asserts a non-success exit code.

### F. Firefox locale pin

#### F1. Force English in launched profiles [0/3]
- [ ] In `crates/ff-rdp-cli/src/commands/launch.rs`, set `intl.accept_languages=en-US, en` in the generated user.js (or the temp profile prefs).
- [ ] Also set `LANG=C.UTF-8` (or `en_US.UTF-8`) in the child process env when launching Firefox.
- [ ] Verify with a live test: after launch, the headless quirks-mode warning is in English.

### G. `--with-network` actually engages the WatcherActor

This is the heart of the iteration.

#### G1. Diagnose [0/2]
- [ ] Reproduce: `ff-rdp navigate <url> --with-network` followed by `ff-rdp network --detail --headers` returns `source: performance-api` with `status:null, method:null`. Confirm `daemon status` shows `buffer_sizes: {}` even after multiple navigates.
- [ ] Inspect the watcher-engagement code path — does `--with-network` actually subscribe the daemon to `networkEvent` resources before navigation, or does it only flip a CLI-side flag that the daemon never sees?

#### G2. Wire `--with-network` end-to-end [0/3]
- [ ] CLI → daemon protocol: when `navigate --with-network` is invoked, send an explicit "subscribe network resources" message to the daemon *before* the navigate request goes out. Daemon stores the events in its per-tab buffer.
- [ ] `network` (without `--no-daemon`) reads from the daemon buffer first, falls back to performance-api only when the buffer is empty *and* `--with-network` was not used.
- [ ] Live integration test against `demo.testfire.net`: navigate with `--with-network`, then `network --detail --headers` returns response headers including `Server` and `Set-Cookie`.

#### G3. Help text update [0/1]
- [ ] `network --help` "Recommended workflows" stanza: clarify that the watcher is only engaged when `--with-network` (or daemon-mode capture) is on. Document the failure mode and how to detect it (`source` field).

### H. `network` format parity

#### H1. Single source of truth [0/2]
- [ ] Reproduce: `network --format text` and `network` (JSON) sometimes show different `status` / `transfer_size`. Identify the divergent path.
- [ ] Make `--format text` derive from the same `Vec<Entry>` the JSON path uses, instead of building text from a different source.

### I. `screenshot --full-page` fix

#### I1. Finish the chrome-scope prepareCapture-rect plumbing [0/3]
- [ ] Resume the deferred work from iter-61h PR #73: capture in chrome scope with an explicit rect of `(0, 0, scrollWidth, scrollHeight)` rather than the viewport rect.
- [ ] Handle DPR / device-pixel-ratio so the resulting PNG dimensions match `scrollHeight * dpr` × `scrollWidth * dpr`.
- [ ] Live test on a long page (Wikipedia /HTTP, ~21k px): `screenshot --full-page` produces a PNG with `height >= scrollHeight`.

### J. Skill / doc drift

#### J1. Decide: add `llm-help` and `recipes` subcommands or fix the skill [0/2]
- [ ] Pick one:
  - **Option A** — add `ff-rdp llm-help` (concatenated `--help` for every subcommand, plain text, optimised for LLM context) and `ff-rdp recipes` (curated example flows). The dogfood skill already advertises them.
  - **Option B** — remove the references from `.claude/skills/dogfood/SKILL.md`.
- [ ] If Option A: each command needs at least one snapshot test against `--help` drift.

#### J2. Fix the dogfood skill's `computed` example [0/1]
- [ ] In `.claude/skills/dogfood/SKILL.md`, replace `computed ".some-element" display,font-size,color` with the actual working syntax (`computed h1 --prop color --prop font-size` after C lands, or `--prop color` for now).

## Acceptance Criteria [0/12]

- [ ] **A.** `ff-rdp page-text` output contains only the `results` key for the text body.
- [ ] **B.** `ff-rdp eval --stringify '({a:1})'` returns `results: {"a":1}` (object), not a string.
- [ ] **C.** `ff-rdp computed h1 --prop color --prop font-size` returns both values.
- [ ] **C.** CSS custom property names (`--bg-color`) are reachable.
- [ ] **D.** `ff-rdp dom 'title'` (single match) returns a ref the same way multi-match does, and `meta.refs_registered` is `true`.
- [ ] **E.** `--timeout` default is ≥10s OR navigate's effective default is 10s; the timeout error message is consistent across paths.
- [ ] **E.** A navigate against a dead Firefox returns a non-success exit code with a clear "not reachable" message.
- [ ] **F.** A freshly-launched headless Firefox emits English console messages (locale-pinned).
- [ ] **G.** `ff-rdp navigate <url> --with-network` followed by `ff-rdp network --detail --headers` returns response headers (Server, Set-Cookie, etc.) for the navigated document.
- [ ] **H.** `network --format text` and JSON output show the same `status` / `transfer_size` / `method` fields for the same request.
- [ ] **I.** `screenshot --full-page` on a 20k-px page produces a PNG with height matching `scrollHeight` (× DPR).
- [ ] **J.** Either `ff-rdp llm-help` / `ff-rdp recipes` exist, or the dogfood skill no longer references them. `computed` example in the skill matches the CLI.

## Design Notes

- **B (eval --stringify parsing)**: be careful — the current behavior returns a string with `\"` escapes. Some agents (and the existing test corpus) may already JSON.parse it themselves. Consider a one-release deprecation: keep returning the string but add a parsed copy under `meta.parsed`, then flip in a follow-up. Decision left to implementer; default plan is to flip directly and call it out in release notes since `--stringify` is a recent flag.
- **G (watcher engage)** is the biggest risk. If the daemon protocol doesn't have a subscribe-resources message yet, this turns into a small daemon-protocol change. Budget accordingly; if it grows, split into a dedicated iter-61k.
- **I (`--full-page`)** is well-trodden ground (iter-61h PR #73 deferred it explicitly). The chrome-scope screenshot path already exists; only the rect computation and DPR handling are missing.
- **F (locale)** likely affects existing fixtures — any recorded fixture with German console text needs regeneration after this lands. Run `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored` if needed.

## References

- Source: [[dogfooding-session-51]]
- Previous: [[iteration-61i-dogfood-49-fixes]] (deferred C lifted here)
- Related: [[iteration-61h-headless-screenshot-firefox150]] (`--full-page` chrome-scope work)
