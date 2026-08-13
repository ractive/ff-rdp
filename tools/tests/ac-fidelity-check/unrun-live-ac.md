---
title: fixture — ticked live AC that admits its own non-execution
---

Fixture for iter-154 (`shell_154_unrun_ac_fails`). The AC below names a test that
really does exist in `crates/`, so every pre-iter-154 heuristic is satisfied — the
only thing wrong with it is that its own text says the test never ran. The gate
must fail it anyway.

## Acceptance Criteria [1/1]

- [x] live_110_replace_never_kills_foreign_firefox: replacing a managed Firefox leaves a
      foreign instance untouched — implemented and compiled; gated behind
      `FF_RDP_LIVE_TESTS=1` and not exercised end-to-end in this session's time budget
