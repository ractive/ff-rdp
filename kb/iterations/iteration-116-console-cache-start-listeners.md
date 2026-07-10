---
title: "Iteration 116: console command primes the cache — start_listeners before get_cached_messages"
type: iteration
date: 2026-07-10
status: planned
branch: iter-116/console-cache-start-listeners
depends_on:
  - kb/iterations/iteration-114-live-suite-debt-zero.md
firefox_refs: []
kb_refs:
  - kb/rdp/actors/console.md
first_call_sites:
  - primitive: WebConsoleActor::start_listeners
    site: >-
      crates/ff-rdp-cli/src/commands/console.rs (called before
      get_cached_messages so a fresh connection's cache is primed)
dogfood_path: |
  ff-rdp --port <p> eval 'console.log("hello %s, you are %d", "world", 42)'
  ff-rdp --port <p> console
  # expected: the printf-formatted message "hello world, you are 42" appears
tags:
  - iteration
  - console
  - rdp
---

# Iteration 116: console command primes the cache

Discovered during [[iteration-114-live-suite-debt-zero]] (Theme B port of
`live_console_printf_e2e`, deliberately left red there): `commands::console::run`
calls `WebConsoleActor::get_cached_messages` without ever calling
`startListeners` first. Per the Firefox WebConsole actor protocol
([[console]] kb note), `getCachedMessages` only returns messages recorded
since listeners were started — so a fresh `--no-daemon` connection's `console`
command legitimately sees nothing that an earlier eval logged. Verified live
in iter-114, including with a temporary product patch adding `start_listeners`
that fixed the exact failing sequence. No existing CLI path primes this cache
(`--follow` uses an unrelated Watcher resource subscription).

## Scope

- Add `WebConsoleActor::start_listeners` to ff-rdp-core (real spec method,
  no drift) and call it in `commands::console::run` before
  `get_cached_messages`.
- Update `kb/rdp/actors/console.md` (check-actor-kb-sync pairs the actor
  change with the kb note).
- Un-red `live_console_printf_e2e` — it is the live coverage for this change.

## Out of scope

- `console --follow` rework (its Watcher subscription path is separate).

## Acceptance Criteria [0/2]

- [ ] live_console_printf_e2e: printf-formatted message
      `hello world, you are 42` returned by `ff-rdp console` on a fresh
      connection after an eval logged it.
- [ ] live_console_no_double_delivery: still green (priming the cache must
      not double-deliver against the follow path).

## Results

(to be filled by the implementing iteration)
