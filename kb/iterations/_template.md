---
title: "Iteration NN: <short title>"
type: iteration
date: YYYY-MM-DD
status: planned
branch: iter-NN/short-description
depends_on: []
# If this iteration introduces new pub items, list each one with its first call site.
# Leave empty ([]) if no new pub items are introduced.
# Required by cargo xtask check-iteration-plan when the body mentions pub symbols.
first_call_sites: []
# Describe how to manually exercise this iteration's output end-to-end.
# Required by cargo xtask check-iteration-plan.
dogfood_path: |
  ff-rdp <command> <args>
  # expected output shape or observable behavior
tags: [iteration]
# Add `skill-edit` if this iteration modifies files under ~/.claude/skills/.
# Skill-edit iterations cannot run through ralph-loop (the cmux child workspace
# has no write access to ~/.claude/skills/). Drive them by hand in a regular
# Claude session. See iter-61z for the canonical example.
---

# Iteration NN: <short title>

<One-paragraph motivation: what problem does this solve and why now?>

## Themes

- **A — <Theme A>.** <One-sentence description.>
- **B — <Theme B>.** <One-sentence description.>

## Tasks

### A. <Theme A title>
- [ ] <Task step 1>
- [ ] <Task step 2>

### B. <Theme B title>
- [ ] <Task step 1>
- [ ] <Task step 2>

## Acceptance Criteria [0/N]

<!-- Each AC MUST name a test function and its asserted post-condition, per CLAUDE.md convention. -->
<!-- Example: `- [ ] live_screenshot_full_page: PNG height ≥ scrollHeight × DPR` -->
<!-- ACs without named tests are not done. -->

- [ ] <test_name>: <asserted post-condition>
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

<Architecture decisions, alternatives considered, trade-offs.>

## Out of scope

<What this iteration deliberately does NOT do.>

## References

- [[<wikilink to related plan or research>]]
