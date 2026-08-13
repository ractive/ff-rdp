---
title: fixture — evidence in a second paragraph of the same list item
---

Fixture for iter-154 (`shell_154_blank_line_continuation_is_read`). A blank line
followed by an indented block is a second paragraph of the *same* Markdown list item
and renders as part of the AC in Obsidian. The first folding implementation treated
the blank line as the end of the AC and rejected this plan for "no run evidence"
even though the evidence is right there (PR #193 finding 4).

## Acceptance Criteria [1/1]

- [x] live_110_replace_never_kills_foreign_firefox: replacing a managed Firefox leaves a
      foreign instance untouched

      [verified: 2026-08-12, 1 passed / 0 failed, foreign PID still alive after replace]
