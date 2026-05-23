---
title: "Iteration 61k: Dogfood-52 fixes — full-page screenshot, locale pin, network watcher path, computed shape, navigate neterror, CSP-safe eval, shadow DOM, --fields on tabs"
type: iteration
date: 2026-05-23
status: planned
branch: iter-61k/dogfood-52-fixes
depends_on:
  - iteration-61j-dogfood-51-fixes
tags:
  - iteration
  - dogfood-fix
  - screenshot-fullpage
  - locale
  - network-watcher
  - computed
  - navigate
  - eval-csp
  - shadow-dom
  - fields
---

# Iteration 61k: Dogfood-52 fixes

Follow-up to [[dogfooding-session-52]]. iter-61j landed but three fixes did not stick on macOS:
`screenshot --full-page` still broken (4th session running), Firefox locale pin ineffective,
and `--with-network` only surfaces watcher data inline in `navigate` — subsequent `network`
calls still fall back to performance-api so response headers remain unreachable.

Plus 10 new bugs catalogued in session 52. Themes:

- **A — `screenshot --full-page` (4th attempt).** Must actually capture full scrollHeight at DPR.
- **B — Firefox locale pin (real this time).** Env vars `LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8` + `intl.locale.matchOS=false`.
- **C — `network` default reads daemon watcher buffer.** Stop falling back to performance-api when watcher data exists for the current navigation.
- **D — `--detail --headers` not silently dropped.** When source is performance-api, surface a `note:` and/or auto-promote to watcher path if buffered.
- **E — `computed` output-shape normalization.** Always array of `{computed:{...}, index, selector}`, regardless of `--prop` count.
- **F — `navigate` neterror detection.** Detect `about:neterror` after navigate and return a structured failure (`error_type: dns_not_found | connection_failed | etc.`) instead of success-shaped JSON.
- **G — `navigate` cross-origin race fix.** If the wait times out but `current_url == target`, treat as success.
- **H — `eval` CSP bypass.** Use `Cu.evalInSandbox` (or equivalent privileged path) via consoleActor so `eval` works on CSP-protected sites (HN, GitHub, banks). The single biggest LLM-friendliness lever.
- **I — `dom` shadow-DOM hints.** At minimum: flag `hasShadowRoot: true` on host elements. Stretch: `--include-shadow open|closed|none` to pierce.
- **J — `--fields` on `tabs`.** Honor the flag everywhere it's documented.
- **K — `consoleActor` cache refresh on navigate.** After any navigate (including to error pages), invalidate cached consoleActor so the next `eval` doesn't get `noSuchActor`.

Out of scope (own iteration):
- Cookie decoder hints (still deferred).
- `ff-rdp headers <url>` dedicated subcommand — once C lands, headers reachable via `network --detail --headers`.

## Tasks

### A. `screenshot --full-page` — fix it for real

#### A1. Verify the failure on a real long page [0/2]
- [ ] Repro: navigate to `https://en.wikipedia.org/wiki/HTTP`, `screenshot --full-page -o /tmp/x.png`. Confirm PNG height < `scrollHeight`.
- [ ] Inspect current implementation in `crates/ff-rdp-cli/src/commands/screenshot.rs` / chrome-scope path.

#### A2. Capture in chrome scope with explicit rect [0/3]
- [ ] Use chrome-scope capture with rect `(0, 0, scrollWidth, scrollHeight)`.
- [ ] Multiply by `window.devicePixelRatio` for physical pixels.
- [ ] On extremely long pages, split into stripes if Firefox imposes a max canvas height (Firefox: 32767 px); stitch.

#### A3. Live test [0/2]
- [ ] Test on Wikipedia/HTTP (~22k px): PNG height ≈ 22k × DPR.
- [ ] Test on a short page (HN front): PNG height ≈ scrollHeight × DPR (no regression on short pages).

### B. Firefox locale pin — env vars + matchOS

#### B1. Set env on launched Firefox process [0/2]
- [ ] In `crates/ff-rdp-cli/src/commands/launch.rs`, set child-process env: `LANG=en_US.UTF-8`, `LC_ALL=en_US.UTF-8`.
- [ ] Add `intl.locale.matchOS=false` to the generated user.js alongside existing `intl.locale.requested=en-US`.

#### B2. Live verification [0/1]
- [ ] After fresh `ff-rdp launch`, navigate to a quirks-mode page; assert the console warning is English, not German.

### C. `network --since -1` default reads daemon watcher buffer

#### C1. Default scoping fix [0/3]
- [ ] When the daemon is running and has buffered network events for the current navigation, `network` (no flags) should use them instead of performance-api.
- [ ] Keep performance-api as fallback only when no buffer entries exist.
- [ ] Document the precedence in `network --help`.

#### C2. Tests [0/2]
- [ ] Live: `navigate <url> --with-network`, then `network` (default) returns `source: watcher` with non-null `status`/`method`.
- [ ] No-daemon mode: `network --no-daemon` still uses performance-api.

### D. `--detail --headers` parity

#### D1. Surface fallback notes [0/2]
- [ ] When `--headers` is requested but the underlying source has none, emit a `note: "--headers ignored (performance-api source has no response headers; use --with-network to engage watcher)"` per entry or once at top level.
- [ ] Tests: snapshot the note.

### E. `computed` output-shape normalization

#### E1. Always array-of-records [0/2]
- [ ] `computed sel` (no --prop), `computed sel --prop X` (single), `computed sel --prop X --prop Y` (multi) all return `[{computed:{...}, index, selector}]`.
- [ ] Update `--help` "Output:" stanza to show one shape.

#### E2. Tests [0/2]
- [ ] Unit test all three call shapes.
- [ ] CLI snapshot for the three forms.

### F. `navigate` neterror detection

#### F1. Detect about:neterror [0/3]
- [ ] After commit, if `location.href` matches `^about:neterror`, parse the `e=` query param (`dnsNotFound`, `connectionFailure`, `netTimeout`, etc.).
- [ ] Return a structured error: exit non-zero, message includes `error_type`.
- [ ] Don't emit success-shaped JSON for the failed case.

#### F2. Test [0/1]
- [ ] Live: `navigate https://this-domain-definitely-does-not-exist-zzz.invalid` returns non-zero with `error_type: "dns_not_found"`.

### G. `navigate` cross-origin race-condition

#### G1. URL-match recovery on timeout [0/2]
- [ ] If commit-wait times out but `current_url == target_url` (or scheme+host+path match), treat as success.
- [ ] Live test on a fast cross-origin redirect chain.

### H. `eval` CSP bypass (the big one)

#### H1. Sandbox-eval path [0/3]
- [ ] Investigate `Cu.evalInSandbox` via consoleActor — privileged JS context, ignores page CSP.
- [ ] Alternative: inject a `<script src=blob:...>` element into the page DOM (still subject to script-src; less promising).
- [ ] Pick the path that works on HN (`script-src 'self' 'unsafe-inline'`, no `unsafe-eval`).

#### H2. Make it the default for `eval` on CSP-restricted pages [0/2]
- [ ] If page CSP forbids `eval()`, route through sandbox automatically; otherwise use the existing path.
- [ ] Surface a one-liner `note:` when sandbox path is used so callers know.

#### H3. Document + test [0/2]
- [ ] `eval --help` mentions CSP behavior.
- [ ] Live test: `navigate https://news.ycombinator.com`, then `eval 'document.title'` returns `"Hacker News"` without CSP error.

### I. `dom` shadow-DOM hints

#### I1. `hasShadowRoot` field on host elements [0/2]
- [ ] In `dom` results, set `hasShadowRoot: true` (and `shadowMode: "open"|"closed"`) on elements with a shadow root.
- [ ] Cheap & non-breaking.

#### I2. Stretch: `--include-shadow open` flag [0/2]
- [ ] Optional flag to descend into open shadow roots when querying.
- [ ] Document in `--help`. If complexity is high, defer to its own iteration.

### J. `--fields` on `tabs`

#### J1. Honor `--fields` [0/2]
- [ ] Pipe `--fields` through `tabs` like other commands.
- [ ] Snapshot test.

#### J2. Audit pass [0/1]
- [ ] Grep for every subcommand and verify `--fields` is wired where documented.

### K. `consoleActor` cache refresh on navigate

#### K1. Invalidate on navigate [0/2]
- [ ] After any navigate (including to about:neterror), clear cached consoleActor and re-fetch on next `eval`.
- [ ] Live test: navigate to a DNS-fail URL, then `eval 1+1` succeeds without the user retrying.

## Acceptance Criteria [0/14]

- [ ] **A.** `screenshot --full-page` on a 20k-px page produces a PNG with height ≈ `scrollHeight * DPR`.
- [ ] **B.** Fresh `ff-rdp launch` on macOS emits English console messages.
- [ ] **C.** `ff-rdp navigate <url> --with-network` followed by `ff-rdp network` (no flags) returns `source: watcher` with non-null `status` / `method`.
- [ ] **D.** `network --detail --headers` either returns headers (watcher path) or emits a clear `note` explaining why they're missing (perf-api path) — never silently dropped.
- [ ] **E.** `computed sel [--prop X]*` always returns the same array-of-records shape.
- [ ] **F.** `navigate <bad-dns-url>` returns a non-zero exit with `error_type` instead of success-shaped JSON.
- [ ] **G.** `navigate` to a fast cross-origin target no longer reports timeout when the URL actually committed.
- [ ] **H.** `eval 'document.title'` succeeds on `https://news.ycombinator.com` (CSP no longer blocks).
- [ ] **I.** `dom 'host-selector'` flags `hasShadowRoot:true` on shadow hosts.
- [ ] **J.** `tabs --fields url,title` returns only the requested fields.
- [ ] **K.** Navigating to about:neterror and then running `eval` succeeds without `noSuchActor`.
- [ ] All previous iter-61j ACs remain green (no regressions).
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.
- [ ] PR opened with iteration-61k/dogfood-52-fixes branch.

## Design Notes

- **A (`--full-page`)** has now failed in sessions 48/49/51/52. Treat as a blocker; if the existing chrome-scope plumbing doesn't yield a stable fix, consider an alternate path: scroll-and-stitch viewport screenshots, then assemble. Less elegant but reliably correct.
- **C (network watcher default)** depends on whether the daemon's per-tab buffer can be queried for "events since current navigation start". If not, add that filtering API first.
- **H (CSP-safe eval)** is the biggest LLM-friendliness gain in this iteration. If `Cu.evalInSandbox` works through the consoleActor protocol, this is a small change. If it requires a new actor or a privileged-only path, it may need its own iteration. Implementer's call.
- **B (locale)** — be careful about overwriting user-set env if the user passes their own env via `ff-rdp launch`. The env vars should be defaults that the user can override.
- **F (neterror)** — make sure the error includes the original target URL and the actual landed URL so callers can disambiguate redirects from failures.

## References

- Source: [[dogfooding-session-52]]
- Previous: [[iteration-61j-dogfood-51-fixes]] (A, B, C lifted/extended from there)
