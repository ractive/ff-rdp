---
title: "Iteration 93: eval via the DevTools console scope — CSP bypass on strict sites"
type: iteration
date: 2026-06-01
status: complete
branch: iter-93/eval-via-debugger-csp-bypass
depends_on:
  - iteration-92-full-page-and-navigate-parity
firefox_refs:
  - lines: 761-870
    path: devtools/server/actors/webconsole.js
    why: >-
      evaluateJSAsync consumer; routes eval through Debugger.evalInGlobal
      (eval-with-debugger.js) which bypasses page CSP — the sandbox scope
      is what makes CSP bypass work.
  - lines: 149-164
    path: devtools/shared/specs/webconsole.js
    why: >-
      evaluateJSAsync request fields spec (eager, frameActor, innerWindowID, …)
      — documents which fields land in the Debugger.evalInGlobal path.
kb_refs:
  - kb/dogfooding/dogfooding-session-59.md
  - kb/rdp/actors/console.md
first_call_sites:
  - primitive: eval routes through console scope, not page-injected script
    site: crates/ff-rdp-cli/src/commands/eval.rs
  - primitive: >-
      WebConsoleActor::evaluate_js_async_scoped sets the right field combo to land in
      the privileged console sandbox
    site: crates/ff-rdp-core/src/actors/console.rs
dogfood_script: iteration-93-eval-via-debugger-csp-bypass.dogfood.sh
tags:
  - iteration
  - eval
  - csp
  - console
  - dogfood-59
---

# Iteration 93 — `eval` survives strict-CSP sites

[[dogfooding-session-59]] §Issue 2: `ff-rdp eval 'document.title'` on
`developer.mozilla.org` fails with:

```
error: call to eval() blocked by CSP
class: "EvalError"
location: "@debugger eval code:1:36"
```

The Firefox DevTools console can evaluate on MDN, so the *protocol*
supports CSP-free evaluation. ff-rdp's current path goes through
`evaluateJSAsync` but the error's `@debugger eval code` location and
the `EvalError` class confirm the snippet is being executed via a
`new Function` / direct eval inside the page's principal — which is
exactly what the page's CSP forbids.

The fix is to make `evaluate_js_async_scoped` land in Firefox's
**console sandbox** (chrome principal) rather than the content
principal. The published webconsole spec
(`devtools/shared/specs/webconsole.js:149-164`) and consumer
(`devtools/server/actors/webconsole.js:761-870`) offer the
`frameActor` / `innerWindowID` / `selectedObjectActor` knobs that
DevTools itself uses; we need to discover the field combination that
hits the sandbox path.

## Impact

`eval` is the universal escape hatch — without it, agents cannot read
`window.scrollY`, framework state, custom globals, or anything not
covered by a typed command. A huge fraction of modern sites ship
strict CSP. This is the single largest dogfooding regression we have
that is not addressed by iter-92.

## Pre-fix repro

`pre_fix_repro_eval_works_on_strict_csp_site`: a live test that
serves a tiny HTML page with `Content-Security-Policy: script-src
'self'; require-trusted-types-for 'script'` from a local fixture
server, navigates, then runs `eval('document.title')`. Must **fail**
on `origin/main` with the `EvalError` / `blocked by CSP` shape and
**pass** on the branch.

## Hard rule

Single theme. Do not bundle the `eval --no-csp` ergonomics question
(opt-in flag vs default behavior) — implement the bypass and make it
the default; expose `--page-scope` as the escape hatch in the
**opposite** direction if anyone needs the old behavior. Defer that
flag to a follow-up if asked.

## Tasks

### Theme A — route eval through the console sandbox [6/6] [pre_fix_repro_test: pre_fix_repro_eval_works_on_strict_csp_site]

- [x] Stand up a local fixture HTTP server (axum or hyper, already
      a dep via tests) that serves a single HTML page with strict
      CSP headers matching MDN's posture (`script-src 'self';
      object-src 'none'; base-uri 'self'`). Bind to `127.0.0.1:0`;
      the test reads back the assigned port. This avoids depending
      on MDN's uptime.
- [x] Confirm the regression in the pre-fix repro test against
      `origin/main`. Capture the exact error class + location string
      so the fix's "should NOT produce this anymore" assertion is
      tight.
- [x] Investigate which `evaluateJSAsync` field combination lands
      in the chrome-privileged sandbox. Candidate paths to try, in
      order: (a) omit `frameActor` and `innerWindowID` entirely
      (top-level console scope), (b) set
      `selectedObjectActor` to the target's `WindowGlobalTarget`
      actor ID, (c) cross-reference with
      `devtools/client/webconsole/actions/input.js` to see what
      flags the toolbox passes. Document the winning combination
      in a comment with a `firefox_refs` line range.
- [x] Update `WebConsoleActor::evaluate_js_async_scoped` (or add a
      sibling `evaluate_js_async_in_console_scope` if the field set
      differs enough that overloading the scoped variant is
      confusing). Preserve the existing scoped path for callers that
      genuinely want page-principal eval (currently: nothing — but
      keep the surface tidy).
- [x] Switch `crates/ff-rdp-cli/src/commands/eval.rs` to the new
      console-scope path by default. The error shape on a script
      error inside the sandbox still surfaces to the user — verify
      via a fixture test that `eval('throw new Error("x")')` returns
      `class: "Error", message: "x"` (no CSP confusion).
- [x] dogfood_script Theme A: spin up the CSP fixture server, run
      `ff-rdp navigate <fixture>` then `ff-rdp eval
      'document.title'`; assert exit 0 AND that stdout JSON
      `result.value` equals the page title.

## Acceptance Criteria [6/6]

- [x] `pre_fix_repro_eval_works_on_strict_csp_site`: live test
      against the local CSP fixture; `eval('document.title')` exits
      0 on branch, fails with `EvalError` / `blocked by CSP` on main.
- [x] `unit_evaluate_js_async_console_scope_request_shape`: golden
      JSON of the request body the new path sends; pinned to the
      field combination we discovered (so a Firefox-side rename
      breaks the test loudly).
- [x] `live_eval_returns_window_scroll_y_on_csp_site`: after
      navigating to the CSP fixture and scrolling, `eval
      'window.scrollY'` returns a non-zero number.
- [x] `live_eval_script_error_still_surfaces`: `eval 'throw new
      Error("boom")'` returns `class: "Error"`, `message: "boom"`
      (no CSP error masking the real error).
- [x] `live_eval_works_on_real_mdn`: ignored-by-default live network
      test (`FF_RDP_LIVE_NETWORK_TESTS=1`); covers the original
      session-59 reproducer.
- [x] `dogfood_script_full_run_iter_93` (`pre_fix_repro_eval_works_on_strict_csp_site`, `live_eval_script_error_still_surfaces`): the sibling `.dogfood.sh`
      exits 0 and writes `/tmp/ff-rdp-iter-93-dogfood-ok`; live coverage provided by the named tests.

## Out of scope

- **`--page-scope` opt-out flag.** The console scope is strictly
  more capable; the only reason to revert is if a future site needs
  page-principal semantics (e.g. probing the page's own `window.eval`
  identity). File a follow-up if that need surfaces.
- **`eval --await` ergonomics.** Async resolution is a separate
  long-standing pain point (see [[project_rdp_async_constraints]]);
  not gated on CSP fix.
- **Auto-fallback to page-scope on console-scope failure.** Adds
  ambiguity (which scope produced the error?) for marginal benefit.
- **Multi-frame eval target selection.** Today eval implicitly hits
  the top-level frame; cross-frame is its own iteration.

## References

- [[dogfooding-session-59]]
- [[project_rdp_async_constraints]] (memory: async eval limits)
- Firefox `devtools/server/actors/webconsole.js:761-870`
- Firefox `devtools/shared/specs/webconsole.js:149-164`
