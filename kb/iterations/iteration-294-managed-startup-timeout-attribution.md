---
branch: iter-294/managed-startup-timeout-attribution
date: 2026-09-29
depends_on: []
dogfood_path: >-
  Investigate the three preserved iteration288 closing startup timeouts offline
  first. Admit any new startup observation only under a finite hypothesis schedule,
  with exact process ownership and the existing launch deadline. Filing does not
  authorize another capture or sweep.
first_call_sites: []
status: obsolete
type: iteration
title: "Iteration 294: Attribute managed Firefox startup timeouts"
tags:
  - iteration
  - startup
  - carry-over
---

Closed 2026-10-02 by [[step-back-2026-10-02]]: startup-timeout assertion failing at load average 150–250 under the sweep's own contention; the fix is in the process (§5: retry once then quarantine, sweep nightly), not in the product.

# Iteration 294: Attribute managed Firefox startup timeouts

Iteration288's own dual-gate closing sweep on Firefox156.0.1 failed three
managed launches before their test bodies. Keep these separate occurrences:

| Original test | Firefox PID / port | Deadline observation |
|---|---|---|
| `live_161_eval_and_flag_strictness::live_161_fields_and_sort_reject_unknown_names` | 54312 / 59458 | 30511ms |
| `live_224_with_page_connection_reset::live_repeated_hop_never_loses_the_connection` | 75826 / 63303 | 30507ms |
| `live_92_navigate_epoch::live_index_navigate_parity` | 97345 / 51217 | 30512ms |

Each root was alive at the original500ms+30s deadline. Each launch returned1;
its product cleanup records an actual child wait after SIGKILL. Stderr retained
38bytes but was redacted; EOF was false at the deadline and true after cleanup.
These observations do not identify the cause, prove internal worker return,
establish a debugger listener, or exclude a product defect. Failed profile paths
and birth tokens were not retained. None reached its flag/navigation assertions.

A bounded macOS-log comparison found the same LaunchServices timeout message in
all three failed roots and in the independently ready raw fixture; that message
alone does not distinguish the failure. One later instrumented startup opened
in710ms and sampled no blocked startup. It is a non-reproduction, not a repair
or historical explanation. Its private cleanup packet retained a failure because
two new group members appeared between censuses: only the exact qualified root
received TERM. Fresh full-run reconciliation found no unexpected survivors and
preserved real/protected state and all prior profiles. Unknown child transitions
and internal worker returns remain unknown; no broader signals or retry occurred.

## Tasks [0/3]

- [ ] Map the retained startup observations to the current launch/probe path and
      define discriminating evidence for each remaining hypothesis. Preserve the
      three occurrences independently and document missing evidence explicitly.
- [ ] If another occurrence is needed, review and declare a finite startup-only
      schedule before execution. Bind process identity and literal private trace;
      distinguish pre-debugger startup delay from listener/probe disagreement.
      Stop answered experiments; do not repeat full sweeps for discovery.
- [ ] Implement only an evidence-supported correction, if a defect is established,
      and obtain independent review plus focused proof and required final gates.
      Otherwise preserve an honest blocked checkpoint and the precise missing
      evidence instead of declaring a fix from a passing retry.

## Acceptance Criteria [0/3]

- [ ] Each original failure has an evidence-backed cause or an explicitly retained
      unresolved disposition; any repair claim has a reproducible discriminant
      and attributable before/after evidence rather than timeout classification.
- [ ] Observations and cleanup preserve original launch timing and ownership;
      process disappearance is never substituted for a worker-return receipt.
      No blind retries, widened waits or weakened assertions are used.
- [ ] Any implemented repair passes independent review and required validation;
      all historical failures and non-reproductions remain visible. If no repair
      is established, this plan remains incomplete with a recoverable checkpoint.

## Evidence and scope

Private evidence is under `.git/ralph-loop/20260924-all-open/iter288/`:
`closing-preparation1/sweep.log`, `startup-c1-offline1/occurrences.json`,
`startup-unified-log1/comparison.json`, and
`startup-diagnostic-preparation1/execution/` with `root-release1/receipt.json`.
The diagnostic's one-of-one schedule is consumed and cannot be reset by this plan.
No startup cause is assigned to262,267 or268. This is not268's original224/240
attribution work, and it does not satisfy its missing worker/ancestry evidence.
Windows259 and dependent266 remain deferred. Filing adds no execution queue.
