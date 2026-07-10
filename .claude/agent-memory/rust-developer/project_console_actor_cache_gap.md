---
name: project-console-actor-cache-gap
description: commands::console::run never calls WebConsoleActor::start_listeners before get_cached_messages, so a fresh --no-daemon connection sees no console output logged by an earlier command — known product gap, filed iter-114
metadata:
  type: project
---

`crates/ff-rdp-cli/src/commands/console.rs::run` calls
`WebConsoleActor::get_cached_messages` without first calling
`start_listeners`. Per Firefox's WebConsole actor protocol,
`getCachedMessages` only returns messages recorded *since listeners were
started* — so any fresh `--no-daemon` connection's `console` command
legitimately returns nothing for output logged by an earlier `eval` in a
different process invocation. This is not a test flake; it is a real gap.

Confirmed live (iter-114, Theme B) by applying a temporary, uncommitted
patch that added `start_listeners` before the cached-messages call — it
fixed the exact repro sequence (`eval console.log(...)` then `console
--pattern ...` in a separate invocation). No existing CLI path currently
primes this cache without blocking forever — `--follow` uses an unrelated
Watcher resource subscription that doesn't touch this cache.

**Why:** `live_console_printf.rs::live_console_printf_e2e` was left
`#[ignore]`-gated and red rather than "fixed" during iter-114's live-suite
port, per that iteration's explicit rule: test-side ports must not touch
product code. See commit `24c193d` on branch
`iter-114/live-suite-debt-zero`-derived worktree for the diagnosis writeup
in the commit body.

**How to apply:** Before attempting to fix `live_console_printf_e2e`, wire
`start_listeners` into `commands::console::run` (or find another way to
prime the cache) as a product change in its own iteration/PR — do not
bundle it into a test-porting PR. Related: [[project_ff_rdp_registry]] for
actor lifecycle patterns in this codebase.
