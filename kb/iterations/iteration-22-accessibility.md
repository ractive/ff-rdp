---
title: "Iteration 22: Accessibility Inspection"
type: iteration
status: completed
date: 2026-04-07
tags:
  - iteration
  - accessibility
  - wcag
  - protocol
branch: iter-22/accessibility
---

# Iteration 22: Accessibility Inspection

Full accessibility tree via Firefox's AccessibilityActor, plus WCAG contrast checking.
Essential for AI agents reviewing frontend code for compliance.

## Notes

Protocol research requires a live Firefox instance. Launch headless Firefox for
discovery: `firefox -no-remote -profile /tmp/ff-rdp-test-profile --start-debugger-server 6000 --headless`

## Tasks

- [x] Research: discover AccessibilityActor protocol from live Firefox session.
  Document getWalker, accessible tree traversal, properties exposed.
- [x] Implement AccessibilityActor in ff-rdp-core: getWalker, children, accessible
  properties (role, name, value, description, states)
- [x] `ff-rdp a11y` — accessibility tree as structured JSON with `--depth N`
  and `--max-chars N` for output size control, truncation markers when pruning
- [x] `ff-rdp a11y --selector ".main"` — subtree rooted at a DOM element
- [x] `ff-rdp a11y --interactive` — filter to only interactive elements
  (buttons, links, inputs, selects)
  → [[backlog/issues/accessibility-tree-command]]
- [x] `ff-rdp a11y contrast` — WCAG color contrast ratio checking for all
  text elements, with AA/AAA pass/fail per element
  → [[backlog/issues/wcag-contrast-checking]]
- [x] Daemon compatibility: ensure AccessibilityActor works through daemon,
  handle `unknownActor` errors after navigation (re-discover actor and retry)

## Test Fixtures

All e2e test fixtures must be recorded from a real Firefox instance — never hand-craft them.
Run with `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored` to record fixtures.
