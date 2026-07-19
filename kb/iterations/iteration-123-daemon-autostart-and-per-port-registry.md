---
title: "Iteration 123: daemon autostart dies on a tabless Firefox + single-slot registry clobbers across ports"
type: iteration
date: 2026-07-18
status: planned
branch: iter-123/daemon-autostart-and-per-port-registry
depends_on: []
firefox_refs: []
kb_refs:
  - kb/iterations/iteration-100-daemon-lifecycle-hardening.md
  - kb/iterations/iteration-101-daemon-session-correctness.md
first_call_sites: []
dogfood_path: |
  # Theme A — autostart must survive a freshly-launched Firefox that has no page tab yet:
  ff-rdp launch --headless --port <p>
  ff-rdp --port <p> navigate https://example.com     # first autostart-triggering command
  ff-rdp --port <p> daemon status --jq '.results.running'
  # expected: true (daemon came up) and NO daemon_autostart_failed warning on the navigate

  # Theme B — two concurrent instances on different ports must not clobber each other:
  ff-rdp launch --headless --port <p1>; ff-rdp launch --headless --port <p2>
  ff-rdp --port <p1> navigate https://example.com
  ff-rdp --port <p2> navigate https://example.com
  ff-rdp --port <p1> daemon status --jq '.results.running'   # expected: true
  ff-rdp --port <p2> daemon status --jq '.results.running'   # expected: true (p1's record not overwritten)
tags:
  - iteration
  - daemon
  - registry
  - lifecycle
  - firefox-152
  - dogfood-61
---

# Iteration 123: daemon autostart dies on a tabless Firefox + single-slot registry

Discovered in [[dogfooding-session-61]] (ff-rdp v0.3.0 / Firefox 152). Two daemon defects, both
in the client/daemon lifecycle code (not RDP spec methods):

**A. The daemon autostart never succeeds when Firefox has no open tab.** `run_daemon`
(`crates/ff-rdp-cli/src/daemon/server.rs:307-451`) validates a tab at startup:

```rust
let tabs = RootActor::list_tabs(&mut transport).context("listing tabs")?;
let tab_actor = tabs.first().context("no tabs available")?.actor.clone();  // server.rs:322
```

A freshly-launched headless Firefox (temp profile) can come up with **zero page tabs** — in
session 61 the first `tabs` after `launch` returned `total: 0`, and `~/.ff-rdp/daemon.log` ended
with `{"error":"no tabs available"}`. So the daemon dies **before** the registry write
(`registry::write_registry`, `server.rs:351-359`). The client's `wait_for_registry`
(`process.rs:427-452`) then times out after 5s, `classify_registry_wait_failure`
(`client.rs:398-425`) reads no matching registry entry and reports **"spawn died before the
registry write"**, and every command falls back to a per-command direct connection with a
`daemon_autostart_failed` warning. Net effect: **the persistent daemon never runs in this
environment** (`daemon status` → `running:false`), silently disabling `inspect` (per-connection
grips) and cross-command `--follow`. Once a tab exists (after the first `navigate`) autostart
would succeed, but by then the spawn-lock backoff has fired and commands keep using the direct path.

**B. The registry is a single global slot, so concurrent instances on different ports clobber
each other.** `DaemonInfo` is written to one file `~/.ff-rdp/daemon.json`
(`registry.rs:28-44`, `write_registry_in` `registry.rs:88-136`), not keyed by Firefox port. With
instances on 6000/6001/6002, the last daemon to register wins; commands for the other ports see
`registry targets localhost:X but expected localhost:Y` (`process.rs:436-441`) and connect
directly, and the reported port flip-flops between runs. `find_running_daemon`
(`client.rs:218-242`) and `wait_for_registry` already validate `firefox_port`, so they mostly work
once storage is per-port — the fix is in the registry key/path, not the lookups.

Theme A is the impactful one (affects every single-instance user in this environment); Theme B is
the lower-priority multi-port correctness fix that also removes the session-61 "silent degradation"
confound.

## Themes

- **A — Autostart survives a tabless Firefox.** Make daemon startup tolerate zero tabs: either
  ensure/open a tab at startup, or defer tab resolution to per-request handling (the daemon already
  targets tabs per request via `--tab`), so the startup no longer fatally requires `tabs.first()`.
  The registry must be written and the daemon must reach `running:true`.
- **B — Registry keyed by Firefox port.** Store one daemon record per `firefox_port` (e.g.
  `~/.ff-rdp/daemon.<port>.json`, or a map) and make the spawn lock per-port, so concurrent
  instances don't overwrite each other.

## Tasks

### A. Autostart survives a tabless Firefox

- [ ] Make `run_daemon` (`server.rs:307-451`) non-fatal on zero tabs: don't bail at
      `tabs.first().context("no tabs available")` — resolve the tab lazily on first command, or
      open a `about:blank` tab, then write the registry so the daemon reaches `running:true`.
- [ ] Confirm `launch --headless` leaves a usable tab (or that the daemon tolerates its absence);
      note in `kb/rdp/*` whether FF152 headless starts tabless.
- [ ] Render the `daemon_autostart_failed` warning consistently: it currently only appears via
      `--jq '.warnings'`, not in `--format text` (`daemon_status.rs:44-63`). Surface it (or suppress
      it) in text output too, and keep the existing dedup.

### B. Per-port registry

- [ ] Key the registry storage by `firefox_port` (path or map) in `registry.rs` (`DaemonInfo`,
      `read_registry_in`/`write_registry_in`) so a write for port A never overwrites port B.
- [ ] Make the spawn lock per-port (`acquire_spawn_lock`, `registry.rs:237-259`) so concurrent
      autostarts on different ports don't serialize/collide.
- [ ] Update `daemon stop` / `daemon status` and orphan-pruning to operate on the correct
      per-port record.

## Acceptance Criteria [0/4]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [ ] live_daemon_autostart_tabless: after `launch --headless` on a fresh profile, the first
      autostart-triggering command brings the daemon up — `daemon status.running == true` and the
      command carries no `daemon_autostart_failed` warning — even when Firefox had no page tab at
      daemon start.
- [ ] live_daemon_two_ports_no_clobber: with daemons autostarted against two Firefox instances on
      distinct ports, `daemon status` for each port reports its own `running:true` daemon (neither
      record is overwritten by the other).
- [ ] live_daemon_warning_text_parity: when autostart genuinely fails, the `daemon_autostart_failed`
      signal is visible in `--format text` output, not only via `--jq '.warnings'`.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- Prefer lazy per-request tab resolution over opening a placeholder tab — the daemon already
  resolves tabs per request, so the startup `tabs.first()` is an over-strict early validation.
- Per-port registry files (`daemon.<port>.json`) are simpler and more concurrency-safe than a
  single map file under one lock; `find_running_daemon`/`wait_for_registry` stay keyed on
  `firefox_port` and need no logic change once the path is port-scoped.
- Out of caution, keep backward-compat cleanup for a stale legacy `daemon.json` (migrate/remove).

## Out of scope

- A user-facing `daemon start` subcommand (only `status`/`stop` exist today via the hidden
  `_daemon` entry) — a nice affordance, but autostart working reliably (Theme A) removes the need;
  file separately if still wanted.
- The `daemon stop` port-still-held escalation (session 60 issue §1) — separate lifecycle bug.

## References

- [[dogfooding-session-61]]
- [[iteration-100-daemon-lifecycle-hardening]]
- [[iteration-101-daemon-session-correctness]]
