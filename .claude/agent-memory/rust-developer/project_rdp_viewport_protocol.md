---
name: Firefox RDP viewport sizing — no actor method, use CSS simulation
description: setViewportSize was never a valid RDP packet type; viewport sizing requires CSS constraint approach for Firefox 149+
type: project
---

There is **no RDP actor in any Firefox version** that accepts `setViewportSize` as a packet type. Key findings (researched April 2026):

- `responsiveActor` in Firefox 149+ only supports: `toggleTouchSimulator`, `setElementPickerState`, `dispatchOrientationChangeEvent`
- `setViewportSize` was historically only a client-side DevTools UI method (`devtools/client/responsive/ui.js`), never an RDP protocol packet
- The `target-configuration` actor's `updateConfiguration` has no viewport width/height field
- Firefox RDM viewport sizing uses browser chrome APIs (`synchronouslyUpdateRemoteBrowserDimensions`) that are inaccessible from content-process JS via RDP
- WebDriver BiDi `browsingContext.setViewport` works but requires the BiDi WebSocket transport, not the RDP TCP socket
- `window.resizeTo()` is silently ignored in headless mode and blocked in windowed non-popup windows

**Working fix:** CSS constraint on `<html>` and `<body>` inline styles via `evaluateJSAsync`:
```js
document.documentElement.style.setProperty('width', '320px', 'important');
document.documentElement.style.setProperty('max-width', '320px', 'important');
document.documentElement.style.setProperty('overflow-x', 'hidden', 'important');
document.body.style.setProperty('max-width', '320px', 'important');
```
This makes `getBoundingClientRect` and `offsetWidth` reflect the constrained width. CSS `@media` queries still use the physical viewport. Use `documentElement.offsetWidth` (not `innerWidth`) in geometry collection JS to get the constrained width.

**Why:** setViewportSize was added in iter-32 as a "fix" but was calling a non-existent RDP packet. The CSS approach is the correct long-term solution.

**How to apply:** When implementing any viewport-simulation feature via RDP, use CSS width constraint. Do not attempt ResponsiveActor methods for viewport sizing.
