---
title: fixture — denied wording that describes behaviour, with the escape hatch
---

Fixture for iter-154 (`shell_154_allow_wording_escape_hatch`). "does not run" is a
correct description of what `--dry-run` does, not a confession about the AC. Theme A
matches literal phrases and not meaning, so such an AC needs an escape that is not
"reword until the grep stops firing" (PR #193 findings 2 and 6).

## Acceptance Criteria [1/1]

- [x] `test_token_comparison_constant_time`: `--dry-run` does not run the underlying command
      [allow-ac-wording: describes product behaviour, not this AC's own status]
