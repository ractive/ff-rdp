---
title: "Stability Roadmap (iter-61m → iter-61s) — derived from kb/rdp/ wiki + 3-agent architectural review"
type: roadmap
date: 2026-05-23
status: planned
tags: [roadmap, stability, architecture, iter-61m, iter-61n, iter-61o, iter-61p, iter-61q, iter-61r, iter-61s]
---

# Stability Roadmap (iter-61m → iter-61s)

After sessions 48–53 and iterations 61g–61l, the recurring instabilities (full-page screenshot, CSP-blocked eval, watcher fallthrough, consoleActor staleness, navigate races) are all symptoms of **one architectural gap**: ff-rdp speaks RDP without the central abstractions Firefox's own DevTools client maintains — typed protocol IDL, registry-managed actor Fronts, resource-subscription bus, multi-actor command coordination, and live-by-default testing.

This roadmap stages the fixes. Each item is a self-contained iteration with concrete ACs and live tests. Dependencies are minimal except where called out — iter-61o (test infrastructure) should land before 61p/61q/61r to keep the refactors honest.

## At a glance

| # | Title | Theme | Closes | Depends on |
|---|---|---|---|---|
| [[iteration-61m-wire-tracing-and-structured-errors\|61m]] | Wire-level tracing + structured errors | foundation — dev velocity | unit/live debugging gap | — |
| [[iteration-61n-daemon-quick-fixes\|61n]] | Daemon quick-fixes: watchTargets + double-boundary + mpsc isolation | tactical bug-cluster | sessions 51–53 AC-C + N2 | 61m |
| [[iteration-61o-live-verify-by-default\|61o]] | Live-verify-by-default test architecture + mock watcher push events | test infrastructure | unit-pass/live-fail pattern (iter-61k/61l) | 61m |
| [[iteration-61p-actor-registry-and-front-lifecycle\|61p]] | Actor registry + Front lifecycle + invalidation | the missing abstraction | consoleActor staleness, all "stale actor" classes | 61o |
| [[iteration-61q-resource-command-bus\|61q]] | ResourceCommand-style watcher bus + full WatcherActor engagement | event delivery | --with-network gaps in all forms | 61p |
| [[iteration-61r-multi-actor-commands\|61r]] | Multi-actor Command abstraction: screenshot, eval, navigate, inspector | command coordination | --full-page (5 sessions), CSP-eval, inspector flakiness | 61p, 61q |
| [[iteration-61s-typed-protocol-ides\|61s]] | Typed protocol layer (spec-file → Rust types) | hardening | "shape errors invisible to compiler" | 61p (registry must exist first) |

## Source documents

- [[ff-rdp-architecture-review]] — overall ff-rdp architecture audit
- [[firefox-devtools-patterns-for-ff-rdp]] — patterns to adopt from Firefox's own DevTools client
- [[ff-rdp-daemon-review]] — daemon-specific deep dive
- [[ff-rdp-wins]] — 10 actionable findings from the kb/rdp/ wiki build
- [[dogfooding-session-53]] — the latest dogfood report (sessions 48–52 cited within)

## Why this order

- **61m first.** Until we can `RUST_LOG=ff_rdp::wire=trace` and see every packet in/out, every refactor below is debugged by squinting at fixtures. Tracing also unblocks the next dogfooding session — instead of guessing, we see what Firefox actually sent.
- **61n second.** Three sub-20-LOC daemon fixes that *each* close a regression cluster. Validates the test infrastructure work in 61o by giving it real bugs to catch.
- **61o third.** Forces the unit-pass/live-fail anti-pattern (iter-61k AC-A/B/C/F/G/H/K, iter-61l C/D/N1/N2 deferrals) to stop. Every later iteration's ACs are gated on a live test that drives real Firefox.
- **61p before 61q & 61r.** The actor registry IS the central abstraction; both ResourceCommand (61q) and multi-actor Commands (61r) want a Front-style handle to talk to actors. Building those on raw `String` actor IDs would just re-cement the current pain.
- **61q before 61r.** The screenshot/eval flows in 61r need stable event delivery for things like "wait for `document-complete` resource" to gate navigate.
- **61s last.** A typed protocol layer is the longest-lever win for long-term stability but has the highest churn cost. Land it on top of a stable runtime, not on shifting sand.

## Out of scope for this roadmap

- BiDi support — separate spec, different scope. Once 61p–61s land, a BiDi backend slots in as a parallel transport.
- `--full-page` stripe-stitching as a fallback path — the wiki's `drawSnapshot(rect, ratio, bg, fullpage=true)` finding is the real fix; we don't need the workaround anymore.
- A library/embedded mode (the "kill the daemon" stance from [[ff-rdp-daemon-review]]). The daemon agent recommended evolve-toward-DevToolsClient (Option B); 61n–61q implement that.
- BPF/dtrace instrumentation. tracing-via-RUST_LOG is enough for now.

## Definition of done (roadmap-level)

When all seven iterations have merged:

1. `--full-page` works on a 22k-px page (5-session regression closed).
2. `eval 'document.title'` works on HN, GitHub, banks (CSP no longer blocks).
3. `network` (no flags) returns watcher data with headers when daemon is engaged; no silent fallback.
4. consoleActor invalidates automatically on target switch; bad-DNS navigate followed by `eval` works.
5. `navigate <bad-DNS>` returns structured `error_type` (no false-success).
6. Mock-server tests can simulate watcher push events; no AC can be checked off without a live test that names the asserted post-condition.
7. Adding a new actor takes one spec module + one Front registration, not a hand-rolled `send → parse Value` pair.

## References to feed into each iteration plan

When kicking off any of these iterations, the implementer should re-read:

- The relevant `kb/rdp/` wiki page(s) (cited in the per-iteration plan)
- The corresponding section of [[ff-rdp-architecture-review]]
- [[firefox-devtools-patterns-for-ff-rdp]] section matching the iteration's theme
- For 61n/61o/61q: [[ff-rdp-daemon-review]] in full
