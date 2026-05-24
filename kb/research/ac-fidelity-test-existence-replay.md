---
title: "AC-fidelity test-existence replay (iter-61w)"
date: 2026-05-24
status: complete
type: research
tags: [iteration-discipline, ac-fidelity, replay, iter-66]
---

# AC-fidelity test-existence replay (iter-61w)

iter-66 strengthens `tools/ralph-loop/scripts/ac-fidelity-check.sh` so that an
AC naming a test slug (`test_*`, `live_*`, `bench_*`) must resolve to an
`fn <slug>` somewhere in the workspace — either in the branch diff or
pre-existing under `crates/`. Naming a test that doesn't exist anywhere now
fails the check immediately rather than being rescued by a stray backtick
match downstream.

This note records the replay against iter-61w that motivated the change.

## What iter-61w shipped

Per [[iteration-61w-security-hardening-and-cleanup]] the iteration ticked
ACs for four named regression tests:

- `test_refstore_capped`
- `test_nav_boundary_url_truncated`
- `test_token_comparison_constant_time`
- `daemon_poisoned_mutex_recovery`

The plan itself self-documents that none of the four were actually written —
each is annotated `_Follow-up_` in the plan body
(`kb/iterations/iteration-61w-security-hardening-and-cleanup.md:75-80`). The
old `ac-fidelity-check.sh` accepted those ACs anyway because the heuristics
matched backtick-quoted symbols (`MAX_REFS`, `subtle::ConstantTimeEq`,
`lock_or_recover!`) appearing in the iter-61w *implementation* diff.

## Replay outcome (counterfactual)

The strengthened script (iter-66) extracts the slug from each AC line and
asserts the function exists. At iter-61w merge time none of the four `fn`
declarations existed, so every one of those four ACs would have produced:

```
❌ ticked AC names test 'test_refstore_capped' but no `fn test_refstore_capped` exists in the workspace: …
```

…and the script would have exited `1`, blocking the merge.

We can't re-run the old commit directly through the strengthened script
today because iter-66 itself adds the missing `fn` declarations
(server.rs:2447, 2496, 2557 and buffer.rs:377 were added between iter-61w
merge and the iter-66 work — three by iter-63's hygiene pass, one fresh in
iter-66) — so a literal `git checkout iter-61w && bash scripts/ac-fidelity-check.sh`
now passes for the wrong reason. The behaviour is instead verified
structurally by
`crates/xtask/tests/ac_fidelity_test_existence.rs::ac_fidelity_rejects_nonexistent_test_slug`,
which feeds the script a fabricated AC whose slug is guaranteed not to exist
anywhere in the tree (`test_nonexistent_xyzzy_iter66_guard`) and asserts the
script exits non-zero. That is the regression guard that locks in the
strengthened behaviour.

## Why a structural test, not a literal replay

A literal replay of iter-61w through the new script today would either:

1. **Pass** (wrong reason): because iter-63/iter-66 backfilled the missing
   `fn` declarations into `main`, so a `grep -r 'fn test_refstore_capped' crates`
   succeeds — the existence check is satisfied retroactively even though
   the iter-61w branch added no such function.
2. **Require shadow-restoring the tree** to the iter-61w-era state, which
   would in turn re-introduce the test debt we just paid off.

Neither is useful as an ongoing regression guard. The xtask test is: it
operates on a synthetic slug that no historical or future branch will
contain, so the script's rejection behaviour stays pinned.

## References

- [[iteration-61w-security-hardening-and-cleanup]] — original test debt
- [[iteration-66-backfill-iter61w-security-tests]] — current iteration
- `tools/ralph-loop/scripts/ac-fidelity-check.sh` — strengthened script
- `crates/xtask/tests/ac_fidelity_test_existence.rs` — regression guard
