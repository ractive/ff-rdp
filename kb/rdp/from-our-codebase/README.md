---
title: ff-rdp's RDP knowledge — Index
type: index
tags: [rdp, index, from-codebase]
date: 2026-05-23
---

# What ff-rdp Already Knows

The hard-won knowledge gathered while building `ff-rdp` itself. These pages are what was reverse-engineered from dogfooding sessions, debugging, and Firefox source spelunking — *before* this wiki was built. Now they're cross-referenced into the wiki proper.

- [[actors-we-use]] — every Firefox actor `ff-rdp-core` currently wires up: file path in our crate, what we use it for, what's working, what's broken.
- [[lessons-learned]] — 20 surprising constraints we hit. Each linked to the dogfooding session or iteration that surfaced it.
- [[open-gaps]] — protocol-level gaps in our implementation, severity-tagged. Drives iteration planning.
- [[glossary]] — our working definitions of RDP terms (actor, grip, packet, watcher, resource, ...).

## How to use this folder

If you're about to add a new `ff-rdp` command:

1. Skim [[actors-we-use]] to see if we already talk to the actor you need.
2. Check [[lessons-learned]] — many actors have non-obvious quirks already documented (evaluateJSAsync's `mapped.await`, longString grips, 64 MiB frame cap, etc.).
3. Find the actor's contract in [[../actors/README|actors/]] (Firefox-side spec).
4. Find the canonical client flow in [[../flows/README|flows/]] if applicable.
5. If your work fixes an item in [[open-gaps]] (or a [[lessons-learned]] gotcha), close the loop and update the relevant page.

## Cross-references

This folder is the bridge between "what RDP is" (overview/protocol/actors/flows) and "what `ff-rdp` does today". Update it whenever an iteration lands new RDP knowledge — don't let new constraints decay into chat history.
