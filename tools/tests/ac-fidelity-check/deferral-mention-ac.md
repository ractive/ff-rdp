---
title: fixture — a passing mention of [deferred …] must not launder an AC
---

Fixture for iter-154 (`shell_154_deferral_mention_does_not_launder`). The deferral
short-circuit skips every remaining check, so it must fire only on an annotation that
*closes* the AC. Here the AC names a live test that exists nowhere in the workspace —
a pre-iter-154 failure — and merely mentions a deferral inside a parenthetical aside.
The unanchored first implementation passed this plan, making iter-154 a strictness
*regression* for it (PR #193 finding 1).

## Acceptance Criteria [1/1]

- [x] live_154_totally_nonexistent_test: the daemon does the thing
      (contrast the AC above, which is [deferred — new plan: kb/iterations/iteration-154-ac-fidelity-evidence.md])
