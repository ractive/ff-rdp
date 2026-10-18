---
title: "screenshot: no way to capture a phone-width viewport with the page's live state"
type: feature
status: open
priority: medium
discovered: 2026-10-18
tags:
  - screenshot
  - responsive
  - viewport
  - ai-agent
reporter: saturnus
ff_rdp_version: 0.4.1 (main 8abb03c1, reproduced 2026-10-18)
---

# screenshot: no way to capture a phone-width viewport with the page's live state

To check a phone layout of something that only exists after an interaction (an open
modal `<dialog>`, a menu, a filled form), the agent needs a 390px-wide screenshot of
the tab as it is now. ff-rdp has two capture paths and neither does that:

- the live RDP capture uses the window `launch` created, which clamps below about
  500 CSS px;
- `screenshot --window-size 390x844` gives the exact size, but re-navigates the URL
  in a fresh profile, so the opened dialog (and any other state) is gone.

`ff-rdp responsive` collects geometry at several widths, but no image.

## Repro

```sh
ff-rdp launch --headless
ff-rdp navigate https://ractive.ch/saturnus/
ff-rdp click '.model button[commandfor="dlg-49g"]'     # a modal dialog opens
ff-rdp screenshot --window-size 390x844 -o phone.png    # the page without the dialog
```

The workaround was a separate headless Chrome script with
`Emulation.setDeviceMetricsOverride`, which is what ff-rdp is meant to replace.

## Ideas

- Launch with a narrow window that is not clamped (`launch --window-size 390x844`
  with whatever prefs lift the minimum), documented as the phone path.
- Responsive design mode / `browsingContext` viewport override through RDP or
  WebDriver BiDi (`browsingContext.setViewport`) on the live tab.
- Or `screenshot --window-size` accepting a short script to replay (e.g. a click)
  after its fresh navigation, so state can be rebuilt.

## Partial: #338

`ff-rdp screenshot --viewport 390x844` now captures a phone-width layout on the
live tab without a second Firefox: the tab's URL is loaded into a 390x844
iframe overlaid on the page, `max-width` media queries evaluate against 390,
images in that first viewport are made eager and awaited, and the iframe is
removed afterwards. It composes with `--color-scheme` / `--media`.

Still open: the iframe loads the URL afresh, so the opened `<dialog>` from the
repro (or any other state built by earlier commands) is not in the capture.
RDP has no viewport sizing for the live tab (Responsive Design Mode is
parent-chrome CSS) and WebDriver BiDi is out of scope, so live-state capture
at phone width remains unsolved. Pages that refuse framing still need
`--window-size`.
