---
title: "Iteration 23: Native DOM & CSS Inspection"
type: iteration
status: planned
date: 2026-04-07
tags: [iteration, dom, css, inspector, protocol]
branch: iter-23/dom-css-inspection
---

# Iteration 23: Native DOM & CSS Inspection

Implement native protocol actors for DOM and CSS inspection — essential for
debugging layout issues during development.

## Research

- [ ] Protocol discovery: attach to InspectorActor, get walker and pagestyle
  actor references from a live Firefox session
- [ ] Document the request/response format for getComputed, getApplied, getLayout

## Tasks

- [ ] Implement InspectorActor in ff-rdp-core: get walker + pagestyle references
- [ ] Implement WalkerActor: `querySelector`, `children`, `documentElement`
- [ ] Implement NodeActor: node grip with attributes, tagName, nodeType
- [ ] Implement PageStyleActor: `getComputed`, `getApplied`, `getLayout`
- [ ] Add `ff-rdp styles <selector>` command — computed styles as JSON
- [ ] Add `ff-rdp styles <selector> --applied` — applied CSS rules with
  stylesheet source locations and specificity
- [ ] Add `ff-rdp styles <selector> --layout` — box model breakdown
  (margin/border/padding/content per side)
- [ ] Add `ff-rdp dom tree <selector> --depth N` — structured DOM subtree
  via WalkerActor instead of eval. Include `--max-chars N` cap with truncation
  markers (`"[... 42 more children]"`) per the output size control pattern.
  → [[native-dom-css-actors]]
- [ ] Daemon compatibility: ensure Inspector/Walker/PageStyle actors work through
  daemon, handle `unknownActor` errors after navigation (stale actor re-discovery)
