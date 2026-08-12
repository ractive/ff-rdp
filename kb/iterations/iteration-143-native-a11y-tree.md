---
branch: iter-143/native-a11y-tree
date: 2026-08-09
depends_on:
  - kb/iterations/iteration-136-core-live-test-repairs.md
dogfood_path: |
  ff-rdp launch --headless --port 6000
  ff-rdp navigate https://example.com --port 6000
  ff-rdp a11y --port 6000 --jq '.meta.source'
  # → must say which tree was served: "native" or "js-fallback", never silence
  ff-rdp a11y --port 6000 --native
  # → must return the platform tree (roles like "document"/"paragraph"),
  #   not the DOM-derived one (roles like "generic")
first_call_sites: []
status: planned
---

# Iteration 143: decide and expose how `ff-rdp a11y` gets its tree

Carry-over from [[iteration-136-core-live-test-repairs]], filed before that PR merged.

## Background

iter-136 established what current Firefox actually supports (see
[[rdp/actors/accessibility]]):

- the walker's argument-less `children` is the root accessor;
- each accessible actor answers its own `children`;
- **none of it replies at all until the platform accessibility service is enabled** —
  the walker waits on a `document-ready` promise that never settles, so a client blocks
  until its socket read timeout rather than getting an error.

Enabling the service is `enable()` on the root form's `parentAccessibilityActor`. It is a
**browser-global, process-wide** change with a real performance cost that persists until
the browser shuts down, and on Windows an active screen reader can block the matching
`disable()`. iter-136 therefore did **not** enable it on the user's behalf: `ff-rdp a11y`
checks `AccessibilityActor::is_service_enabled` and falls straight through to the
JS-derived tree when it is off.

That is safe but two things are still wrong for users.

## Themes

### Theme A — the output does not say which tree it is

`ff-rdp a11y` returns the same JSON shape from two very different sources. The native
platform tree reports real accessible roles (`document`, `paragraph`, `link`); the JS
fallback reports DOM-derived approximations (`generic`, …) and cannot see anything the
platform computes but the DOM does not expose. Today a caller cannot tell them apart
except by `--verbose` stderr. Anything downstream that scores accessibility off this
output is comparing apples to oranges without knowing it.

Fix: report the source in `meta` (e.g. `"source": "native" | "js-fallback"` plus the
reason when it fell back). Same treatment for `a11y audit`/`--interactive` if they share
the path.

Implementation precedent: [[iteration-134-meta-route-all-commands]] just rolled out the
same "always present regardless of `--verbose`" shape for `meta.route` via a small
`connection_meta::merge_route(&mut meta, via_daemon)` helper called at every relevant
command's meta-building call site (not gated behind the existing
`merge_into_if_verbose`). A `connection_meta::merge_source` (or local equivalent) called
the same way — right before `output::envelope(...)` in `a11y::run`/`run_critical` and
`a11y audit` — is the straightforward way to satisfy this theme without inventing a new
pattern. Note `a11y.rs` and `a11y_contrast.rs` already gained a `merge_route` call site in
iter-134; `meta.source` is an independent field added at the same call site, not a
replacement for it.

### Theme B — no way to ask for the real thing

There is no opt-in. A user who wants the platform tree has to enable Firefox
accessibility out-of-band.

Fix: an explicit flag (`--native` / `--enable-a11y-service`, name to be decided) that
enables the service via `parentAccessibilityActor.enable()`, walks the native tree, and
restores the previous state afterwards when it was ff-rdp that turned it on. Must degrade
honestly: if `enable` fails or `bootstrap` still reports disabled, say so rather than
silently falling back.

**Resolved before this iteration started**: [[decision-log]] DEC-027 (filed ahead of
iter-143 landing) already answers the default-vs-opt-in question — `--native` stays
opt-in, never the default, because `enable()` is browser-global/process-wide and its
`disable()` can be blocked by an active Windows screen reader. No further decision-log
work is needed for this theme; implement the flag opt-in from the start rather than
re-opening the question.

### Theme C — bound the stall

Even with the guard from iter-136, any future caller that reaches a walker request while
the service is off will block for the full socket read timeout. Consider a shorter,
purpose-specific deadline on accessibility walker requests so a mistake costs
milliseconds, not the default timeout.

## Acceptance Criteria [1/5]

- [ ] live_a11y_source_meta: `ff-rdp a11y` output carries a `meta.source` of
      `js-fallback` against a Firefox with the accessibility service off
- [ ] live_a11y_native_opt_in: with the opt-in flag, the root role is `document` and the
      tree contains platform roles the JS fallback does not produce
- [ ] live_a11y_service_restored: after an opt-in run that enabled the service,
      `bootstrap().state.enabled` is back to its pre-run value
- [ ] unit/e2e: enable failure surfaces as an explicit error or an annotated fallback,
      never a silent one
- [x] [[decision-log]] records the default-vs-opt-in decision — done ahead of this
      iteration's implementation work: DEC-027 (filed on main before this branch existed)
      settles opt-in-never-default. No code landed yet for this iteration; only the
      decision-log prerequisite is satisfied.

## Notes

- Do not re-litigate the protocol: iter-136 already verified it against the Firefox
  source and recorded FF153 fixtures (`a11y_walker_children_response.json`,
  `a11y_children_response.json`, `a11y_children_empty_response.json`,
  `a11y_bootstrap_response.json`).
