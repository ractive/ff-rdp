---
title: "WCAG color contrast ratio checking"
type: feature
status: open
priority: medium
discovered: 2026-04-07
tags: [accessibility, a11y, wcag, contrast]
---

# WCAG color contrast ratio checking

Accessibility audits require checking foreground/background color contrast ratios
against WCAG AA (4.5:1 for normal text, 3:1 for large) and AAA (7:1 / 4.5:1)
thresholds.

## Proposed command

```sh
ff-rdp a11y contrast                     # check all text elements
ff-rdp a11y contrast --selector "h1,p,a" # specific elements
ff-rdp a11y contrast --fail-only         # only show failures
```

## Output

```json
{
  "checks": [
    {
      "selector": "p.intro",
      "text": "Welcome to our site",
      "foreground": "#666666",
      "background": "#ffffff",
      "ratio": 5.74,
      "aa_normal": true,
      "aa_large": true,
      "aaa_normal": false,
      "aaa_large": true,
      "font_size": "16px"
    }
  ],
  "summary": {"total": 42, "aa_pass": 38, "aa_fail": 4}
}
```

## Implementation

Via eval: for each text element, compute foreground color and walk up the tree to
find the effective background color (handling transparency). Contrast ratio is a
straightforward luminance calculation.
