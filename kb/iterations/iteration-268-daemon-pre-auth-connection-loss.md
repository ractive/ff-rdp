---
title: "Iteration 268: diagnose recurrent daemon pre-auth connection loss"
type: iteration
date: 2026-09-13
status: in-progress
branch: iter-268/daemon-pre-auth-connection-loss
first_call_sites: []
dogfood_path: |
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_224_with_page_connection_reset::live_repeated_hop_never_loses_the_connection -- --include-ignored --exact --nocapture --test-threads=1
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_240_daemon_frame_desync_and_wedge::live_240_sustained_hops_never_desynchronise -- --include-ignored --exact --nocapture --test-threads=1
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
tags: [iteration, daemon, authentication, testing, carry-over]
---

# Iteration 268: recurrent connection loss during daemon authentication

Filed from [[iteration-255-infobox-facts-refs-and-query-matching]] after the
pre-auth connection-loss watch in [[iteration-203-live-sweep-watch-conditions-third-holder]]
recurred. This is separate from [[iteration-267-daemon-post-auth-timeout-recurrence]]:
that plan owns authenticated request timeouts, whereas both observations below
failed while authenticating. EOF and connection reset remain distinct wire outcomes;
a shared cause has not been established.

## Observations

- The iteration-252 Windows compatibility sweep failed
  `live_240_daemon_frame_desync_and_wedge::live_240_sustained_hops_never_desynchronise`
  at hop 27/40: `User: daemon auth failed: recv failed: failed to fill whole buffer`.
  Exact isolation passed all 40 hops in 36.44 s. The shared daemon log had no
  reliable failed-occurrence timestamp/PID attribution. Plan 203 explicitly required
  capture and a scoped follow-up on recurrence.
- Iteration 255's unpaused dual-gate sweep failed
  `live_224_with_page_connection_reset::live_repeated_hop_never_loses_the_connection`
  at hop 7/12: `User: daemon auth failed: recv failed: Connection reset by peer
  (os error 54)`. Daemon proxy port 58890; live-launch log records Firefox PID
  97083, debug port 58776, epoch 1789322081. No attributable failed-connection
  daemon auth/request/dispatcher timing was captured. The failure precedes page
  collection, and this fixture contains no fact rows; this does not establish a
  cause or prove a particular server deadline.
- Iteration 255's repair sweep (2026-09-13) failed
  `live_240_daemon_frame_desync_and_wedge::live_240_sustained_hops_never_desynchronise`
  at hop 16/40: `User: daemon auth failed: recv failed: Connection reset by peer
  (os error 54)`, with zero reconnects. Proxy port 59843; live-launch log records
  Firefox PID 68682, debug port 59766, epoch 1789324143. The shared daemon log
  contains `listening on port 59843, PID 68787`, but no attributable failed-auth
  timing. This is another pre-auth reset, not post-auth plan 267; a shared cause
  with the original EOF or the 224 reset remains unproved. The 224 repeated-hop
  test passed in this sweep; that does not erase its prior failed occurrence.
  Evidence: `.git/ralph-loop/20260912-validation-efficiency/iter255-repair1/sweep.log`
  and `live-launches.log` in that directory.

Current evidence: `.git/ralph-loop/20260912-validation-efficiency/iter255-implementation/sweep.log`
and the exact isolated log and verdict in that directory. Historical evidence:
`.git/ralph-loop/20260912-validation-efficiency/pr247-windows-abort/sweep.log` and
its `isolated-live_240_daemon_frame_desync_and_wedge.log`.

The exact 255 isolation passed all 12 hops with zero reconnects in 15.76 s on
daemon proxy 63286. It does not reproduce the failure or supply the missing
failed-occurrence timing, and does not close this plan.

The repair's exact 240 isolation passed all 40 hops with zero reconnects in
27.46 s (proxy 63802). The original hop-16 reset is preserved, and the missing
failed-occurrence auth/dispatcher timing remains an unticked requirement.

## Tasks [0/4]

- [ ] Capture attributable client and daemon timing for auth receive, auth reply,
      connection lifecycle and dispatcher/RPC-slot state at a failing occurrence,
      including PID, debug/proxy port and monotonic timestamps
- [ ] Explain each observed pre-auth EOF/reset, distinguishing timeout, intentional
      close, process exit and other transport failures; do not infer a common cause
- [ ] Fix the demonstrated mechanism while preserving auth ownership and request
      attribution; do not hide failures with retries or widened timeouts alone
- [ ] Add a meaningful regression test, exercise both named live scenarios, and
      run the required closing dual-gate sweep

## Acceptance Criteria [0/4]

- [ ] A failing occurrence has attributable client/daemon auth and dispatcher timing
- [ ] The demonstrated cause and EOF/reset relationship are documented with evidence
- [ ] The regression fails before the fix and passes after it without weaker assertions
- [ ] Both named repeated-hop tests pass in the closing dual-gate sweep, with any
      remaining connection-loss signatures explicitly preserved and dispositioned

## Out of scope

- Post-auth request timeouts, owned by iteration 267.
- The target-promotion and distinct Sourcepoint consent-action failures, owned by
  [[iteration-262-daemon-live-target-never-promoted]].
- Raising timeout defaults or claiming an isolated pass discharges a sweep recurrence.

## Implementation preflight — 2026-09-19

Coordinate diagnostic boundaries with267: its 'timeout after auth' message only proves the client sent authentication and then waited for a greeting, not that authentication succeeded. Keep EOF/reset and timeout observations separately attributable, without assuming different root causes. Instrument a connection identity before authentication completes. Existing isolated passes do not explain failed occurrences; obtain bounded failed-occurrence evidence before repair, and avoid timeout increases or blind retries.

This source/evidence audit adds implementation guidance, not a new execution result.
Original task and acceptance-criterion wording and checkbox states remain unchanged.

## Iteration263 recurrence — 2026-09-19

Final263 repair sweep on Firefox156.0 failed
`live_240_sustained_hops_never_desynchronise` at hop20/40, zero reconnects,
proxy58186: `daemon auth failed: recv failed: Connection reset by peer
(os error 54)`. All344names reconcile (342pass/2fail), with no skipped or
reclassified tests and zero profile leaks. Attributable failed-auth timing is
still unavailable; no root cause or shared mechanism is claimed. No isolated
pass replaces this failed occurrence. Original tasks/ACs remain unchanged.
Evidence: `.git/ralph-loop/20260919-queue/iter263-repair2/live-sweep.log`,
`reconciliation.json` and `live-launches.log`.

The final263 security-dependency sweep separately failed the same240 scenario at
hop3/40, proxy55319,0reconnects: `daemon auth failed: recv failed: failed to fill
whole buffer`. Keep this EOF distinct from the preceding hop20 reset; attributable
failed-auth timing remains unavailable and no shared cause is proved.
Full344=341pass/3fail, no missing names or profile leaks. Evidence:
`.git/ralph-loop/20260919-queue/iter263-security/sweep.log`.

## Iteration265 sweep recurrence — 2026-09-19

The final265 sweep failed `live_224_with_page_connection_reset::live_repeated_hop_never_loses_the_connection` on hop9, proxy63178: `daemon auth failed: recv failed: failed to fill whole buffer`. This is another EOF observation, not proof of a common cause with resets or greeting timeouts. Attributable failed-auth timing remains unavailable. Full346=344pass2fail, zero profile leaks. Evidence: `.git/ralph-loop/20260919-queue/iter265/live-sweep.log` and `supervisor-accounting.json`. Original tasks/ACs remain unchanged.

## Attributed267 handshake evidence — 2026-09-19

Iteration267 reproduced the scope-table greeting-wait Timeout envelope with
attributable connection timing: daemon13895/proxy56229 rejected its auth read
about88microseconds after handler entry, before client13984/local56297 completed
its auth send. The client then received reset54; the existing transient-error
mapping presented that reset as "timeout after auth". A separate control
confirmed accepted sockets inherit the listener's nonblocking mode on macOS.
The scoped267 repair restores blocking mode before applying existing deadlines;
its explicit nonblocking-socket regression fails without that repair and passes
with it. See [[iteration-267-daemon-post-auth-timeout-recurrence]] and primary
checkout evidence `.git/ralph-loop/20260919-queue/iter267/trace-v2.log`,
`loaded-modules-v2.log`, and `trace-flags.log`.

This demonstrates why presentation alone cannot separate267 and268 handshake
failures. It does not supply missing failed-occurrence traces for this plan's
historical224/240 EOF/reset observations, and does not close this plan or any of
its original ACs. Preserve every original observation and await the required
attributed evidence before assigning those occurrences the same cause.

## Bounded investigation — 2026-09-19

Investigation baseline: `03ba1fd9e2d74193e99b3ff51d4e732b8e7b5337`, containing
the merged iteration 267 repair. No new product change was justified. Temporary
diagnostics recorded client and server socket endpoints, PID, monotonic auth
milestones, shutdown state, dispatcher counters and a nonblocking snapshot of
whether the RPC slot was empty, occupied or locked. No auth token or request
payload was logged. These snapshots were observation only; this did not investigate
iteration 259's request/reply handover or iteration 262's target lifecycle.

The two named repeated-hop tests passed with the merged repair and diagnostics
intact: 2 passed, 0 failed, 43.17 s, with all 12 and 40 hops completed. Removing
only iteration 267's `set_nonblocking(false)` repair, retaining the same diagnostics
and test assertions, also passed the serial pair: 2 passed, 0 failed, 43.84 s.
One six-worker diagnostic batch then ran those modules alongside the established
160/161/164/165/219 load controls: 36 passed, 1 failed, 62.40 s. Both named
224/240 repeated-hop tests passed again. No EOF or reset in either named scenario
was captured. These were targeted investigations, not a closing live sweep.

The mutant batch's failure was
`live_165_eval_call_scope::live_165_wrap_trigger_is_confined_to_declaring_scripts`,
whose greeting-wait Timeout envelope contained an underlying reset 54. The trace
joins client 1170/local 53521 to daemon 853/proxy 53400: handler entry at monotonic
45366.878300916, auth-read error at 45366.879477916 (about 1.177 ms later), intentional
auth rejection at 45366.879522083, and client reset at 45366.893829250. At rejection,
shutdown was false, the dispatcher was alive with 41 frames started and finished,
and the RPC slot was empty. Client auth-send completion was recorded after
rejection. This is an additional controlled observation consistent with 267's
already-demonstrated mechanism, not a newly attributed 224/240 failure. It does
not explain any historical EOF, nor establish that all historical resets shared
that cause. The removed repair was restored; no retry or timeout bound changed.

Evidence and exact command/env/start/end/exit metadata are retained under
`.git/ralph-loop/20260919-queue/iter268/`: `fixed-pair.log`, `fixed-trace.log`,
`mutant-pair.log`, `mutant-trace.log`, `mutant-loaded.log`,
`mutant-loaded-trace.log`, their launch logs and `.meta` sidecars. The temporary
patches are retained there for a reproducible diagnostic setup; product sources
were restored to the baseline and rebuilt (`restored-build.log`).

**Blocked, not complete:** no attributed failing 224/240 occurrence was obtained,
and the missing historical auth/dispatcher traces cannot be recovered from passing
controls. Every original task and AC remains unticked. A new regression or second
fix would be speculative; 267 already supplies its own failing-before/passing-after
regression. A closing 268 dual-gate sweep has not run. Resume on a new attributable
224/240 failure or a concrete new diagnostic hypothesis, preserving the original
EOF/reset observations and acceptance strength. This investigation adds no new
carry-over plan and requests no completion PR.
