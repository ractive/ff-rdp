---
title: "Iteration 69: Reply-vs-event correctness + shared reply-loop helpers"
type: iteration
date: 2026-05-24
status: planned
branch: iter-69/reply-vs-event
depends_on:
  - iteration-61p-actor-registry-and-front-lifecycle
  - iteration-61q-resource-command-bus
first_call_sites:
  - primitive: "ff_rdp_core::transport::recv_reply_from"
    site: "crates/ff-rdp-core/src/actor.rs::actor_request (replaces the inline loop)"
  - primitive: "ff_rdp_core::transport::recv_event_from"
    site: "crates/ff-rdp-core/src/actors/console.rs::evaluate_js_async + parse_eval_result"
dogfood_path: |
  # 1. Trigger an interleaved console event during an unrelated actor call.
  # The reply-loop must NOT pick the consoleAPICall packet as the reply.
  ff-rdp eval 'console.log("noise"); setTimeout(() => console.log("late"), 100); 42'
  # Expected: result == 42 (not 'undefined' or an Err from misclassified event).

  # 2. ThreadActor.attach (the case the old comment cited as justification for the relaxed match).
  # Verify the new code routes 'paused' as an event and 'attach' reply as a reply.
  ff-rdp debug attach   # (or equivalent)
  # Expected: clean attach, 'paused' delivered via the event channel.
tags: [iteration, protocol]
---

# Iteration 69: Reply-vs-event correctness + shared reply-loop helpers

`actor_request` (`crates/ff-rdp-core/src/actor.rs:62-68`) matches replies by
`from == to` alone and explicitly ignores `type`. The kb
(`kb/rdp/protocol/message-format.md:70-91`) and Firefox's `Front.js`
both state replies have **no `type`** — any `from`+`type` packet is an
event. The comment justifying the relaxation cites ThreadActor.attach's
`paused` response, but `paused` is in fact an event, not the reply. With a
WebConsoleActor that has `startListeners` active, any in-flight
`consoleAPICall` will be misread as the reply to the next request on that
actor. Fix `actor_request`; extract the open-coded loops in
`actors/console.rs:138-157, 172-198` into shared helpers so future actors
inherit the correct rule.

## Themes

- **A — Tighten reply matching.** `actor_request` requires
  `from == to && msg.get("type").is_none()`. Inline events get re-queued or
  routed to the event-bus subscriber.
- **B — Shared helpers.** Extract `recv_reply_from(transport, actor)` and
  `recv_event_from(transport, actor, predicate)` in `transport.rs`. Collapse
  the three open-coded loops onto them.
- **C — Error-code mapping.** Add `MissingParameter`, `BadParameterType`,
  `NotImplemented`, `WrongOrder`, `ProtocolError`, `UnknownError` to
  `ActorErrorKind` and mark `MissingParameter`/`BadParameterType` as
  terminal in `is_transient`.

## Tasks

### A. Tighten reply matching
- [ ] Change `actor.rs:62-68` to require `msg.get("type").is_none()`.
- [ ] When a packet arrives that has the right `from` but is an event, route it through the existing event bus (`resources/command.rs` dispatcher) instead of dropping it.
- [ ] Update the comment to cite the correct rule and link to `kb/rdp/protocol/message-format.md`.

### B. Shared helpers
- [ ] Add `transport.rs::recv_reply_from(actor: &str) -> Result<Value>`.
- [ ] Add `transport.rs::recv_event_from(actor: &str, mut predicate: impl FnMut(&Value) -> bool) -> Result<Value>`.
- [ ] Migrate `actor_request` to `recv_reply_from`.
- [ ] Migrate `actors/console.rs:138-157` (evaluate_js_async reply path) and `172-198` (evaluationResult event wait) to the helpers.
- [ ] Audit other actor modules for similar open-coded loops and migrate.

### C. Error-code mapping
- [ ] Extend `ActorErrorKind` in `crates/ff-rdp-core/src/error.rs:122-133` with the five new variants.
- [ ] Update `is_transient` so `MissingParameter` and `BadParameterType` return `false` explicitly (currently true only by accident of falling into `Other`).
- [ ] Add unit tests covering each new variant's parse path.

## Acceptance Criteria [0/6]

- [ ] `actor_request_routes_event_correctly`: feeding `[{from:A,type:"consoleAPICall",…}, {from:A,result:42}]` to a fake transport returns the second packet as the reply; the first is routed to the event channel.
- [ ] `actor_request_rejects_typed_packet_as_reply`: a packet with `from == to && type == "paused"` is NOT picked as the reply.
- [ ] `recv_reply_from_helper_extracted`: `actor_request` is a 3-line wrapper around `recv_reply_from`.
- [ ] `console_evaluate_js_async_uses_helpers`: open-coded loops at `actors/console.rs:138-157, 172-198` are replaced by `recv_reply_from` / `recv_event_from` calls.
- [ ] `actor_error_kind_terminal_for_param_errors`: `is_transient(ActorErrorKind::MissingParameter) == false`; same for `BadParameterType`.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

The relaxation in `actor_request` was almost certainly added during early
iteration to unblock a debugging session. The right fix is the strict rule
plus a place for stray events to land — the event bus from iter-61q is
already that place, so routing the misclassified packet there is the
natural completion.

`recv_event_from` taking a predicate (rather than a fixed event-name match)
generalises the `evaluationResult` wait, the `tabNavigated` abort signal,
and `document-event` filtering — all current open-coded loops in disguise.

## Out of scope

- Per-actor FIFO pipelining (Firefox's promise-chain per `from`). The
  current global serialisation is fine for the CLI; pipelining is a
  separate, larger design.
- Migrating to a single error taxonomy (`ProtocolError` vs `RdpError`).
  Documented as transitional; out of scope here.

## References

- [[iteration-61p-actor-registry-and-front-lifecycle]]
- [[iteration-61q-resource-command-bus]]
- Protocol review report (2026-05-24), §2.1, §2.8
- `kb/rdp/protocol/message-format.md`
