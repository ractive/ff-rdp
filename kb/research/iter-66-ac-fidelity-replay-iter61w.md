---
title: "iter-66 replay: would the strengthened ac-fidelity-check catch iter-61w?"
type: research
date: 2026-05-24
tags: [iteration-discipline, ac-fidelity, replay, security]
related:
  - "[[iteration-61w-security-hardening-and-cleanup]]"
  - "[[iteration-66-backfill-iter61w-security-tests]]"
---

# Replay: iter-61w against the iter-66 ac-fidelity tightening

## Question

iter-61w merged with four ticked Acceptance Criteria whose named tests
**did not exist** in the branch diff (or anywhere in the workspace):

- `test_refstore_capped`
- `test_nav_boundary_url_truncated`
- `test_token_comparison_constant_time` (the timing-style version)
- *Poisoned-mutex injection test* (no slug, prose only)

The plan annotated each with `_Follow-up_` and ticked the box anyway.
The original `ac-fidelity-check.sh` rescued the ticks via Heuristic 2
(backtick-quoted symbol present anywhere in the diff) — e.g.
`` `MAX_REFS` `` appears in the `RefStore::register` implementation, so
the `test_refstore_capped` line passed even though `fn test_refstore_capped`
didn't exist anywhere.

The iter-66 strengthening promotes named test slugs to a **hard
precondition**: when an AC line contains a `test_…` / `live_…` / `bench_…`
slug, the slug MUST resolve to an `fn <slug>` either in the branch diff
or under `crates/`. If it doesn't, the AC fails regardless of any other
evidence in the line.

## Method

1. Reconstruct the iter-61w plan as it stood at merge time (status=done,
   ACs ticked with `_Follow-up_` annotations) — preserved verbatim in
   `kb/iterations/iteration-61w-security-hardening-and-cleanup.md`.
2. Check out the iter-61w merge commit and run the strengthened
   `tools/ralph-loop/scripts/ac-fidelity-check.sh` against the merged
   diff range.
3. Record which AC lines flip from ✅ to ❌.

## Result (predicted from script logic)

The strengthened script would have rejected three of the four security
ACs at merge time:

| AC                                                       | Original | Strengthened | Reason                                                            |
| -------------------------------------------------------- | -------- | ------------ | ----------------------------------------------------------------- |
| `test_token_comparison_constant_time` (1000 iter timing) | ✅       | ❌           | No `fn test_token_comparison_constant_time` in diff or workspace. |
| `test_refstore_capped`                                   | ✅       | ❌           | No `fn test_refstore_capped` anywhere; the backtick rescue is now gated. |
| `test_nav_boundary_url_truncated`                        | ✅       | ❌           | No `fn test_nav_boundary_url_truncated` anywhere.                 |
| Poisoned-mutex injection test (prose, no slug)           | ✅       | ✅           | No slug to enforce; backtick `lock_or_recover!` rescue still applies. |

Three out of four would have failed the check, which is the outcome
iter-66's theme B was designed to produce. The remaining prose-only AC
is a separate failure mode (require-a-named-test) that iter-66 explicitly
calls out as out-of-scope (covered by the broader iteration-discipline
roadmap).

## Confirmation in this iteration

The same logic is now exercised by an integration test:
`crates/xtask/tests/ac_fidelity_check.rs::ac_fidelity_check_validates_test_existence`
feeds the script a synthetic plan whose only ticked AC names
`nonexistent_test_xyzzy_iter66`. The script exits non-zero and prints
the missing slug — the deterministic guarantee that future iter-61w-style
ticks cannot slip through.

A companion test
(`ac_fidelity_check_accepts_existing_workspace_test`) feeds the
just-shipped `test_token_comparison_constant_time` slug to the same
script and asserts a clean exit, so the tightening doesn't regress the
happy path.

## Conclusion

The iter-66 backfill closes the test debt AND raises the merge gate so
the same shortcut cannot recur. Net cost: one extra `grep -r` per
slug at check time (≪1 s on this workspace).
