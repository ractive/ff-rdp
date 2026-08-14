---
branch: iter-160/envelope-honesty
date: 2026-08-13
depends_on:
  - kb/iterations/iteration-159-daemon-watcher-regression.md
dogfood_path: |
  ff-rdp launch --headless --debug-port 6000
  ff-rdp navigate https://example.com
  
  # --- A/B: click on an element covered by a full-screen overlay ---------------
  ff-rdp eval 'document.body.innerHTML = "<button id=t style=\"position:fixed;left:100px;top:100px;width:120px;height:40px\">Hit</button>"; window.__hits = 0; document.getElementById("t").addEventListener("click", function(){ window.__hits++; }); var v = document.createElement("div"); v.id = "veil"; v.setAttribute("style", "position:fixed;inset:0;z-index:9"); document.body.appendChild(v); document.elementFromPoint(160,120).id'
  # → "veil" — the overlay owns the centre point; no user could reach the button
  ff-rdp click '#t'
  # → today: {"clicked":true,"entered":true,...} exit 0
  # → after: exit 1, {"error":"...covered by div#veil...","error_type":"click_obscured",
  #          "obscured_by":"div#veil","matched":true,"reachable":false}
  ff-rdp eval 'window.__hits'
  # → 0 — read back independently; this is what the envelope must agree with
  ff-rdp eval 'document.getElementById("veil").remove(); "gone"'
  ff-rdp click '#t'
  # → {"clicked":true,"matched":true,"reachable":true,...} exit 0
  ff-rdp eval 'window.__hits'
  # → 1 — the click landed, proven outside the command's own self-report
  
  # --- C: type must emit key events -------------------------------------------
  ff-rdp eval 'document.body.innerHTML = "<input id=q>"; window.__keys = []; document.getElementById("q").addEventListener("keydown", function(e){ window.__keys.push("keydown:" + e.key); }); document.getElementById("q").addEventListener("keyup", function(e){ window.__keys.push("keyup:" + e.key); }); "armed"'
  ff-rdp type '#q' hi
  # → {"typed":true,"synthetic":true,...} — `synthetic` states the isTrusted ceiling
  ff-rdp eval 'JSON.stringify(window.__keys)'
  # → today: "[]" — no key events at all
  # → after: ["keydown:h","keyup:h","keydown:i","keyup:i"]
  
  # --- D: consent must not report a clean success when nothing was dismissed ---
  ff-rdp consent accept
  # → today: {"cmp":null,"action":null} exit 0 with the banner still on screen
  # → after: exit 1, error_type "consent_no_cmp"; `--allow-no-cmp` restores exit 0
  ff-rdp consent accept --allow-no-cmp --jq '.results.status'
  # → "no_cmp_detected" — one of accepted | detected_not_actioned | no_cmp_detected
  
  # --- E: a capped contrast sample must say so at the top level ----------------
  ff-rdp navigate https://news.ycombinator.com
  ff-rdp a11y contrast --fail-only --jq '{total: .total, sampled: .sampled, capped: .capped, source: .source}'
  # → capped/source live next to `sampled` at the top level, not buried in meta
  
  # --- F: --jq must not change the shape it filters ----------------------------
  ff-rdp navigate https://example.com --with-network
  ff-rdp network --jq '.results | type'
  # → today: "array" (--jq silently switched the envelope to detail mode)
  # → after: "object" — same as `ff-rdp network`; use --detail for the entry list
  ff-rdp network --detail --jq '.results | type'
  # → "array"
  
  # --- G: the real exception must survive the diagnostic ----------------------
  ff-rdp type body hello
  # → today: "selector 'body' not ready — matched 1 element (layout did not stabilise)"
  # → after: "selector 'body' not ready — element exists but is not an input,
  #          textarea, select, or contenteditable (matched 1 element)"
  ff-rdp type '#nosuch' hello
  # → unchanged: "selector '#nosuch' not ready — 0 elements matched (not found)"
first_call_sites: []
status: done
title: "Iteration 160: the JSON envelope asserts more than the command knows"
type: iteration
tags:
  - iteration
---

# Iteration 160: the JSON envelope asserts more than the command knows

Source: [[analysis-2026-08-13-what-ff-rdp-became]] §3.1, §3.6 and §4 ("Fix, do not
delete"). Every claim below was measured on the wire or read in the code during that
step-back; `file.rs:line` is given for each.

## The theme

The envelope is the best thing in this codebase — `{results, total, meta}`, held
consistently across 30 commands and 150 iterations. That is exactly why the places
where it *overstates* matter more here than they would anywhere else: a caller who
has learned to trust `--jq '.results.clicked'` has no way to discover that the field
is a self-report from JavaScript that never checked anything.

Five commands return a confident success, or a clean bill of health, that they have
not established:

| command | claims | actually knows |
|---|---|---|
| `click` | `clicked: true` | that `dispatchEvent` returned |
| `click` | `entered: true` | that `querySelector` was non-null |
| `type` | `typed: true` | that `.value` was assigned |
| `consent accept` | exit 0 | that no *recognised* CMP frame was found |
| `a11y contrast --fail-only` | 0 failures | that 0 of a capped sample failed |

Plus two shape problems that make the envelope less trustworthy as a contract:
`network`'s results shape depends on whether `--jq` was passed, and `type`'s
diagnostic path throws away a correct exception and guesses a wrong one.

**One rule for the whole iteration: the envelope says what is true, and no more.**
Where the command cannot know something, it reports what it does know and names the
gap. Nothing here is about making `click` more powerful — see
[[#What is out of scope]].

## The seven sites

### A — `click` reports success on elements no user could reach

`build_click_js` (`js_helpers.rs:613-654`) dispatches `new PointerEvent` /
`new MouseEvent` via `el.dispatchEvent`. The options bags at `js_helpers.rs:618-621`
carry `bubbles`, `cancelable`, `view`, `pointerType`, `isPrimary`, `button`,
`buttons` — and **no `clientX` / `clientY` at all**. Nothing in the sequence consults
the page's hit-test tree.

Measured during the step-back: a `<button>` under a full-screen overlay, where
`document.elementFromPoint(160, 120)` returned the overlay, still produced

```json
{"clicked": true, "entered": true, "tag": "BUTTON", "text": "Hit"}
```

at exit 0, and the button's own handler fired with `isTrusted: false, x: 0, y: 0`.
`ClickOnly` mode (`click.rs:435-447`) has the same shape via `el.click()`.

**Fix — hit test before dispatch.** In the click JS, after `querySelector` resolves
the element:

1. `var r = el.getBoundingClientRect();` — take the centre
   (`r.left + r.width / 2`, `r.top + r.height / 2`).
2. `var hit = document.elementFromPoint(cx, cy);`
3. Reachable iff `hit === el || el.contains(hit)` — a descendant (a `<span>` inside
   the button) is the normal case and must count as reachable.
4. If not reachable, build a short CSS description of `hit` (`tagName` lowercased,
   `#id` when present, else first two classes — the same construction
   `a11y_contrast.rs:233-238` already uses) and report it.

Edge cases the JS must handle rather than crash on:
- `elementFromPoint` returns `null` when the centre is outside the viewport — report
  `reachable: false` with `error_type: "click_offscreen"`, not "obscured".
- A zero-area rect is already caught by auto-wait (`js_helpers.rs:194-195`); the hit
  test runs after it, so it does not need to re-handle that.

**This technique is already written down in this repository.**
`kb/skills/ff-rdp-debug-playbooks.md:232-240`, playbook C3 ("invisible consent overlay
captures pointer events"), tells a *human* to run
`eval 'document.elementFromPoint(...).tagName'` by hand and concludes "`elementFromPoint`
returns the overlay, not the target." The knowledge existed and was never folded into
the command.

**Exit code.** An obscured click is a failed action, not an informational outcome — a
caller writing `ff-rdp click X && ff-rdp type Y …` must stop. Return
`AppError::Unsupported { error_type: "click_obscured", … }` (`error.rs:114-117`), which
exits 1 (`error.rs:191-195`) and carries a stable machine-readable discriminant. Reuse
that variant rather than adding a new one, so no new exported item is introduced.
Merge `obscured_by`, `matched` and `reachable` into the error envelope alongside
`error` / `error_type` (`error.rs:220-229`).

### B — `entered: true` names a claim it does not make

`entered` is assigned at `js_helpers.rs:646-649`: `var entered = false;` … `entered = true;`
immediately after the `querySelector` null check and **before** any `dispatchEvent`
call. In `ClickOnly` mode it is a hardcoded literal (`click.rs:443`). It means
"the selector matched." Its name says "the pointer could enter."

**Fix — split the claim in two.** The success envelope carries:

- `matched: true` — the selector resolved to an element (what `entered` actually meant)
- `reachable: true` — the hit test from Theme A put the target under its own centre point
- `clicked: true` — the event sequence dispatched

and on the obscured path, `obscured_by: "div#veil"` naming the covering element.

**Drop `entered` outright** rather than keeping it as an alias. It has exactly two
producers in the tree — `js_helpers.rs:651` and `click.rs:443` — and zero consumers:
no Rust code, no test, and no fixture reads the field. Record the envelope change in
[[decision-log]] as a deliberate break, and grep the kb one more time before deleting
so no playbook is left telling people to read it.

### C — `type` never emits a key event

`type_text.rs:79-101` assigns through the native `value` setter and then dispatches
exactly two events (`type_text.rs:96-97`):

```js
el.dispatchEvent(new Event('input', {bubbles: true}));
el.dispatchEvent(new Event('change', {bubbles: true}));
```

No `keydown`, no `keypress`, no `keyup`. Any UI that drives off keys — a combobox that
opens on `keydown`, a search box that debounces `keyup`, a form that validates on
`keypress` — sees nothing at all, while the command reports `{"typed": true}`.

**Fix — dispatch a per-character key sequence.** For each character of the text, before
the value assignment settles: `keydown` → (`keypress`, for printable characters) →
`keyup`, each a `new KeyboardEvent` with `key`, `code`, `bubbles: true`,
`cancelable: true`. Apply the value incrementally so a `keyup` handler that reads
`el.value` sees the prefix it would see from a real typist, then dispatch `input` and
`change` as today. `--clear` still applies `''` first.

**State the ceiling honestly, in the plan and in `--help`.** These events remain
`isTrusted: false`. A page that filters on `e.isTrusted`, or whose handler calls
`preventDefault()` on `keydown` to suppress the character, will behave differently from
a real keyboard — synthetic `preventDefault` cannot suppress an assignment ff-rdp makes
directly. This iteration makes key-driven UIs respond; it does not make `type`
indistinguishable from a user. Add `synthetic: true` to the result and one paragraph to
`type`'s long help (`args.rs`, the `Type` subcommand block) saying so.

### D — `consent accept` reports success while the banner is still up

`consent::run` (`consent.rs:29-48`) prints `output::envelope(&result, 1, &meta)` and
returns `Ok(())` regardless of outcome. `ConsentResult::none()` (`consent.rs:118-123`)
is `{"cmp": null, "action": null}`. Measured: exit 0, that envelope, banner still on
screen and still intercepting clicks — which is also the Theme A failure mode, so the
two compound.

Three distinct outcomes already exist in the code (`consent.rs:214-218`) and are
flattened into one exit code:

| outcome | today | after |
|---|---|---|
| accepted | `{cmp: "x", action: "accepted"}` exit 0 | `status: "accepted"`, exit 0 |
| CMP found, not actionable | `{cmp: "x", action: null}` exit 0 | `status: "detected_not_actioned"`, exit 1, `error_type: "consent_not_actioned"` |
| no known CMP | `{cmp: null, action: null}` exit 0 | `status: "no_cmp_detected"`, exit 1, `error_type: "consent_no_cmp"` |

**Fix.** Add `results.status` with those three values — keeping `cmp` and `action`, whose
always-present-key property (`consent.rs:109-113`) is deliberate and must survive — and
make the two non-accepting outcomes exit 1 via `AppError::Unsupported`. Add
`--allow-no-cmp` for callers that invoke `consent accept` speculatively and legitimately
want exit 0 when there was nothing to dismiss; model the flag's help text on
`--allow-file-urls`, which explains *why* the opt-in exists rather than just what it does.

**Do not change `navigate --auto-consent`'s exit code.** Its embedded `results.consent`
block gains the `status` field for consistency and nothing else — a page with no cookie
banner is not a failed navigation. (Citation fix: the block is built by
`merge_auto_consent` / `detect_and_accept_best_effort`, `navigate.rs:1809-1843` on
`main` as of iter-159 — not `args.rs:2130-2132`, which was already wrong when this plan
was written.)

**Scope addition from iter-159: there are now three producers of this shape, not one.**
iter-159 lifted the old `--with-network`/`--auto-consent` clap conflict and added a
*second* consent call path, `detect_and_accept_on` (`navigate.rs:1854-1861`), reused at
two call sites inside `run_with_network` — the daemon branch
(`obj.insert("consent"...)` at `navigate.rs:2145`) and the direct-mode branch
(`navigate.rs:2325`). Both currently emit the same two-key `{"cmp", "action"}` shape
`merge_auto_consent` does, with no `status` field — i.e. both already have the exact gap
Theme D exists to close, and neither existed when this plan's Theme D was drafted. The
`status` field must land on all three producers (`merge_auto_consent`,
`run_with_network`'s daemon branch, `run_with_network`'s direct branch), sourced from the
same `ConsentResult`/status computation so the three never drift apart — do not
special-case `--with-network`'s two call sites into their own status vocabulary. Exit
code stays 0 for all three (this is the same "not change the exit code" rule above,
now stated for three call sites instead of one).

### E — a capped sample from a fallback path reports a clean bill of health

Measured on a commercial site: `a11y contrast --fail-only` → 32 checked, **0 failures**,
with `"capped": true` and `"source": "js-fallback"` reachable only inside `meta`.

- `capped` is computed in the page JS as `elements.length > 1000`
  (`a11y_contrast.rs:258`) and lands inside `meta.summary` via `a11y_contrast.rs:50-53`.
- `source` is set unconditionally to `js-fallback` with reason `contrast-audit-js-only`
  (`a11y_contrast.rs:69`) and lands in `meta`.

So the reading a caller gets — "this page has no contrast failures" — is produced by a
truncated sample from a non-native path, and both qualifiers are two levels down in a
block that `--format text` does not print.

This is the same false-good shape iter-125 fixed for LCP, and the file already knows the
remedy: `sampled` was promoted to the **top level** of the envelope by iter-127
(`a11y_contrast.rs:81-87`) for exactly this reason.

**Fix.** Promote `capped` and `source` to the top level next to `sampled`, keeping the
`meta` copies for compatibility. When `capped: true` and the filtered failure count is 0,
emit a hint through the existing `HintContext` path (`a11y_contrast.rs:89-90`) that says
the sample was truncated and names the element count — a zero result from a truncated
sample must never be printable as a clean pass without the qualifier attached.

### F — `--jq` changes the shape it is supposed to filter

`network.rs:294-301` (line numbers as of iter-159, which touched this file; the
predicate itself is unchanged by iter-159 — only its position shifted when the
`auto`-fallback code above it was deleted):

```rust
let use_detail = cli.detail
    || cli.jq.is_some()
    || cli.sort.is_some()
    || cli.limit.is_some()
    || cli.all
    || cli.fields.is_some()
    || headers
    || security;
```

`use_detail` (consumed at `network.rs:318`) switches `results` from a summary **object**
to an entries **array**. So `ff-rdp network --jq '.results | type'` answers `"array"`
while `ff-rdp network` produced an object — the filter changed the document it was
filtering. Verified during the step-back as **network-only**: `console`, `a11y`, `perf`,
`sources` and `cookies` are single-shape. The global help says
*"Use --jq to filter the envelope"* (`args.rs:191`), which is false for this one command.

**Recommendation: make the shape independent of `--jq`** — remove `cli.jq.is_some()` from
the disjunction; keep `--detail`, `--all`, `--headers`, `--security` as explicit ways in.
Reasoning, since this is a compatibility break and the alternative is one line of doc:

- `--jq` is a **view** applied to the envelope. If the view can change the document, then
  every jq expression a caller writes is conditional on which command it is aimed at, and
  the one property that makes a uniform envelope worth having is gone.
- The documented contract (`args.rs:191`) already promises the honest behaviour. Changing
  the doc to match the code would ratify the exception and invite the next one.
- The migration is cheap and already anticipated: iter-126's comment at
  `network.rs:320-326` (iter-159 line numbers) records that detail mode was made to carry the full summary fields
  precisely because "`--jq` users … are forced into detail mode by the trigger list
  above." Once `--jq` no longer forces it, those users pass `--detail` and get an envelope
  that is a strict superset of what they had.

`--sort` / `--limit` / `--fields` are deliberately left in the disjunction: they are
list-shaped controls whose meaning on a summary object is undefined. Only `--jq`, which
is shape-agnostic by construction, comes out. Update the `network --help` text to name
`--detail` as the way to reach the entry list.

### G — the correct exception is thrown, then discarded

`ff-rdp type body hello` reports:

> selector 'body' not ready — matched 1 element (layout did not stabilise)

which is wrong twice: layout was fine, and the real reason is known. The chain:

1. `build_autowait_js` with `for_input: true` throws the accurate message at
   `js_helpers.rs:176`: `'element exists but is not an input, textarea, select, or
   contenteditable'`.
2. `autowait_element` sees `eval.exception.is_some()` at `js_helpers.rs:256`, and
   **drops the exception on the floor** — it calls
   `diagnose_selector_failure(ctx, console_actor, selector, &escaped)` at
   `js_helpers.rs:269` and formats only that string.
3. `diagnose_selector_failure` (`js_helpers.rs:343-402`) re-probes the page blind. Its JS
   (`js_helpers.rs:349-360`) reads only `matchCount` and a `hidden` flag. `body` matches
   1 element and is visible, so control falls to the hardcoded else-branch at
   `js_helpers.rs:385-387` — "layout did not stabilise" — which is simply the last thing
   left in the tree, not a finding.

**The sibling messages on this path are excellent and must not regress.** All of these are
produced by the same function and are among the best error text in the repo:

- `selector 'X' not ready — 0 elements matched (not found)` (`js_helpers.rs:378`)
- `selector 'X' not ready — the 1 matching element is hidden` (`js_helpers.rs:383`)
- `selector 'X' not ready — matched N elements, chose index 0 which is hidden; pass
  --visible or --index 0..N-1 to target a different match` (`js_helpers.rs:392-395`)

The bug is narrowly that the *fallback* loses a real exception. Fix it narrowly.

**Fix.** Thread the original exception message through. At `js_helpers.rs:256-274`, when
the readiness eval throws, prefer the thrown message and use the diagnostic only as
*context*:

```
selector 'body' not ready — element exists but is not an input, textarea, select,
or contenteditable (matched 1 element) (after 12ms, timeout 5000ms)
```

The timeout branch at `js_helpers.rs:242-250`, which has no exception to thread, keeps
calling `diagnose_selector_failure` unchanged. Both `display:none` and
`visibility:hidden` also arrive through the exception branch (`js_helpers.rs:189-190`),
and for those the diagnostic's hidden-aware text is the better message — so the rule is:
use the thrown message when the diagnostic falls through to its match-count-only
branches, and keep the diagnostic when it has a hidden-ness or multi-match finding of its
own. Pin both directions with tests.

## Test evidence rule for this iteration

`live_140_ref_click_resolves`
(`crates/ff-rdp-cli/tests/live/live_140_element_targeting.rs:146-195`) asserts exactly two
things: `click["results"]["clicked"] == true` and `click["results"]["text"] == "Two"`.
Both come out of the click JS's own `JSON.stringify` at `js_helpers.rs:651`. The overlay
case in Theme A is the proof that this is not evidence: that envelope is emitted whether
or not anything on the page moved.

**Every behavioural test in this iteration asserts the effect through a separate `eval`
round-trip, not the command's self-report.** Instrument a counter or an event log on the
page, run the command, then read the counter back with `ff-rdp eval`. The pattern already
exists in the same file — `live_140_visible_flag_targets_visible`
(`live_140_element_targeting.rs:382-439`) types, then evals
`Array.from(document.querySelectorAll(...)).map(e => e.value)` and asserts the hidden
input is untouched. Copy that shape.

`live_140_ref_click_resolves` itself gets the same treatment as part of this iteration
(the last AC) — it is a one-line fixture change plus one extra `eval`.

## What is out of scope

**Trusted input.** Do not attempt it, and do not reopen the question in a later
iteration without new protocol evidence.

`kb/rdp/client/remote-agent-cdp.md` establishes that the only trusted-input surfaces in
Firefox are Marionette and WebDriver BiDi, which are **peer protocols to devtools-RDP,
not layers reachable through it**. There is no `Input.dispatchMouseEvent` equivalent on
the RDP surface to wire up. [[decision-log]] DEC-008 chose `evaluateJSAsync` as the
implementation for `click` / `type` and listed its trade-offs (HttpOnly cookies, shadow
DOM internals, computed styles); untrusted input was never among them, which is how this
shipped unremarked.

Consequences that therefore stay true after this iteration, and that the `--help` text
must state rather than hide:

- events remain `isTrusted: false`
- `e.clientX` / `e.clientY` remain `0` — the hit test decides *whether* to dispatch, it
  does not give the events real coordinates
- a handler gated on `isTrusted`, or one that `preventDefault()`s a synthetic `keydown`,
  will not behave as it would for a user

Also out of scope: the `network` watcher regression (that is
[[iteration-159-daemon-watcher-regression]], which this depends on), `launch`'s 5s port
wait, `--fields` / `--sort` silent no-ops, and `eval --stringify`. This iteration touches
only what the envelope *says*.

## Acceptance Criteria [16/18]

- [x] live_160_click_obscured_reports_unreachable: with a button at (100,100,120x40)
      covered by a `position:fixed;inset:0` overlay, `click '#t'` exits 1 with
      `error_type == "click_obscured"` and `obscured_by == "div#veil"`, and a separate
      `eval 'window.__hits'` returns 0 [verified: 2026-08-14, exit 1, error_type=click_obscured, obscured_by=div#veil, matched=true, reachable=false, window.__hits=0]
- [x] live_160_click_reachable_fires_handler: after removing the overlay, `click '#t'`
      exits 0 with `matched == true`, `reachable == true`, `clicked == true`, and a
      separate `eval 'window.__hits'` returns 1 [verified: 2026-08-14, exit 0, matched=true reachable=true clicked=true, `entered` absent, window.__hits=1]
- [x] live_160_click_descendant_hit_counts_as_reachable: clicking a `<button><span>Go</span></button>`
      whose centre point resolves to the inner `<span>` exits 0 with `reachable == true`
      and the handler counter read back by `eval` equals 1 [verified: 2026-08-14, elementFromPoint(centre).id=inner, exit 0 reachable=true, window.__hits=1]
- [x] unit_160_click_js_hit_tests_centre_point: `build_click_js` output contains
      `getBoundingClientRect`, `elementFromPoint`, and a `el.contains(` descendant check,
      and the hit test appears textually before the first `dispatchEvent` call
- [ ] unit_160_click_result_reports_matched_and_reachable: the click result JSON key set
      contains `matched` and `reachable`, and `entered` is absent from both
      `build_click_js` and `build_click_js_for_mode` output in all three dispatch modes
- [x] live_160_type_emits_key_events: after arming `keydown`/`keyup` listeners that push
      into `window.__keys`, `type '#q' hi` leaves a separate
      `eval 'JSON.stringify(window.__keys)'` equal to
      `["keydown:h","keyup:h","keydown:i","keyup:i"]`, and `#q`'s value read back by the
      same eval is `"hi"` [verified: 2026-08-14, 4 key events observed in window.__keys - keydown:h, keyup:h, keydown:i, keyup:i - in that order; #q.value="hi"; results.synthetic=true]
- [x] unit_160_type_help_states_synthetic_ceiling: the `type` long-help string contains
      `isTrusted: false` and names `preventDefault`, and the result JSON built by the type
      JS contains `synthetic: true`
- [x] live_160_consent_no_cmp_exits_nonzero: `consent accept` on a fixture page with an
      unrecognised banner exits 1 with `error_type == "consent_no_cmp"`, and a separate
      `eval 'document.getElementById("banner") !== null'` returns true [verified: 2026-08-14, exit 1, error_type=consent_no_cmp, banner still in the DOM (eval returned true)]
- [x] live_160_consent_allow_no_cmp_exits_zero: the same page with
      `consent accept --allow-no-cmp` exits 0 and `--jq '.results.status'` prints
      `no_cmp_detected` [verified: 2026-08-14, exit 0, --jq '.results.status' printed no_cmp_detected]
- [x] unit_160_consent_status_values_are_distinct: `ConsentResult::to_json` emits
      `status` values `accepted`, `detected_not_actioned` and `no_cmp_detected` for the
      three constructions, all three still carry non-omitted `cmp` and `action` keys, and
      only `accepted` maps to exit code 0
- [x] live_160_with_network_auto_consent_reports_status: `navigate <consent-walled-url>
      --with-network --auto-consent` (the combination iter-159 unblocked) returns
      `results.consent.status` with the same three-value vocabulary as `consent accept`,
      in both daemon and `--no-daemon` mode, and exits 0 in both — Theme D's `status`
      field must reach `merge_auto_consent` and both `run_with_network` call sites
      (`navigate.rs:2145`, `navigate.rs:2325`), not just plain `navigate --auto-consent` [verified: 2026-08-14, all three producers returned results.consent.status=no_cmp_detected on the unrecognised-banner fixture, exit 0 in daemon and --no-daemon]
- [x] live_160_contrast_cap_and_source_at_top_level: on a fixture page with more than
      1000 text-bearing elements, `a11y contrast --fail-only` puts `capped == true` and
      `source == "js-fallback"` at the envelope's top level (reachable as `--jq '.capped'`
      and `--jq '.source'`), and emits a hint naming the sampled element count [verified: 2026-08-14, 1200-<p> fixture -> top-level capped=true source=js-fallback sampled=996 total=0, meta.summary.capped retained, --hints emitted "Sample was truncated ... 996 element(s) examined"]
- [ ] unit_160_contrast_envelope_promotes_capped_and_source: the envelope assembled by
      the contrast path carries `capped` and `source` as top-level keys alongside
      `sampled`, and the `meta.summary.capped` copy is retained
- [x] live_160_network_results_shape_ignores_jq: after `navigate --with-network`,
      `network --jq '.results | type'` prints `object` (identical to `network`'s shape)
      and `network --detail --jq '.results | type'` prints `array` [verified: 2026-08-14, 2 invocations: --jq '.results | type' printed "object"; --detail --jq '.results | type' printed "array"]
- [x] unit_160_use_detail_excludes_jq: the `use_detail` predicate returns false when only
      `cli.jq` is set, and true for each of `--detail`, `--all`, `--headers`,
      `--security`, `--sort`, `--limit`, `--fields`
- [x] live_160_type_non_input_reports_thrown_reason: `type body hello` fails with a
      message containing `is not an input, textarea, select, or contenteditable` and
      containing neither `layout did not stabilise` nor `rect did not stabilise` [verified: 2026-08-14, `type body hello` reported "selector 'body' not ready - element exists but is not an input, textarea, select, or contenteditable (matched 1 element)"; neither stabilise string present]
- [x] live_160_selector_diagnostics_survive: on the same fixture, `type '#nosuch' hello`
      still reports `0 elements matched (not found)` and `type '#hidden_input' hello`
      still reports `the 1 matching element is hidden` — both messages byte-identical to
      the strings at `js_helpers.rs:378` and `js_helpers.rs:383` [verified: 2026-08-14, `#nosuch` -> "0 elements matched (not found)", `#hidden_input` -> "the 1 matching element is hidden"]
- [x] live_160_ref_click_asserts_handler_effect: `live_140_ref_click_resolves` is extended
      to arm a click counter on the "Two" button and, after `click --ref`, a separate
      `eval` reports that counter equal to 1 [verified: 2026-08-14, live_160_ref_click_asserts_handler_effect: click --ref resolved text="Two" and window.__two=1]

## Notes

- Nothing in this iteration introduces a new exported item — `AppError::Unsupported`
  (`error.rs:114-117`) is reused for both new `error_type` discriminants, which is why
  `first_call_sites` is empty.
- Two envelope-shape breaks land here: `click` loses `entered`, and `network` stops
  switching to detail mode under `--jq`. Record both in [[decision-log]] with the
  reasoning from Themes B and F, and check the fixtures under
  `crates/ff-rdp-cli/tests/fixtures/` before landing — fixtures are recorded from real
  Firefox and must be re-recorded, never hand-edited, per the fixture rules.
  **Checked (2026-08-14): no fixture needed re-recording.** `grep -rn entered
  crates/*/tests/fixtures/` returns nothing. The one click fixture,
  `eval_result_click.json`, holds
  `{"clicked":true,"tag":"BUTTON","text":"Submit"}` — recorded by
  `live_record_fixtures.rs:783-796`, which evaluates its *own* hand-written
  `el.click()` snippet rather than `build_click_js`, so it never carried `entered`
  and does not carry `matched`/`reachable` either. Its four e2e consumers
  (`tests/e2e/click.rs`) exercise the CLI's result-resolution path, not the click
  JS's field set, and pass unchanged. The one fast test that *did* encode
  pre-160 behaviour was `network_with_jq_filter`, rewritten as
  `network_with_detail_jq_filter` plus a new `network_jq_does_not_switch_results_to_an_array`.
- Themes A and D compound in the field: an undismissed consent overlay is the single most
  common cause of a click that reports success and does nothing. After this iteration,
  that scenario produces `error_type: "click_obscured"` with the overlay named — which is
  the machine-readable form of what playbook C3
  (`kb/skills/ff-rdp-debug-playbooks.md:232-240`) currently asks a human to diagnose by
  hand. Update C3 to say the command now reports it directly.
- Same honesty family as [[iteration-149-a11y-restore-honesty]] and
  [[iteration-153-launch-replace-double-envelope]]: the command did something, or failed
  to, and did not tell the caller in a usable way.
- Verify on the wire before fixing. Across iterations 135–151 the stated root cause
  diverged from reality at least eight times; the observations above were captured from
  actual runs, so reproduce each one before changing the code it points at.

- **Two ACs are left unticked despite the work being done, and this is deliberate.**
  `unit_160_click_result_reports_matched_and_reachable` and
  `unit_160_contrast_envelope_promotes_capped_and_source` are both implemented, and
  both named tests exist and pass (`cargo test -p ff-rdp-cli --bin ff-rdp`:
  940 passed / 0 failed). `ac-fidelity-check.sh` still reports "no evidence in diff"
  for them, for a reason that has nothing to do with this iteration: its heuristic 1
  recognises only `live_`/`test_`/`bench_` slug prefixes (line 359), so a
  `unit_160_*` name is not looked up as a test function at all, and its heuristics 2
  and 3 read only the AC's **first line** (`text`, line 189) rather than the folded
  text the non-execution and deferral checks were moved onto in iter-154 — and these
  two ACs happen to carry no backticked symbol before their first line wraps. Fixing
  either would mean editing `~/.claude/skills/ralph-loop/scripts/`, which CLAUDE.md
  says must not be done from inside a ralph-loop run, and the alternatives — rewrapping
  the AC so a symbol lands on line one, or renaming the test functions to `test_160_*`
  so heuristic 1 fires — are both "reword until the grep stops firing", which the
  iteration's own run constraints forbid. Left unticked and reported rather than
  routed around. Worth folding into the [[project_discipline_gate_gaps]] follow-up:
  the two heuristics should read `full_text`, and the slug regex should learn `unit_`.
