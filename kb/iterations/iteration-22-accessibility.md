---
title: "Iteration 22: Accessibility Inspection"
type: iteration
status: planned
date: 2026-04-07
tags: [iteration, accessibility, a11y, wcag, protocol]
branch: iter-22/accessibility
---

# Iteration 22: Accessibility Inspection

Full accessibility tree via Firefox's AccessibilityActor, plus WCAG contrast checking.
Essential for AI agents reviewing frontend code for compliance.

## Notes

Protocol research requires a live Firefox instance. Launch headless Firefox for
discovery: `firefox -no-remote -profile /tmp/ff-rdp-test-profile --start-debugger-server 6000 --headless`

## Tasks

- [ ] Research: discover AccessibilityActor protocol from live Firefox session.
  Document getWalker, accessible tree traversal, properties exposed.
- [ ] Implement AccessibilityActor in ff-rdp-core: getWalker, children, accessible
  properties (role, name, value, description, states)
- [ ] `ff-rdp a11y` — accessibility tree as structured JSON with `--depth N`
  and `--max-chars N` for output size control, truncation markers when pruning
- [ ] `ff-rdp a11y --selector ".main"` — subtree rooted at a DOM element
- [ ] `ff-rdp a11y --interactive` — filter to only interactive elements
  (buttons, links, inputs, selects)
  → [[accessibility-tree-command]]
- [ ] `ff-rdp a11y contrast` — WCAG color contrast ratio checking for all
  text elements, with AA/AAA pass/fail per element
  → [[wcag-contrast-checking]]
- [ ] Daemon compatibility: ensure AccessibilityActor works through daemon,
  handle `unknownActor` errors after navigation (re-discover actor and retry)
