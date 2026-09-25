---
title: "Iteration 279: Attribute the navigation wall/reported timing gap"
type: iteration
date: 2026-09-23
status: done
branch: iter-279/timing-regions-20260925
depends_on:
  - "264"
first_call_sites: []
dogfood_path: >-
  On owned Firefox, run the exact
  live_navigate_default_fast::live_navigate_elapsed_matches_wall test with FF_RDP_LIVE_TESTS=1, retaining TIMING_SAMPLE and
  command/source/process identities. Before further capture, declare a finite
  source-informed schedule that attributes spawn, connection, navigation, output and teardown
  time without changing the existing 750ms or lower sanity assertions. Preserve any
  unavailable attribution honestly.
tags:
  - iteration
  - carry-over
  - timing
---
# Iteration 279: Attribute the navigation wall/reported timing gap

Completed under the later all-open execution grant on2026-09-25. The original
filing was planning-only and outside272–278; dated preparation notes below retain
their historical state. The original timing assertions and public elapsed-time
meaning are unchanged. Completion evidence follows those historical notes.

## Evidence and historical boundary

Iteration272's first closing dual-gate sweep (2026-09-23, default six workers)
failed `live_navigate_default_fast::live_navigate_elapsed_matches_wall`:

| Observation | External wall | Reported elapsed | Absolute gap | Host load averages |
|---|---:|---:|---:|---|
| Closing sweep failure | 1669 ms | 860 ms | 809 ms, above750ms bound | 150.59 / 109.97 / 59.86 |
| One isolated control | 545 ms | 291 ms | 254 ms | 55.07 / 93.08 / 59.23 |

The sweep had347passes/1failure across348exactly reconciled names. The isolated
control does not erase the failure, establish its cause, or make the sweep pass.
Raw `TIMING_SAMPLE`, process/output records and command receipts remain under
`.git/ralph-loop/20260923-remaining272-278/iter272/` in the primary checkout,
especially `sweep.log`, `navigate-timing-isolation.log` and
`post-sweep-processes.txt`. No retrospective stage timestamps are available for
this occurrence, so attribution is currently unknown.

[[iteration-246-sweep-load-misclassification]] corrected the earlier inference:
external CLI wall time includes connection, process and teardown work outside
internal navigation dispatch timing. A larger external number under contention
can therefore coexist with an honest reported interval; it is not proof that
iteration122's internal timing regressed.

[[iteration-264-sweep-load-timing-bounds]] retained the750ms gap assertion and the
reported-elapsed lower sanity assertion, adding load diagnostics. Its ten isolated
gaps were179–227ms and ten deliberately loaded gaps249–656ms. It proved sensitivity
to the iteration122 mutation (`elapsed_ms=1`). Those historical measurements do
not cover the new809ms occurrence. Load is context, not an automatic waiver; a
later green sample does not establish why the earlier one failed.

## Tasks [3/3]

- [x] Map the exact external and product timing boundaries in
      `live_navigate_elapsed_matches_wall` and the navigate/connect/output paths.
      Define correlated run identities and monotonic intervals that distinguish
      spawn/connection, navigation dispatch/readiness, output and process teardown,
      explicitly retaining unobserved intervals and any clock/correlation limits.
- [x] Predeclare a finite, source-informed owned-browser measurement schedule
      and its stop conditions before execution. Preserve every failed occurrence,
      exact binaries, route, timing fields, host/process context and owned-child
      waits. Obtain attribution for a failing occurrence; if none is captured,
      report that limit and leave the attribution criterion unticked. Do not use
      repeated full sweeps as discovery or tune execution to select a green run.
- [x] Apply only a demonstrated product or measurement correction. Keep the
      original750ms gap and lower sanity assertions active, add meaningful
      coverage for the established cause, and verify the timing-regression
      mutation still fails. Preserve the public elapsed-time meaning.

## Acceptance Criteria [3/3]

- [x] `live_navigate_elapsed_matches_wall`: an attributable failing occurrence
      records which measured interval explains the gap, with matching run/source
      identity and explicit unknown time; load alone is not treated as cause.
- [x] `live_navigate_elapsed_matches_wall`: the demonstrated correction passes
      the unchanged750ms and lower sanity assertions; reintroducing dishonest
      iteration122-style elapsed reporting still fails a meaningful regression
      assertion. No conditional load exemption, early-return pass or threshold
      widening is used.
- [x] Ordered current-stable fmt, strict workspace/all-target Clippy and normal
      parallel workspace tests pass, followed by this iteration's own dual-gate
      closing sweep with exact all-tier verdict and profile reconciliation.

## Out of scope

Executing279 as part of272–278; changing auto-wait deadlines or diagnostics;
changing the public elapsed-time definition to fit an observation; widening
assertions or waits; interpreting load or isolated green runs as automatic
exoneration; reviving268/271 or unrelated launch/ownership investigations.

## Focused entry command (not executed for279)

After the finite schedule and attribution design above are approved for a future
279 run, the existing test can be invoked without a whole-sweep discovery run:

```sh
FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live \
  live_navigate_default_fast::live_navigate_elapsed_matches_wall \
  -- --exact --include-ignored --test-threads=1 --nocapture
```

A passing invocation supplies a control, not attribution for the preserved red.

## Authorized preparation — 2026-09-25

The owner's later all-open grant includes279; the opening planning-only and
272–278 exclusion statements above describe the original filing boundary. The
original plan is preserved at SHA-256
`baa9bc0673c84cc1c7df75593df1ff130d806a0e17f6911d3a07ba70c86ed467`.
Its original tasks, acceptance criteria and historical failures remain intact.

Preparation uses branch `iter-279/timing-regions-20260925` at base
`9aff4a66116783aae7bcf4a63d5a7187763f8ed9`. Only the previously independently
reviewed timing diagnostics from272's `closing-repair1` were imported:
`commands/navigate.rs`, `tests/live/live_navigate_default_fast.rs` and the
CLI/mock test in `tests/e2e/navigate.rs`, renamed
`e2e_279_navigation_timing_records_same_command`. No272 deadline/frame/route
repair or unrelated plan was imported. There is no demonstrated timing repair.

Task1's source mapping is documented privately under
`.git/ralph-loop/20260924-all-open/iter279/initial-design/report.md` in the
primary checkout. The ten same-PID/scope monotonic markers distinguish predispatch,
aggregated post-snapshot work, output and a combined outside-run residual. Public
elapsed is sampled before status grace; `commit_resolved` is later, after resource
cleanup. That aggregate and the outside-run residual are not individual teardown
or worker-return measurements. The prior isolated272 capture passed
wall489/reported240/gap249ms; it attributes neither preserved failing occurrence.

Task2's proposed next control is exactlyone original timing occurrence under eight
owned CPU workers, with a fixed five-second warmup before browser launch. The
private `iter279/load-owner1` wrapper/control packet requires independent review,
browser-free control qualification and fresh root ownership admission before any
capture. Compilation is not execution or ownership qualification. No capture or
control is launched by this preparation note, and a new passing case would leave
the failing-occurrence criterion unmet.

Task3 and all acceptance criteria remain unticked. The unchanged750ms gap and>5ms
sanity assertions remain active, as does the public dispatch-to-commit meaning.
The1ms mutation check is owed after a demonstrated correction; preparation does not
spend it. Ordered nonlive validation and binary/source receipts are recorded under
`iter279/nonlive1`; they do not substitute for an attributable failing occurrence,
an effective correction or279's own dual-gate closing sweep.

## Narrow repair preparation — 2026-09-25

The required272 closing2 sweep supplied a newly qualified failing occurrence:
wall1301/reported415/gap886ms, with ten same-PID timing records. Root verified
its574 source inputs, Firefox110 pins, actual binaries, launch and profile
conservation in `iter272/closing2/timing-failure-qualified.json` and the adjacent
qualification receipts. The closing remains348passes/1failure of349; this does
not erase either historical timing failure. The prior279 eight-worker native1
control passed and its finite schedule is closed; it is not repeated here.

The failing commit_resolved-to-return region is240.216250ms. That aggregate
contains the final refresh, optional-wait checks, result assembly, diagnostics
and any descheduling. It does not assign all240ms to one function. The source
analysis and finite next proposal are preserved privately in
`iter279/failing-occurrence-design1/report.md` (SHA-256
`43fd82c044fbfca452fa9d59e447d3cb95f673c3d439d43ba876c769513e45d7`).

Repair1 removes only the final target refresh for a committed core navigation
whose connection has no remaining wait or page consumer. The connection and
its target registry do not escape run_core. No-wait, wait-text, wait-selector,
wait-for and with-page retain the existing refresh/latch sequence; network,
history, fallback and readiness helpers are unchanged. Public elapsed remains
dispatch-to-commit, with the original750ms gap and>5ms assertions unchanged.
Separate debug-only postcommit_refresh begin/end/skipped records retain all ten
original core/run markers.

The actual CLI/mock proof fails before the guard on an unused getTarget after
subscription teardown. Consumer controls exercise changed document actors and
script navigate-to-eval on a separately accepted connection. Exact unchanged
behavior U source, five executable copies, dep-info, compile invocations and
before-failure logs are preserved under `iter279/repair1/U`; C validation and
freeze receipts are under `iter279/repair1`. A refused provenance freeze with
stale Cargo build-script Git-watch paths was retained, then corrected by a
source-byte-preserving build-script invalidation and verified rebuild.

All original tasks and acceptance criteria remain unticked (AC0/3), pending
combined independent evidence/repair review, native correction and1ms mutation
proof, and this iteration's own dual-gate closing sweep. The proposed finite
U/C/M sequence needs separate review/admission: one unchanged diagnostic case,
one corrected case, then only if C qualifies one dishonest elapsed=1 case.
No native or mutation case is executed by this repair-preparation phase. No
loaded retries, lifecycle holds, timer-origin changes or threshold exemptions
are introduced. C gates and exact freeze outcomes remain in the private receipts.

Repair1 nonlive validation passed:12 navigate CLI/mock tests; ordered stable
fmt, strict workspace/all-target Clippy, then normal parallel workspace
2555passed/0failed/420ignored. The nested282 child summary is excluded from
that workspace total. All command receipts retain actual exit codes and
fresh private FF_RDP_HOME paths. This is not native correction acceptance.

## Completion — 2026-09-25

Original tasks3/3 and acceptance3/3 are fulfilled. Product repair and focused
regressions passed independent review (`iter279/review-repair1/report.md`);
the completed finite native proof and exact corrected-source restoration passed
independent review by `review279_native_evidence` with zero actionable findings.
All private paths here are relative to the primary checkout's
`.git/ralph-loop/20260924-all-open/`.

AC1 uses the attributable886ms failure in `iter272/closing2/`:
connection231.264500ms, predispatch359.456834ms, dispatch-to-commit minus
public elapsed0.491625ms, postcommit240.216250ms, call/drop0.071500ms,
postcore42.637583ms, output0.094625ms, precall0.051500ms and combined
outside-run/rounding11.715583ms. These sum to886ms. The postcommit region is
an aggregate, not a measurement of refresh alone; historical809/897ms gaps
remain unexplained.

AC2's finite native U/C/M schedule is closed with one qualified occurrence each:
unchanged behavior498ms wall/244ms reported; corrected483/253ms; dishonest
elapsed=1 mutation464/1ms, failing the unchanged original lower sanity assertion.
The unchanged750ms gap and>5ms lower assertion remain active. Actual test exits
were0/0/101 and runner exits0/0/1. Original U refresh took2.901125ms in that
occurrence only. No U/C speed difference is assigned wholly to the correction.
`iter279/native-proof1/` retains raw records, qualified ownership, actual waits,
source/binary pins, all failures and exact restoration to C. No mutation remains.

AC3:12 focused CLI/mock tests passed; ordered stable update, fmt, strict
workspace/all-target Clippy and normal parallel workspace tests passed with
2555passed/0failed/420ignored. These results apply to the restored exact C source.
The own closing sweep at03:01CEST passed348/348 across all six exact tiers
(CLI337 and core1/3/3/2/2), with both live gates enabled:

```text
LIVE_SWEEP_SUMMARY executed=348 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=348
LIVE_SWEEP_PROFILES leaked=0 unattributed=0
```

`iter279/closing1/` retains the full profile-root summary, actual sweep exit0,
all-tier reconciliation, raw-browser actual wait, source/binary qualification and
cleanup. All ten added profiles were attributed and baseline profiles conserved.
All nine offered xtask checks passed; dogfood explicitly skipped because this
plan has no dogfood_script and is not counted as live proof. Final documentation
checks are recorded separately; unchanged source tests are not repeated.

## Carry-over

| Observation | Disposition |
| --- | --- |
| Unused final target refresh after subscription teardown | Closed in this PR: guard in run_core; before-failing CLI/mock regression, five consumer controls, script connection control and native corrected pass. |
| Attributable886ms timing failure | Closed in this PR under the original attribution criterion: measured regions and explicit residual above; unchanged timing assertions, meaningful mutation failure and own closing pass. This does not claim every future contention failure is prevented. |
| Historical809ms and897ms failures lack stage timestamps | No plan: raw evidence cannot retrospectively supply missing intervals; attribution capability and unchanged regression now exist. A new failure with correlated source/run evidence requires a new finite investigation; these old causes remain unknown. |
| Early mock fixture failures and stale build-script provenance refusal | Closed in this PR: corrected fixture/provenance and exact rebuilt-source checks; all failed records retained under repair1. |
| Private collector reversed-refresh acceptance, positive entry offsets and broad environment capture | Closed in private diagnostic tooling: three reviewed repairs,12 offline controls; original bytes retained, separate sanitized views used. No product-source or acceptance relaxation. |
| Root closing ledger parser rejected a non-object command result | Closed offline: type-aware reconciliation preserves all raw ledgers;340 paired attempt records, all ten new profiles attributed. No native rerun. |
| Final own closing sweep | Closed:348pass/0fail, zero profile leaks/unattributed profiles, all baseline profiles retained and owned raw browser waited. |
| Earlier272 frame-parity mismatch | Fold: iteration283-frame-parity-observation-windows in272's preserved checkout; outside279 and not explained by timing repair. |

Plans246/264 retain their original timing meaning and assertions; no acceptance
rewrite is needed. Iteration272 must integrate this reviewed change and perform
its own closing validation. No279 sweep is substituted for272's required proof.
