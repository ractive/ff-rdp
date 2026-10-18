---
title: "click: false click_offscreen on pages with scroll-behavior: smooth"
type: bug
status: resolved
priority: medium
discovered: 2026-10-18
tags:
  - click
  - scrolling
  - ai-agent
reporter: saturnus
ff_rdp_version: 0.4.1 (main 8abb03c1, reproduced 2026-10-18)
resolved_by: 337
---

# click: false click_offscreen on pages with scroll-behavior: smooth

`ff-rdp click` refuses with `click_offscreen` on an element far down a page whose
stylesheet sets `html { scroll-behavior: smooth; }`. Run again straight away, the same
click works. The scroll to the element animates, and `click` seems to measure the
element's position before the animation ends.

## Repro

Firefox 158, `ff-rdp launch --headless` (viewport 1366×683). The page is the saturnus
landing page (https://ractive.ch/saturnus/; reproduced there 2026-10-18 on main 8abb03c1); its CSS has `html { scroll-behavior: smooth; }` (and
`auto` under `prefers-reduced-motion: reduce`). The target is a card button about
1200px down the page.

```sh
ff-rdp navigate https://ractive.ch/saturnus/
ff-rdp click '.model button[commandfor="dlg-28s"]'
# {"error":"selector … matched an element whose centre point is still outside the
#  viewport after scrolling it into view — no click was dispatched. It may be inside a
#  clipped or zero-size scroll container; …","error_type":"click_offscreen",
#  "matched":true,"reachable":false,"obscured_by":null}
ff-rdp geometry '.model button[commandfor="dlg-28s"]'   # in_viewport: true
ff-rdp click '.model button[commandfor="dlg-28s"]'      # clicked: true
```

The error's hint (a clipped or zero-size scroll container) sends the reader the
wrong way: the element is neither.

## Expected

The click scrolls the element into view and clicks it on the first try, whatever the
page's `scroll-behavior`.

## Proposed fix

Scroll with `element.scrollIntoView({ block: "center", behavior: "instant" })`
(`instant` overrides the page's CSS `scroll-behavior`), or wait until `scrollend` /
the scroll position settles before measuring. If the check still fails, mention smooth
scrolling in the hint.

## Root cause (confirmed 2026-10-18)

`HIT_TEST_JS` in `crates/ff-rdp-cli/src/commands/js_helpers.rs` calls
`scrollIntoView({block: 'center', inline: 'center'})` without `behavior`, so the page's
CSS `scroll-behavior: smooth` applies and the rect is read mid-animation. `'auto'` would
not fix it — in `scrollIntoView` it defers to the CSS value; it must be `'instant'`.
`scroll --text` (`scroll.rs`, `behavior: 'auto'`) has the same flaw and returns a
mid-scroll rect.
