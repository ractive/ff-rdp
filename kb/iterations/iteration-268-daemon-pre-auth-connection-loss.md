---
title: "Iteration 268: diagnose recurrent daemon pre-auth connection loss"
type: iteration
date: 2026-09-13
status: planned
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
