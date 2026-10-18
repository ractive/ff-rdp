---
title: "screenshot --color-scheme: captures while CSS transitions are still running"
type: bug
status: resolved
resolved_by: "#336"
priority: low
discovered: 2026-10-18
tags:
  - screenshot
  - color-scheme
  - ai-agent
reporter: saturnus
ff_rdp_version: 0.4.1 (main 8abb03c1, reproduced 2026-10-18)
---

# screenshot --color-scheme: captures while CSS transitions are still running

`ff-rdp screenshot --color-scheme dark` flips the simulated scheme and captures as soon as
`matchMedia` reports it. Elements whose colours transition (here
`.button { transition: background 150ms … }`) are caught halfway: in the dark capture
the buttons kept their light-theme white background while their text had already
turned light, so they looked broken (light text on white). The page itself is fine;
its dark tokens are correct.

## Repro

Firefox 158. The saturnus landing page (https://ractive.ch/saturnus/; reproduced there 2026-10-18 on main 8abb03c1 — the "Desktop and command line" button renders white with light text); `.button` has
`background: var(--control)` with a `transition` on `background`, and the dark
`@media (prefers-color-scheme: dark)` block redefines `--control`.

```sh
ff-rdp navigate https://ractive.ch/saturnus/
ff-rdp screenshot --color-scheme dark -o dark.png   # .button: white fill, light text
```

## Expected

The capture shows the settled dark page.

## Proposed fix

After the scheme (or `--media`) change, wait for running transitions and animations
to finish (`document.getAnimations()` settled, with a cap), or disable transitions for
the capture (inject `*, *::before, *::after { transition: none !important }` on this
connection), or at least wait a couple of frames plus the longest
`transition-duration` found.
