---
title: "Iteration 278: missing launch receipt in the parallel timeout harness test"
type: iteration
date: 2026-09-20
status: done
branch: iter-278/parallel-launch-timeout-missing-receipt
depends_on: []
first_call_sites: []
dogfood_path: cargo test --workspace -q
tags: [iteration, carry-over, testing]
---

# Iteration 278: missing launch receipt in the parallel timeout harness test

Filed during262 foundation integration. This is a Firefox-free fake-launch test,
separate from262's target acquisition behavior and its live proof. No278 execution
is authorized by the selected262/268/271/259/266 run.

## Observations

Two default-parallel `cargo test --workspace -q` runs failed
`harness_session::isolated_launch_timeout_still_runs_scoped_cleanup_and_passes_product_bound`
at `crates/ff-rdp-cli/tests/e2e/harness_session.rs:255`: reading the fake
`launch-timeout` receipt returned ENOENT. Both e2e targets reported376passed/1failed;
the preceding CLI unit target passed1267/0 with2ignored. Exact isolation between
those runs passed1/1. The failed test is unchanged from merged foundationPR263.

The fixture supplies a1-second outer timeout for a fake launch ending in
`exec sleep 10`, requires the forwarded product timeout7, checks process reaping
and scoped cleanup, and expects total elapsed under3seconds. Missing receipt
does not alone prove why the child failed to write it. Do not label a later serial
pass a correction or assume scheduling causality without evidence.

Evidence: primary checkout `.git/ralph-loop/20260920-foundation-repair/iter262/repair2/validation/`,
`tests-final.{json,log}`, `isolated-existing-harness.{json,log}` and
`tests-rerun.{json,log}`. Preserve the earlier foundation spike's timeout-fixture
history too; it previously changed a100ms deadline to1second after a missing-receipt
failure. That history does not establish this occurrence's cause.

## Tasks [3/3]

- [x] Capture the fake child's startup, argument receipt and deadline/cleanup ordering for an attributable failure without launching Firefox.
- [x] Correct the demonstrated fixture or harness mechanism while preserving real timeout, forwarded-product-bound, reaping and scoped-cleanup assertions.
- [x] Add a meaningful regression, retain prior failures and isolation controls, and run the required ordered gates with default-parallel workspace validation.

## Acceptance Criteria [3/3]

- [x] The missing receipt has an evidenced explanation, with startup and cleanup distinguished.
- [x] The repaired test still proves actual launch timeout, product-bound propagation and owned child cleanup; no skip, fabricated receipt or retry-only masking.
- [x] Required ordered gates and default-parallel workspace validation pass on the final source; remaining failures have explicit dispositions.

## Execution clarification — 2026-09-23

The owner authorized the 272–278 queue after parking 268. Earlier dated unselected-run
boundaries are historical; 271 remains excluded and 259/266 constraints remain binding.
Original tasks and acceptance criteria above are unchanged.

The fake-launch fixture and shared harness are byte-identical between PR 263 and merged
PR 264/c761c1b1. The one-second outer deadline begins before spawn; launch-timeout is a
child-written fixture receipt, distinct from product JSON. Historical ENOENTs remain
unattributed. Use one controlled pre-receipt startup-delay case, one ordinary exact-test
control and one instrumented normal-parallel workspace run after observation review;
stop at an attributable failure. Retain parent spawn/deadline/kill/actual-wait, child
receipt-write results, stderr and cleanup evidence. A forced delay proves a fixture
hazard, not either historical cause. Repairs must retain actual timeout, seven-second
argument propagation, reap, exact home/port cleanup and total < 3s contract; never
manufacture receipts or silently move the clock. Prefer this Firefox-free work before
273 operationally, without claiming a shared cause.

## Implementation and evidence — 2026-09-23

The accepted controlled occurrence establishes a test-contract hazard: the parent
may correctly reach its pre-spawn one-second deadline and kill/reap its child
before the child writes a launch receipt. In controlled2, the child parsed the
actual argument `7` then stopped before its first receipt write. The parent
observed the deadline at 1,001,092us, actually reaped the killed child at
1,005,874us, and completed cleanup for the exact requested home/port at
1,035,787us. The original test failed with the missing receipt (exit101), despite
that correct timeout and cleanup. This is an attributed controlled failure;
the two historical parallel ENOENTs and earlier 100ms occurrence remain unknown.
Cleanup used a separate private home and did not remove the fixture receipt.
The ordinary original test control passed once, including real receipt `7`,
kill/wait and exact scoped cleanup, in 1.035965s. No shared cause with273 is claimed.

The test now separates two independent obligations through the same launch caller:

- `isolated_launch_forwards_product_bound_to_child` uses a prompt real child,
  which records its actual argument then exits42. Its outer allowance is the
  existing ordinary harness calculation (product7s + shutdown5s + margin500ms),
  so proving an argument receipt does not race a deliberate one-second kill.
- `isolated_launch_timeout_without_child_receipts_reaps_and_runs_scoped_cleanup`
  executes the same finite sleep10 before any receipt. Its original stopwatch
  still surrounds precisely the launch operation: deadline1s begins before
  spawn and total must remain <3s. Parent-owned PID, requested home/port and
  actual kill/wait diagnostics are the authority. The test verifies successful
  kill/reap, child absence, exact cleanup invocation, home removal and absence
  of all four launch receipts. There is no handshake, retry or synthetic receipt.

The original repair changed only `tests/common/mod.rs` and `tests/e2e/harness_session.rs`. Temporary
observers were archived and removed; private collector code is not shipped.
No product runtime changed, so this iteration has no applicable live-Firefox
sweep. Both live gates stayed unset for tests; ordinary workspace passes are not
live proof. The script gate alone used `FF_RDP_LIVE_TESTS=1` to reach its explicit
no-script skip; source inspection verified that this plan has no runnable script.

The two repaired focused tests passed individually. Six finite mutations each
failed the intended assertion with exit101: argument0 instead of7; deadline20s
instead of1s; omitted kill (actual sleep10 wait violates total<3s); omitted wait
(explicit wait error); wrong cleanup home; and wrong cleanup port. All mutated
bytes, compiler results, actual outer waits and failures were retained separately,
then exact candidate source was restored. No guard failure counts as a mutant
assertion result.

Private evidence is under `.git/ralph-loop/20260923-remaining272-278/iter278/`:
`controlled2-outcome.md`, `ordinary1/`, `observer-archive/`,
`implementation/validation-schedule.md`, `implementation/positive-propagation2/`,
`implementation/positive-timeout/`, the six `implementation/mutation-*/` directories,
and `final-gates/`. The original instrumented workspace discovery slot remains
unspent because controlled2 answered the selected mechanism; the final ordinary
parallel workspace gate is a separate required validation, not a discovery retry.
Actual token usage is unavailable.

Initial candidate ordered validation on the restored source: stable toolchain update,
`cargo fmt`, strict all-target workspace clippy, then default-parallel
`cargo test --workspace -q`: 2536 passed, 0 failed, 419 ignored across37 result
summaries (including doctests). The ignored/live-unexecuted coverage is unchanged
in applicability; no Firefox was launched. This initial source was independently reviewed; the sole P2 correction and its
final validation are recorded below.

All nine offered `check-*` commands were accounted for: iteration-plan (this
plan and directory), source invariants, actor/KB sync against iteration base
`f2b9bfba2790b17b8ac343b0edb0eeeb8607b527`, live-test layout, skill drift,
help idioms and vendored JS passed; Firefox refs reported no references and
dogfood reported no script. Branch-to-plan resolution passed. Hyalo HYALO005
checked476 files with zero violations. These no-input checks are not runtime
coverage. No new plan was filed: every finding has the disposition below.

## Initial review correction — independently approved

Independent implementation review found one P2: serializing `home.path()` with
`json!` can panic for valid non-UTF-8 Unix paths before scoped cleanup. The
selected receipt-dependency repair and existing evidence were otherwise accepted.
AC2/3 were reopened while the correction awaited scoped review;
the recorded 2536/0/419 run remains evidence only for the earlier frozen source.

The single attempted helper regression compiled, but its before-case failed at
owned non-UTF-8 parent creation with errno92 (`Illegal byte sequence`) on this
macOS APFS host. This is a setup failure, not reproduction of the JSON panic.
Its actual outer wait101 and clean finalization are retained under
`repair-nonutf8/`; no after-case or subsequent tests ran. The attempted test was
archived and Rust source was restored to the reviewed frozen candidate at that
handback. The supervisor then authorized the finite split validation below.
No global environment change, new filesystem, Firefox or background work.

The repair extracts a test-harness diagnostic helper: exact UTF-8 `home` is
optional (null when unavailable), while `home_display` is explicitly accompanied
by `home_display_is_lossy`. The actual original `Path` continues unchanged into
scoped cleanup; diagnostic display never becomes cleanup authority.

The in-memory Unix byte-path helper test reproduced the exact old JSON panic
(`path contains invalid UTF-8 characters`, exit101), then passed with the repair.
This proves diagnostic serialization, not creation of a non-UTF-8 APFS directory.
A separate real owned-home integration test passed both outcomes once: actual
cleanup child status0 removed its home; actual child status9 preserved its home
for inspection, then the test explicitly removed only that owned empty home.
Both compare exact original cleanup bytes/home/port and verify child absence.
The existing actual argument7 and timeout-without-receipts tests also each passed
once (timeout test1.11s, unchanged pre-spawn1s and total<3s). All outer actual
waits were timely, without watchdog or collector errors. Evidence is in
`repair-nonutf8-split/`, separate from the invalid APFS setup occurrence.

The original six mutation results and controlled attribution remain applicable:
their tested argument/deadline/kill/wait/cleanup authority paths and assertions
are unchanged by this representation-only repair. No mutation or native collector
qualification was repeated. The new private runner changes only fixed test-name
selectors; no extra capture or discovery workspace slot was spent.

Repaired-source final ordered gates passed: stable update, fmt, strict all-target
workspace clippy, then default-parallel workspace2538passed/0failed/419ignored
across37 result summaries. The two additional tests account for the pass-count
increase; no Firefox was launched. Tasks3/3 are implemented and validated;
The fresh scoped review approved the repair with zero actionable findings;
all three original ACs are now satisfied without changing their wording.
Review records are `review-implementation/report.md` and
`review-nonutf8/report.md` (SHA256
`0a80193d4dba5b2540354faf58fc906f56fc5e12350c59af20468f3422df53d4`).
The supervisor verified completion-only bookkeeping; exact-head CI and merge
remain publication gates.

## Exact-head CI follow-up — 2026-09-24, in review

The previously approved278 receipt/cleanup correction remains unchanged. Its
controlled cause and focused/mutation/non-UTF-8 evidence above are reused only
for those unchanged inputs. Exact-head CI ondd32a3b failed a separate unit262
submission-handover fixture: macOS run35918983412/job107377755017 reported
1283passed/1failed/2ignored, with `predicate/ok: Err(Timeout("waiting for submission target handover"))`.
The label cannot distinguish non-exhausted poll_result400ms from request_ack1000ms.
No relationship to278 or a262 caller defect is established.

One isolated diagnostic passed21 cases/42 actual joins. Parallel1 stoppedINVALID125
with zero named-case evidence on a native collector gap. A reviewed and finitely
qualified collector allowed parallel2 to reach all21 named cases/42 joins, which
passed, but that full-target occurrence stoppedINVALID125 on EPERM:1155observed
ok/2ignored/129without verdict, no target summary. Both invalid occurrences and
all qualification/setup failures are preserved; they are not successful gates.
The original CI cause remains unknown and is carried to planning-only
[[iteration-281-ci-submission-handover-timeout-attribution]]. No281 execution,
additional local parallel capture or native collector repair is authorized.

This accumulated delta adds durable diagnostics solely under test compilation in
`commands/type_text.rs`: complete case identity, the caller’s actual deadline,
request/snapshot/write progression, caller return or panic, normal peer-return
observations and both actual joins. Activation is scoped to the fixture’s caller
thread and reset on unwind; no process environment flag or mutation is required.
Large generated JavaScript is omitted from request logs; actor/type/result identity
is retained. Acquired peers join before outcome assertions, including caller panic
and early unwind; absent normal returns remain explicit. Existing2s read limits
are retained and peer writes also receive2s limits so diagnostic-error cleanup
cannot block indefinitely in a socket write. No readiness handshake, retry,
400/1000ms increase or original assertion relaxation is introduced. Runtime builds
contain no observer; no native collector ships. A live-Firefox sweep is inapplicable
to this compiled-out test-only delta.

The finite local verification is one focused21-case invocation, then required
ordered fmt/strictclippy/default-parallel workspace gates. Each test invocation
uses an exclusively owned private FF_RDP_HOME, with HOME unchanged and private
state retained. Results and commands are in `durable-ci-diagnostics/` under the
private iteration evidence root. Focused validation passed21 cases; this is not
CI causality. Final ordered fmt/strictclippy/default-parallel workspace gates passed on the
durable candidate:2538passed/0failed/419ignored across37 summaries. All nine
xtask checks were accounted for; this plan and new281 passed, the directory
reported283 plans with0failures/93warnings, and HYALO005 checked477 files with
zero violations. No-script/no-Firefox-reference checks remain non-runtime skips.
The first strict clippy101 (unused import, two join closures and a needless
borrow) is preserved; only those equivalent idioms were corrected before the
final ordered gates. The focused21-case evidence remains applicable and was
not repeated. Fresh independent review approved the entire accumulated diagnostic delta and
the planning-only281 disposition with zero actionable findings. Original278
AC3/3 is restored without changing its wording; the controlled AC1/2 interpretation
is unchanged. Review: `review-durable-ci/report.md`, SHA256
`baa5e1578ac30e9450c5ac087ef832ab7ff69da3d1c120df270659d29fecf637`.
The supervisor verified completion-only bookkeeping and admits one exact new-head
CI observation. Every required check must be green on that head before merge.
Green CI will not explain the old failure or close281.

## Carry-over

| Result or finding | Disposition |
| --- | --- |
| Two historical parallel ENOENTs and earlier100ms fixture failure | No plan: historical causes remain unknown; controlled evidence establishes and repairs the independently reproduced receipt/deadline contract hazard. A new receipt failure on the repaired tests requires investigation, not a scheduling assumption. |
| Controlled1 exit125 despite inner failure | No plan: invalid collector occurrence preserved in `controlled1*`; no accepted attribution from it. The owned-child identity race was corrected and independently reviewed before separately granted controlled2. |
| First repaired propagation attempt exit125 | No plan: collector failure, no test verdict. Evidence and exact owned-home recovery retained in `implementation/positive-propagation/` and `stopped-validation.md`; distinct resumed propagation passed after collector qualification. No historical zombie attribution. |
| Native libproc qualification exit101 | No plan: retained `collector-zombie1` counterexample (libproc0/ESRCH before actual owned wait42), not an observed zombie. The branch was not accepted from this failure. |
| Collector native identity gap | Closed in this iteration's private evidence tooling: SDK-defined sysctl identity/status fallback only for libproc0/ESRCH, independently reviewed. Separate negative and real owned-child controls observed matching PID/PPID/PGID/birth and actual zombie status, followed by actual wait42 and absence; SID remained unavailable. No product code or inferred wait. |
| Original controlled2 exit101 | Closed in this PR: split argument propagation from timeout-without-receipts, with unchanged original timeout/total and owned cleanup contract. |
| Six expected mutation failures | Closed in this PR: each targeted assertion rejected its defect; failures and exact restoration retained. |
| Earlier observer/guard review findings | Closed in private packets before their respective admissions; every version and failed occurrence retained. Final source contains no temporary observer or collector framework. |
| First dogfood gate exit1 with live gate unset | No plan: the iteration-branch env check precedes the no-script skip. Re-invoked only that check with its required env after verifying no script exists; no Firefox, script execution or live proof. Initial failure retained in `final-gates/dogfood.*`. |
| Directory plan gate reports93 plans with warnings, zero failures | No plan: unchanged other-plan warning inventory retained in `final-gates/all-plans.stdout`; this plan passes without warnings. No changes to other plans or expansion of the authorized queue. Supervisor retains batch inventory reconciliation. |
| P2 diagnostic panic before cleanup | Repaired in this PR, independently approved with zero actionable findings: optional exact UTF-8 home and explicitly marked lossy display; original path unchanged for cleanup. Exact helper before101/after0, real cleanup success/failure outcomes and final2538/0/419 ordered gates passed. |
| Non-UTF-8 regression before-case setup failure101 | No plan: preserved separately in `repair-nonutf8/before/`, not reproduction. Supervisor-approved split diagnostic/real-cleanup validation passed without resetting this spent case; actual non-UTF-8 filesystem integration was not measured on APFS. |
| dd32 exact-head macOS unit262 Timeout | Planning-only [[iteration-281-ci-submission-handover-timeout-attribution]]; durable diagnostics address missing evidence, not the unknown cause. Final2538/0/419 ordered gates passed; fresh independent review approved this disposition with zero actionable findings. |
| Local CI diagnostic controls | Isolated21/42 passed; parallel1 and parallel2 remain INVALID125 with their distinct native errors. Parallel2’s complete named21/42 evidence is retained separately from the incomplete target. No further local capture/collector repair. |
