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

### Theme B — no way to ask for the real thing

There is no opt-in. A user who wants the platform tree has to enable Firefox
accessibility out-of-band.

Fix: an explicit flag (`--native` / `--enable-a11y-service`, name to be decided) that
enables the service via `parentAccessibilityActor.enable()`, walks the native tree, and
restores the previous state afterwards when it was ff-rdp that turned it on. Must degrade
honestly: if `enable` fails or `bootstrap` still reports disabled, say so rather than
silently falling back.

Open question to settle first: should `--native` also become the default once it is
proven, or stay opt-in because of the global performance cost? Decide in
[[decision-log]] before writing the flag.

### Theme C — bound the stall

Even with the guard from iter-136, any future caller that reaches a walker request while
the service is off will block for the full socket read timeout. Consider a shorter,
purpose-specific deadline on accessibility walker requests so a mistake costs
milliseconds, not the default timeout.

## Acceptance criteria

- [ ] live_a11y_source_meta: `ff-rdp a11y` output carries a `meta.source` of
      `js-fallback` against a Firefox with the accessibility service off
- [ ] live_a11y_native_opt_in: with the opt-in flag, the root role is `document` and the
      tree contains platform roles the JS fallback does not produce
- [ ] live_a11y_service_restored: after an opt-in run that enabled the service,
      `bootstrap().state.enabled` is back to its pre-run value
- [ ] unit/e2e: enable failure surfaces as an explicit error or an annotated fallback,
      never a silent one
- [ ] [[decision-log]] records the default-vs-opt-in decision

## Notes

- Do not re-litigate the protocol: iter-136 already verified it against the Firefox
  source and recorded FF153 fixtures (`a11y_walker_children_response.json`,
  `a11y_children_response.json`, `a11y_children_empty_response.json`,
  `a11y_bootstrap_response.json`).
