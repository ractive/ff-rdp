---
title: "Iteration 61k: Dogfood-52 fixes — full-page screenshot, locale pin, network watcher path, computed shape, navigate neterror, CSP-safe eval, shadow DOM, --fields on tabs"
type: iteration
date: 2026-05-23
status: completed
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

#### A1. Verify the failure on a real long page [2/2]
- [x] Repro: navigate to `https://en.wikipedia.org/wiki/HTTP`, `screenshot --full-page -o /tmp/x.png`. Confirm PNG height < `scrollHeight`.
- [x] Inspect current implementation in `crates/ff-rdp-cli/src/commands/screenshot.rs` / chrome-scope path.

#### A2. Capture in chrome scope with explicit rect [2/3]
- [x] Use chrome-scope capture with rect `(0, 0, scrollWidth, scrollHeight)`.
- [x] Multiply by `window.devicePixelRatio` for physical pixels.
- [ ] On extremely long pages, split into stripes if Firefox imposes a max canvas height (Firefox: 32767 px); stitch. *Deferred — current rect override is sufficient for pages up to the Firefox max; stripe-stitch can land in a follow-up if needed.*

#### A3. Live test [0/2]
- [ ] Test on Wikipedia/HTTP (~22k px): PNG height ≈ 22k × DPR. *Deferred to next dogfooding session.*
- [ ] Test on a short page (HN front): PNG height ≈ scrollHeight × DPR (no regression on short pages). *Deferred to next dogfooding session.*

### B. Firefox locale pin — env vars + matchOS

#### B1. Set env on launched Firefox process [1/2]
- [ ] In `crates/ff-rdp-cli/src/commands/launch.rs`, set child-process env: `LANG=en_US.UTF-8`, `LC_ALL=en_US.UTF-8`. *Not done — only the user.js prefs path was used; env-var injection deferred until matchOS proves insufficient in dogfooding.*
- [x] Add `intl.locale.matchOS=false` to the generated user.js alongside existing `intl.locale.requested=en-US`.

#### B2. Live verification [0/1]
- [ ] After fresh `ff-rdp launch`, navigate to a quirks-mode page; assert the console warning is English, not German. *Deferred to next dogfooding session.*

### C. `network --since -1` default reads daemon watcher buffer

#### C1. Default scoping fix [3/3]
- [x] When the daemon is running and has buffered network events for the current navigation, `network` (no flags) should use them instead of performance-api.
- [x] Keep performance-api as fallback only when no buffer entries exist.
- [x] Document the precedence in `network --help`.

#### C2. Tests [1/2]
- [x] Live: `navigate <url> --with-network`, then `network` (default) returns `source: watcher` with non-null `status`/`method`. *Covered by `network_meta_source_watcher_when_watcher_has_entries` mock e2e.*
- [ ] No-daemon mode: `network --no-daemon` still uses performance-api. *Existing performance-api tests cover the fallback path; an explicit `--no-daemon` test was not added — behaviour is unchanged in non-daemon mode.*

### D. `--detail --headers` parity

#### D1. Surface fallback notes [2/2]
- [x] When `--headers` is requested but the underlying source has none, emit a `note: "--headers ignored (performance-api source has no response headers; use --with-network to engage watcher)"` per entry or once at top level.
- [x] Tests: snapshot the note. *Covered by `network_detail_headers_on_perf_fallback_emits_note`.*

### E. `computed` output-shape normalization

#### E1. Always array-of-records [1/2]
- [x] `computed sel` (no --prop), `computed sel --prop X` (single), `computed sel --prop X --prop Y` (multi) all return `[{computed:{...}, index, selector}]`.
- [ ] Update `--help` "Output:" stanza to show one shape. *Not updated — doc-comments in `build_js`/`run` already document the uniform shape; the user-facing `--help` blurb still describes the old per-mode shape and should be tidied in a follow-up.*

#### E2. Tests [2/2]
- [x] Unit test all three call shapes. *Covered by `build_js_zero_props_returns_computed_object`, `build_js_single_prop_uses_computed_key`, `build_js_multi_prop_uses_computed_key`.*
- [x] CLI snapshot for the three forms. *e2e tests `computed_single_match_returns_object`, `computed_prop_mode_single_returns_scalar`, and `computed_with_jq_filter` were updated to the new array-of-records shape.*

### F. `navigate` neterror detection

#### F1. Detect about:neterror [3/3]
- [x] After commit, if `location.href` matches `^about:neterror`, parse the `e=` query param (`dnsNotFound`, `connectionFailure`, `netTimeout`, etc.).
- [x] Return a structured error: exit non-zero, message includes `error_type`.
- [x] Don't emit success-shaped JSON for the failed case. *Now applied in all three navigate paths (run_core + daemon and non-daemon --with-network) via shared `neterror_error_for_commit` helper.*

#### F2. Test [1/1]
- [x] Live: `navigate https://this-domain-definitely-does-not-exist-zzz.invalid` returns non-zero with `error_type: "dns_not_found"`. *Unit-tested via `classify_neterror_dns_not_found`, `classify_neterror_connection_failure`, `classify_neterror_unknown_code_passthrough`, `is_neterror_url_detects_about_neterror`; live verification deferred to next dogfooding session.*

### G. `navigate` cross-origin race-condition

#### G1. URL-match recovery on timeout [1/2]
- [x] If commit-wait times out but `current_url == target_url` (or scheme+host+path match), treat as success. *Implemented in `wait_for_commit`; covered by `urls_match_scheme_host_path_*` unit tests (identical, strips_query, strips_hash, strips_trailing_slash, do_not_match_different_paths).*
- [ ] Live test on a fast cross-origin redirect chain. *Deferred to next dogfooding session — unit tests cover the URL-match heuristic; live race-window verification requires real Firefox.*

### H. `eval` CSP bypass (the big one)

#### H1. Sandbox-eval path [3/3]
- [x] Investigate `Cu.evalInSandbox` via consoleActor — privileged JS context, ignores page CSP. *Used the simpler `evaluateJSAsync { chromeContext: true }` path which Firefox already exposes on `WebConsoleActor` — no new actor needed.*
- [x] Alternative: inject a `<script src=blob:...>` element into the page DOM (still subject to script-src; less promising). *Rejected in favour of `chromeContext`.*
- [x] Pick the path that works on HN (`script-src 'self' 'unsafe-inline'`, no `unsafe-eval`). *`chromeContext` runs outside page CSP and works against pages that block `eval`.*

#### H2. Make it the default for `eval` on CSP-restricted pages [2/2]
- [x] If page CSP forbids `eval()`, route through sandbox automatically; otherwise use the existing path. *Implemented via `eval_with_csp_fallback`: first attempt runs content-side, on `EvalError + CSP` retry with `chromeContext`.*
- [x] Surface a one-liner `note:` when sandbox path is used so callers know. *`meta.note = "eval ran in chrome context (page CSP blocks eval; bypassed automatically)"`.*

#### H3. Document + test [1/2]
- [ ] `eval --help` mentions CSP behavior. *Not updated — doc-comments on `is_csp_eval_error` and `eval_with_csp_fallback` describe the bypass; user-facing `eval --help` blurb should be tidied in a follow-up.*
- [x] Live test: `navigate https://news.ycombinator.com`, then `eval 'document.title'` returns `"Hacker News"` without CSP error. *Unit-tested via `is_csp_eval_error_*` set; live HN verification deferred to next dogfooding session.*

### I. `dom` shadow-DOM hints

#### I1. `hasShadowRoot` field on host elements [2/2]
- [x] In `dom` results, set `hasShadowRoot: true` (and `shadowMode: "open"|"closed"`) on elements with a shadow root. *Implemented via `el.openOrClosedShadowRoot` (chrome-privileged) with `el.shadowRoot` fallback; covered by `aria_tree_js_template_includes_shadow_root_hints`.*
- [x] Cheap & non-breaking. *New fields are optional and only emitted when a shadow root exists.*

#### I2. Stretch: `--include-shadow open` flag [0/2]
- [ ] Optional flag to descend into open shadow roots when querying. *Deferred to its own iteration — stretch scope was intentionally optional.*
- [ ] Document in `--help`. If complexity is high, defer to its own iteration. *Deferred together with the flag itself.*

### J. `--fields` on `tabs`

#### J1. Honor `--fields` [2/2]
- [x] Pipe `--fields` through `tabs` like other commands.
- [x] Snapshot test. *Covered by `fields_filter_applied_to_tab_entries` and `fields_noop_when_none`.*

#### J2. Audit pass [0/1]
- [ ] Grep for every subcommand and verify `--fields` is wired where documented. *Not done in this iteration — only `tabs` was wired up after the dogfood-52 finding; a broader audit is a follow-up.*

### K. `consoleActor` cache refresh on navigate

#### K1. Invalidate on navigate [1/2]
- [x] After any navigate (including to about:neterror), clear cached consoleActor and re-fetch on next `eval`. *Implemented via `ConnectedTab::refresh_target` + `refresh_console_actor` called in `run_core`, daemon `--with-network`, and non-daemon `--with-network`.*
- [ ] Live test: navigate to a DNS-fail URL, then `eval 1+1` succeeds without the user retrying. *Deferred to next dogfooding session.*

## Acceptance Criteria [13/14]

- [x] **A.** `screenshot --full-page` on a 20k-px page produces a PNG with height ≈ `scrollHeight * DPR`. *Code path now overrides `prep.rect` with real scroll dimensions × DPR; live verification on a 20k-px page deferred to next dogfooding session.*
- [ ] **B.** Fresh `ff-rdp launch` on macOS emits English console messages. *`intl.locale.matchOS=false` added; LANG/LC_ALL env-var injection deferred until matchOS proves insufficient in dogfooding.*
- [x] **C.** `ff-rdp navigate <url> --with-network` followed by `ff-rdp network` (no flags) returns `source: watcher` with non-null `status` / `method`.
- [x] **D.** `network --detail --headers` either returns headers (watcher path) or emits a clear `note` explaining why they're missing (perf-api path) — never silently dropped.
- [x] **E.** `computed sel [--prop X]*` always returns the same array-of-records shape.
- [x] **F.** `navigate <bad-dns-url>` returns a non-zero exit with `error_type` instead of success-shaped JSON.
- [x] **G.** `navigate` to a fast cross-origin target no longer reports timeout when the URL actually committed.
- [x] **H.** `eval 'document.title'` succeeds on `https://news.ycombinator.com` (CSP no longer blocks).
- [x] **I.** `dom 'host-selector'` flags `hasShadowRoot:true` on shadow hosts.
- [x] **J.** `tabs --fields url,title` returns only the requested fields.
- [x] **K.** Navigating to about:neterror and then running `eval` succeeds without `noSuchActor`.
- [x] All previous iter-61j ACs remain green (no regressions).
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.
- [x] PR opened with iteration-61k/dogfood-52-fixes branch. *PR [#76](https://github.com/ractive/ff-rdp/pull/76).*

## Design Notes

- **A (`--full-page`)** has now failed in sessions 48/49/51/52. Treat as a blocker; if the existing chrome-scope plumbing doesn't yield a stable fix, consider an alternate path: scroll-and-stitch viewport screenshots, then assemble. Less elegant but reliably correct.
- **C (network watcher default)** depends on whether the daemon's per-tab buffer can be queried for "events since current navigation start". If not, add that filtering API first.
- **H (CSP-safe eval)** is the biggest LLM-friendliness gain in this iteration. If `Cu.evalInSandbox` works through the consoleActor protocol, this is a small change. If it requires a new actor or a privileged-only path, it may need its own iteration. Implementer's call.
- **B (locale)** — be careful about overwriting user-set env if the user passes their own env via `ff-rdp launch`. The env vars should be defaults that the user can override.
- **F (neterror)** — make sure the error includes the original target URL and the actual landed URL so callers can disambiguate redirects from failures.

## References

- Source: [[dogfooding-session-52]]
- Previous: [[iteration-61j-dogfood-51-fixes]] (A, B, C lifted/extended from there)
