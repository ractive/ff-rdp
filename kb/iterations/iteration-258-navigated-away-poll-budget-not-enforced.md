---
title: "Iteration 258: navigated_away's poll budget is advisory — one blocked read spends the whole --timeout"
type: iteration
date: 2026-09-07
status: in-progress
branch: iter-258/navigated-away-poll-budget-not-enforced
depends_on: [237]
first_call_sites:
- primitive: RdpTransport::with_read_deadline
  site: crates/ff-rdp-cli/src/commands/type_text.rs::navigated_away
- primitive: RdpTransport::abandon_reply
  site: crates/ff-rdp-core/src/actors/console.rs::WebConsoleActor::evaluate_js_async_scoped
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate https://en.wikipedia.org/wiki/Main_Page
  ff-rdp dom "input[name=search]" --jq '.results[0].ref'
  time ff-rdp type --ref <REF> --text "Turing Award" --submit \
    --jq '{navigated: .results.navigated, method: .results.method}'
  # TODAY (post-iter-237): {"navigated":true,"method":"request_submit"} in ~12.4s wall clock,
  #   against a REQUEST_SUBMIT_NAVIGATION_GRACE_MS of 3s. The answer is correct; the time it
  #   took to reach it is not bounded by any budget the code states.
  # AFTER: the same correct answer, with the poll actually honouring its own deadline.
  ff-rdp daemon stop
tags: [iteration, cli, bugfix, carry-over, latency, transport]
---

# Iteration 258: `navigated_away`'s poll budget is advisory — one blocked read spends the whole `--timeout`

> Filed by [[iteration-237-submit-navigation-grace-period]]'s carry-over sweep, from a measurement
> taken while fixing Part A. Not a regression from 237 — 237 made the *answer* right and left the
> *latency* exactly as it found it.

## Why

`navigated_away` (`crates/ff-rdp-cli/src/commands/type_text.rs`) is written as a poll with a
deadline: a 50 ms sleep between iterations, `if started.elapsed() >= deadline { return false }` at
the bottom. Every budget passed to it — `ENTER_NAVIGATION_GRACE_MS` (600 ms),
`REQUEST_SUBMIT_NAVIGATION_GRACE_MS` (3 s) — reads as a bound on how long that call can take.

None of them is. The deadline is only consulted *between* iterations, and the work inside one
iteration is a `WebConsoleActor::evaluate_js_async` whose read timeout is the CLI's global
`--timeout` (10 s by default; see `crate::error::socket_timeout_ms`). The one case where the
budget matters most is precisely the case where Firefox stops answering: while it commits a new
document it does not reply on the pre-submit console actor at all. So the first iteration blocks
for 10 s, the loop wakes, sees its 3 s deadline long gone, and returns — having completed zero
probes and spent 3.3× the budget it was given.

Measured on 2026-09-07 with iter-237 merged into the working tree:

```
$ time ff-rdp type --ref e1 --text "Turing Award" --submit --jq '{n:.results.navigated,m:.results.method}'
{"n":true,"m":"request_submit"}
type --submit wall clock: 12.37s
```

`navigated: true` is the right answer (that is iter-237's fix, via `navigated_after_refresh`), but
12.4 s for a search-box submit reads as a hang to anything watching the process — the same
complaint that made iter-237 Part B worth doing for `click`.

This also silently defeats the design iter-237 Part A documented at length: the whole point of
giving the post-Enter check 600 ms and the post-`requestSubmit()` check 3 s was that the two are
answering different questions and deserve different budgets. When a page goes quiet, both take
10 s regardless, and the constants are decoration.

## Root cause (confirmed, not assumed)

Two independent deadlines with no relationship between them:

- the poll's own, in `navigated_away`, checked only after a probe returns;
- the transport's, set once per process from `--timeout` and applied to every socket read.

Nothing clamps the second to what is left of the first.

## Themes

- **A — make the poll's deadline bound its own reads.** The transport already carries a read
  timeout; the poll needs the read it is about to issue to expire no later than its own deadline.
  Check whether `RdpTransport` can take a per-call read deadline (or be given one temporarily,
  the way `set_target_guard` is armed and disarmed around a section) before adding a new mechanism.
- **B — do not turn a fast negative into a wrong negative.** Shortening the read must not make a
  slow-but-successful probe report "no navigation" where it previously reported "yes". The
  existing `Ok(_) | Err(ProtocolError::Timeout) => {}` arm already treats a read timeout as "keep
  polling", so a shorter read should mean *more* iterations inside the same budget, not an earlier
  false answer.
- **C — check the other poll loops for the same shape.** `poll_js_condition`,
  `wait_for_predicates` and `autowait_element` all pair a stated budget with reads bounded only by
  `--timeout`. Establish whether they have the same defect before deciding this is one function's
  bug. `autowait_element`'s Part B short-circuit (iter-237) is a partial mitigation for one of
  them, not a fix for the shape.

## Tasks

### A. Bound the read by the poll's deadline [3/3]
- [x] Establish what per-read deadline control `RdpTransport` already exposes; add one only if it
      genuinely has none
- [x] Clamp `navigated_away`'s `evaluate_js_async` read to the remainder of its own deadline
- [x] Unit test with a scripted console that never answers: `navigated_away(.., 600)` must return
      inside ~1 s, not inside `--timeout`

### B. Survey the sibling loops [1/1]
- [x] Determine whether `poll_js_condition` / `wait_for_predicates` / `autowait_element` share the
      defect; fix them here if the mechanism from A applies unchanged, otherwise record which do
      and file separately

### C. Measure [1/1]
- [x] Re-run the `dogfood_path` reproduction before and after; record both wall-clock numbers here

## Acceptance Criteria [3/4]

- [x] `type --submit` against a real navigating search form still reports `navigated: true`, in
      measurably less wall-clock time than the 12.37 s recorded above (record the after number)
- [x] A unit test proves `navigated_away` returns within its stated budget when the console never
      answers
- [ ] `cargo run -p xtask -- live-sweep` clean with both env gates set
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean

## Out of scope

- Lowering the default `--timeout`. It is the right budget for a whole command; the defect is that
  a sub-second sub-step inherits it.
- Re-litigating `navigated_after_refresh` (iter-237). It is what makes the answer correct, and it
  is bounded by `--timeout` on purpose — a load genuinely in flight deserves the command's stated
  budget. Only the *grace-period* poll is claiming a bound it does not enforce.

## References

- [[iteration-237-submit-navigation-grace-period]] — Part A, where the measurement was taken
- `crates/ff-rdp-cli/src/commands/type_text.rs` — `navigated_away`, `navigated_after_refresh`
- `crates/ff-rdp-cli/src/error.rs` — `socket_timeout_ms`, the per-process read timeout

## Implementation preflight — 2026-09-19

Iteration 253 repaired readiness labeling, not polling deadlines. Start with the existing transport read-timeout setter and prior-timeout getter; preserve and restore the actual prior value on every exit. Enforce an absolute operation budget: an idle socket timeout alone does not bound evaluation while push events or multiple reply phases keep arriving. Cover a blocked evaluation, bounded incoming-event traffic and restoration, while preserving reply correlation and refreshed-target navigation detection. Re-measure the historical 12.37-second observation on an owned browser rather than treating it as a current baseline. Relevant code: commands/type_text.rs::navigated_away, commands/js_helpers.rs polling helpers, and core transport timeout APIs.

This source/evidence audit adds implementation guidance, not a new execution result.
Original task and acceptance-criterion wording and checkbox states remain unchanged.

## Implementation and measured evidence — 2026-09-19

`RdpTransport::with_read_deadline` scopes one absolute deadline across every
buffered/socket read, partial frame, unsolicited event and evaluation phase,
then restores the actual prior read timeout on success and error. The scope
uses its remaining budget instead of restarting the socket idle timeout.
Navigation-signal errors and the refreshed-target second opinion are preserved.
An expired scope does not send another probe. A timed-out immediate console
acknowledgement is tracked and its eventual untyped reply discarded, so a later
evaluation cannot accidentally adopt the old result ID. Matching evaluation
events still use their result IDs.

The same mechanism now bounds positive-budget `poll_js_condition` and all
`wait_for_predicates` evaluations, including multiple predicates. The documented
zero-budget condition probe retains its single-evaluation behavior. The survey
confirmed the same defect in `autowait_element`, but its intentionally post-timeout
diagnostic probes and separate stability allowance need a diagnostic policy;
that distinct work is filed in [[iteration-272-autowait-deadline-and-diagnostics]].

Fresh Firefox 156.0, daemon-route Wikipedia measurement on separately owned
browsers, using the freshly built CLI and `dogfood-lib.sh` isolation/teardown:

| Source | Submission wall time | Result |
|---|---:|---|
| Iteration base `a5e2bf8`, before code edits | 11.361 s | `navigated:true`, `method:request_submit` |
| This implementation | 3.800 s | `navigated:true`, `method:request_submit` |

Both runs navigated to Wikipedia Main_Page, registered the search inputs with
`dom 'input[name=search]'`, and submitted `Turing Award` with the equivalent
`--selector 'input[name=search]'` selection. The historical 12.37 s observation
remains historical. This before/after pair improves by 7.561 s (about 67%).
Raw commands, owned launch envelopes and wall times are in
`.git/ralph-loop/20260919-queue/iter258/{dogfood.sh,baseline.log,after.log}`
in the primary checkout; worktree `.git` is a pointer, not that artifact directory.

Non-live coverage includes a 600 ms silent console bound, continuous push events,
an acknowledgement/result pair that together exceed the budget, successful
navigation despite a shorter prior idle timeout, actor teardown, finite/infinite
timeout restoration, a slowly trickling partial frame that resumes correctly,
late-ack/result-ID correlation, and sibling polls sharing a single budget.
The focused tests passed. Removing the navigation deadline makes its elapsed-time
assertion fail; omitting abandoned-ack tracking makes the next evaluation return
the old true result instead of its own false result. Both mutations were restored.
An initial mutation regex matched no formatted call site and is explicitly an
operator error, not useful mutation evidence.

Only `Cargo.lock` was imported from verified main merge
`3a398566281abfa48ce96f1f8d855e6c1116a302` after the supervisor's merge marker;
this takes the independently reviewed rustls advisory patch without stacking
iteration 258 on the iteration 263 implementation.

## Closing sweep and carry-over

The final-source dual-gate sweep on Firefox 156.0 (2026-09-19, 10:32–10:36 UTC)
executed every compiled ignored live test, manually reconciled by exact name
across all five targets: 344 expected and observed, zero duplicates or missing
verdicts. CLI 333 passed / 2 failed; core tiers 1 + 3 + 3 + 2 passed.

```text
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
LIVE_SWEEP_SUMMARY executed=344 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=344
LIVE_SWEEP_PROFILES leaked=0 unattributed=0 root=/Users/james/Library/Application Support/ff-rdp/profiles
```

The runner exited 1. Both failures occur before the affected poll code and match
existing daemon-failure signatures; this is not a clean sweep and the original
clean-sweep AC stays unticked. No isolation rerun or second sweep was used to
replace that evidence. The owned raw browser PID 66490 was stopped and reaped,
its profile removed, and port 6000 verified free. There is one emitted profile
summary, retained above; per-target profile diagnostics also reported no leaks.

| Finding / unmet work | Disposition |
|---|---|
| `live_104_security_pwa::live_manifest_fetch_canonical`: manifest exited 124 with the existing “timeout after auth” envelope; proxy 61100. The diagnostic means the greeting wait after the client sends auth, not proof of server authentication or a manifest-evaluation timeout. | Fold into [[iteration-267-daemon-post-auth-timeout-recurrence]]; this observation does not provide the required attributed handshake timing. |
| `live_145_error_envelope_completeness::live_145_click_element_not_found_unchanged`: readiness failed before click, 15.213 s / 47 polls, target_count 1, live_target_count 0, dispatcher 52 started/finished, no in-flight frame or RPC owner. | Fold into [[iteration-262-daemon-live-target-never-promoted]]; preserve the original promotion failure and unmet green-sweep requirements. |
| Auto-wait reads plus post-timeout diagnostic evaluations are not bounded by the advertised readiness budget. | Filed [[iteration-272-autowait-deadline-and-diagnostics]]; distinct diagnostic policy required. |
| Original clean dual-gate sweep AC. | Unmet; this iteration remains in progress despite the implemented deadline fix. Existing failure owners do not satisfy this AC. |

All raw sweep output, compiled enumeration, name reconciliation and source-hash
verification are retained in the primary checkout's
`.git/ralph-loop/20260919-queue/iter258/` directory.

All nine enumerated xtask checks passed (including plan272, full plan inventory,
actor/KB sync against `a5e2bf8` and source invariants). Firefox-reference and
dogfood-script checks had no referenced fixture/script to execute; the actual
owned-browser dogfood evidence above is separate. Hyalo0.23.0 HYALO005 passed.
The ordered `cargo fmt`, strict workspace clippy and workspace tests passed on
stable1.98.1 / clippy0.1.98 (48a229ceae), ending10:40:03UTC. The stable update
record from this day's iteration263 run was reused as authorized. No source
changed after the closing sweep; SHA-256 verification passed after the gates.
Independent review and supervisor checkpoint/PR actions remain outstanding.
