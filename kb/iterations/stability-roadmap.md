---
title: "Stability Roadmap (iter-61m → iter-61y) — derived from kb/rdp/ wiki + 3-agent architectural review"
type: roadmap
date: 2026-05-24
status: in-progress
tags: [roadmap, stability, architecture, iter-61m, iter-61n, iter-61o, iter-61p, iter-61q, iter-61r, iter-61s, iter-61t, iter-61u, iter-61v, iter-61w, iter-61x, iter-61y]
---

# Stability Roadmap (iter-61m → iter-61w)

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
| [[iteration-61t-wire-the-foundations\|61t]] | Wire the foundations (Registry, ResourceCommand bus, ScopedGrip, resources-destroyed) | wiring | scaffolding-without-callers found in 61m..61s review | 61p, 61q, 61r, 61s |
| [[iteration-61u-spec-and-front-correctness\|61u]] | Spec & Front correctness (oneway, longstring, renames, missing watcher methods) | correctness | spec drift + missing `get*Actor` Fronts | 61s, 61t |
| [[iteration-61v-navigate-and-screenshot-completion\|61v]] | navigate document-event gating + screenshot fallback drop + bus throttle zero | completion | navigate-race-timeout, screenshot-headless-chrome-scope, race on short navigates | 61r, 61t |
| [[iteration-61w-security-hardening-and-cleanup\|61w]] | Security hardening (auth, refstore, terminal escapes) + bulk-packet skip + kb refresh | hardening + docs | post-61v security audit + stale kb pages | 61t |

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

## Post-mortem: what the 61m..61s arc deferred and why

The original roadmap framed 61m..61s as a clean foundation-then-features
progression.  In practice the seven iterations landed the *abstractions*
(Registry, ResourceCommand bus, ScopedGrip, multi-actor Command, typed spec)
but deferred most of the *wiring* into production paths.  An architectural
review between 61s and 61t catalogued the gap.  The corrective iterations
61t..61w cleaned it up:

- **iter-61t** wired the Registry into the daemon dispatcher, migrated
  `daemon/buffer.rs` onto the `ResourceCommand` bus, wrapped eval grips in
  `ScopedGrip`, and dispatched `resources-destroyed-array`.  Without this,
  61p/61q/61r were unit-passing but not load-bearing.
- **iter-61u** fixed spec correctness: `oneway` methods (`clearResources`,
  `unwatchResources`, `unwatchTargets`) no longer expect replies; the
  long-string fetch path matches Firefox's IDL; renames brought spec method
  names back in line; and the five missing `get*Actor` Fronts on
  `WatcherActor` were added (still primitive — see
  [[../rdp/from-our-codebase/wired-vs-primitive#watcher-getactor-methods-iter-61u--primitive|wired-vs-primitive]]).
- **iter-61v** closed the last two stubborn open-gaps from the dogfooding
  sessions: `--full-page` (proven by `live_screenshot_full_page_dpr2`) and the
  navigate timeout race (document-event gating + bus throttle = 0).
- **iter-61w** is a hardening + docs refresh on top: constant-time token
  comparison, bounded `RefStore`, terminal-escape sanitisation, typed
  bulk-packet rejection, and this very roadmap update.
- **iter-61x** ("honest commits") closes the claim/code gap the post-61v
  review exposed: `DescriptorFront::getProcess(0)` for genuine chrome-context
  eval, the typed `RdpError::Navigation{cause}` enum that 61v PR-claimed
  but didn't write, the `dom-interactive` arm that lets `--wait interactive`
  work, the DPR=2 live screenshot test on its third attempt, fleshed-out
  longstring + cache-disable live tests, deletion of the dead
  `wait_for_commit` polling helper, `Arc<Resource>` bus fan-out, and the
  five test-coverage carry-overs from 61w.
- **iter-61y** ("iteration discipline tooling") converts the postmortem
  mitigations from prose into mechanism: `cargo xtask` with
  `check-dead-primitives`, `check-todo-annotations`, and
  `check-iteration-plan`; a `.githooks/pre-commit` hook for TODO
  annotation; a CI `discipline` job; the `_template.md` iteration plan
  with `first_call_sites` + `dogfood_path` frontmatter; and CLAUDE.md +
  CONTRIBUTING.md updates. Themes D and E — the ralph-loop skill
  "Claims vs code" PR diff and the AC fidelity merge gate — are
  deferred to [[iteration-61z-discipline-skill-integration]] because
  they edit `~/.claude/skills/`, which a cmux child workspace can't
  touch.

The single recurring lesson from the arc: shipping a primitive is not the
same as shipping the user-visible behaviour change it was supposed to
enable.  The iter-61t review caught the gap; the convention going forward is
that an iteration plan's ACs must each name a live test that exercises the
production path, not the primitive in isolation.

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
