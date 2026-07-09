---
title: "Responsive truthful viewport emulation over RDP"
date: 2026-07-09
status: settled
type: research
tags: [research, responsive, media-queries, viewport, rdp, iteration-98]
---

# Responsive truthful viewport emulation over RDP

Research outcome for [[iteration-98-media-query-truthfulness]] Theme A: can
ff-rdp make CSS `@media` queries actually flip to a requested width over the
RDP protocol, or must it fall back to a truthful self-check?

## Question

The `responsive` command constrains layout width by setting inline CSS on
`<html>`/`<body>`. Element geometry is then correct for the requested width,
but `@media` queries continue to evaluate against the **physical** viewport,
producing a physically-impossible CSS state (e.g. `html` measuring 390px while
`(min-width: 1024px)` styles stay active). Can we drive *real* emulation so the
media environment reports the requested width?

## Candidates evaluated

1. **`window.resizeTo()` / `window.resizeBy()`** — silently ignored in
   headless mode and blocked for non-popup top-level windows in windowed mode.
   Cannot change the media environment from the content-process execution
   context ff-rdp evaluates JS in. Rejected.

2. **RDP `responsiveActor` / RDM emulation surface** — the responsive-design-mode
   viewport sizing was moved to browser-chrome APIs
   (`synchronouslyUpdateRemoteBrowserDimensions`) that are not reachable over
   the RDP protocol's content-process surface. Firefox 149+ no longer exposes a
   `setViewportSize` packet on any RDP actor reachable from an attach/headless
   session. Rejected (no reachable spec surface → nothing to `allow-spec-drift`
   against, so `firefox_refs` stays empty).

3. **WebDriver BiDi `browsingContext.setViewport`** — genuinely flips the media
   environment, but requires the BiDi WebSocket transport, not the RDP TCP
   socket ff-rdp speaks. Out of scope for this tool's transport. Rejected for
   iter-98; a future BiDi-transport iteration could revisit.

## Conclusion

Truthful width/height emulation is **not achievable over the RDP transport** in
attach or headless mode with the current Firefox tree. The layout-only CSS
mechanism is retained (geometry stays accurate for the requested width), and
Theme A ships the **self-check floor** instead of real emulation:

- After applying each viewport width, `responsive` probes
  `matchMedia("(width: <requested>px)").matches` plus `window.innerWidth` and
  writes a `media_query_check` object `{requested, inner_width, matches}` into
  every breakpoint.
- On `matches == false` a warning is attached to the envelope; `--strict` turns
  a mismatch into a non-zero exit.

This satisfies the iteration's **hard rule**: ff-rdp never presents a viewport
state where the reported width and the page's media-query evaluation disagree
without flagging it in the same JSON envelope. An honest warning is the floor;
truthful emulation remains a documented non-goal until a BiDi transport lands.

## Cross-references

- [[iteration-98-media-query-truthfulness]] — the iteration this research backs.
- [[field-report-responsive-cascade-2026-07-05]] — the field report that
  surfaced the physically-impossible state.
- `crates/ff-rdp-cli/src/commands/responsive.rs` — `SET_VIEWPORT_CSS_JS`
  (layout-only mechanism) and `MEDIA_QUERY_CHECK_JS` (the self-check).
