---
title: "Iteration 284: Daemon cancellation and joined shutdown"
type: iteration
date: 2026-09-25
status: planned
branch: iter-284/daemon-cancellation-and-joined-shutdown
depends_on: []
first_call_sites: []
dogfood_path: >-
  Use a finite deterministic actual-thread schedule for cancellation, blocked I/O
  and joined shutdown, with normal-service controls and exact owned joins. Run the
  required ordered gates and this iteration’s own dual-gate closing sweep; do not
  substitute process death or passive capture completeness for worker return.
tags:
  - iteration
  - daemon
  - lifecycle
---

# Iteration 284: Daemon cancellation and joined shutdown

## Problem and boundary

The September25 protocol audit identifies a current lifecycle contract gap:
`run_daemon` discards worker handles, detaches client handlers and can return
after setting shutdown without joining them. Retaining handles alone is
insufficient: socket reads, bounded channels, continuous input, writer locks
and queued RPC claimants can prevent those workers from reaching an exit.

This prerequisite to [[iteration-268-daemon-pre-auth-connection-loss]] delivers
cancellable ownership and actual joined shutdown. It is a separate product
repair under the owner’s all-open adaptation request. It does not explain
historical 224/240 EOF/reset failures or complete268. No archived diagnostic
source is imported wholesale and no passive collector is added merely to
observe threads that the product still leaves detached.

## Tasks [0/4]

- [ ] Define and implement private ownership of every acquired reader,
      dispatcher, release drainer, client handler and optional lazy watcher,
      with one idempotent stop reason and admission stopped once shutdown begins.
- [ ] Make each blocking path cancellation-aware: socket/auth reads and writes,
      channel receive/send/backpressure, writer-lock contention and queued RPC
      claims. Wake or interrupt owned I/O without first acquiring a blocked
      writer lock; preserve normal traffic and 262’s startup-recovery reply sink.
- [ ] Join all acquired workers before daemon return, including partial startup
      failure and panic paths. Record actual join outcomes and propagate failure
      honestly. Resolve optional lazy-watcher loss policy explicitly: its current
      continued-main-service promise and shutdown-on-any-return supervision must
      agree. Do not strand an acquired handle when startup or a worker fails.
- [ ] Exercise a finite deterministic schedule through the actual product thread
      lifecycle, with repair-sensitive regressions and normal-service controls;
      obtain independent review, ordered gates and this iteration’s own required
      dual-gate closing sweep.

## Acceptance Criteria [0/4]

- [ ] Stop admission and idempotent cancellation have a defined observable reason;
      controlled stop during authentication, blocked/full channel operations,
      queued RPC claims and occupied writers wakes all affected owned threads.
- [ ] Every acquired worker/handler/lazy-watcher handle has an actual join outcome
      before daemon return, including partial startup, panic and optional-watcher
      loss. Successful return requires the declared successful shutdown contract;
      process death, an exit marker or missing output never substitutes for join.
- [ ] Meaningful before/after controls exercise the real thread paths and preserve
      normal request/event ordering, 262 reply-sink behavior and the selected
      lazy-watcher policy; no arbitrary sleep, broad timeout increase or normal
      traffic loss is used to make shutdown finish.
- [ ] Independent review and ordered fmt/clippy/workspace gates pass, followed by
      this iteration’s own dual-gate closing sweep with exact-name/profile
      reconciliation and explicit dispositions for other failures.268’s original
      tasks/AC 0/4 and separate 224/240 validation remain unchanged.

## Validation and scope limits

Use a declared finite schedule with synchronization at actual blocking points,
owned socket/peer endpoints and retained actual joins; include an ordinary
traffic control and partial-acquisition failure. The schedule should establish
cancellation mechanics directly, without a new native termination collector,
unbounded pass streak, artificial load campaign or extra discovery sweep.
A watchdog terminating a stuck process is a failed control, never successful
worker completion. Test cleanup must release its own controlled blockers.

Keep the implementation private unless a real non-test consumer requires an
interface. Do not redesign general protocol attribution, grip ownership or
public transport APIs. 259 owns request/reply handover and 266 owns grip lifetime;
serialize overlapping daemon edits but do not invent a dependency on their
unresolved historical investigations. After 284 merges,268 separately assesses
its required scoped attribution and224/240 validation, preserving consumed
capture claims and every missing historical outcome.

## Provenance

The bounded September25 protocol reconciliation reused current-main source
observations and retained268 evidence. This plan creates a prospective product
contract, not a finding that detached workers caused any historical auth loss.
Independent scope review is required before implementation. All tasks and
criteria remain pending.
