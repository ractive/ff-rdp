---
title: "Iteration 123: daemon autostart dies on a tabless Firefox + single-slot registry clobbers across ports"
type: iteration
date: 2026-07-18
status: completed
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

- [x] Make `run_daemon` non-fatal on zero tabs: the `tabs.first().context("no tabs available")?`
      bail is gone. Tab resolution now lives in `establish_watcher` (returns `Ok(None)` on zero
      tabs) + `establish_watcher_with_retry` (short bounded retry for the momentary-tab case). On a
      persistently tabless start the registry is still written, the daemon reaches `running:true`,
      and a supervised `watcher-establisher` thread (`background_establish_watcher_loop`) resolves
      the watcher lazily once a tab appears and hands the subscription to the dispatcher.
- [x] Confirmed the daemon tolerates a tabless start (does not require a placeholder tab); noted in
      [[dogfooding-session-61]] and the design notes below that FF152 headless can report `total: 0`
      on the first `listTabs` after `launch` until the first navigation lazily creates the tab.
- [x] Render the `daemon_autostart_failed` warning in `--format text`: `render_warnings` in
      `output_pipeline.rs` now prints each recorded warning to stderr in text output (both plain and
      `--jq` text paths), not only via `--jq '.warnings'`. JSON behaviour and dedup are unchanged.

### B. Per-port registry

- [x] Keyed the registry storage by `firefox_port` in `registry.rs`: files are now
      `daemon.<port>.json` (see `registry_filename`); `read_registry`/`write_registry`/
      `remove_registry` and the `_in` helpers take a `port`, and `write_registry` keys on
      `info.firefox_port`. A write for port A never overwrites port B (`per_port_writes_do_not_clobber`).
- [x] Made the spawn lock per-port: `acquire_spawn_lock(port)` locks `daemon.<port>.spawn.lock`, so
      concurrent autostarts on different ports don't serialize/collide
      (`spawn_lock_is_per_port_and_does_not_cross_block`).
- [x] Updated `daemon stop` / `daemon status` / `daemon_rpc` / doctor / `wait_for_registry` /
      stale-PID pruning to read+remove the correct per-port record (all `registry::*` call sites now
      pass `cli.port` / `firefox_port` / `expected_port`). Stale legacy single-slot `daemon.json` is
      retired on the next `write_registry` (`remove_legacy_registry_in`).
- [x] **Review fix (post-merge-review):** `run_daemon_stop`/`daemon_rpc` initially hardcoded
      `cli.port` even when called from `stop_prior_instance(cli, port)` with a `port` resolved from
      `--debug-port` (which can differ from `--port`) — so `launch --replace --debug-port N` with
      `--port != N` could silently act on the wrong daemon's registry entry. Fixed by threading an
      explicit `port: u16` parameter through both functions instead of implicitly reading `cli.port`.
      See `live_daemon_stop_prior_instance_targets_debug_port_not_cli_port`.

## Acceptance Criteria [5/5]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [x] live_daemon_autostart_tabless: after `launch --headless` on a fresh profile, the first
      autostart-triggering command brings the daemon up — `daemon status.running == true` and the
      command carries no `daemon_autostart_failed` warning — even when Firefox had no page tab at
      daemon start. (live test `live_daemon_autostart_tabless`; unit cores
      `establish_watcher_returns_none_on_zero_tabs`,
      `establish_watcher_with_retry_gives_up_when_no_tab_appears`.)
- [x] live_daemon_two_ports_no_clobber: with daemons autostarted against two Firefox instances on
      distinct ports, `daemon status` for each port reports its own `running:true` daemon (neither
      record is overwritten by the other). (live test `live_daemon_two_ports_no_clobber`; unit cores
      `per_port_writes_do_not_clobber`, `remove_only_affects_the_named_port`,
      `spawn_lock_is_per_port_and_does_not_cross_block`.)
- [x] live_daemon_warning_text_parity: when autostart genuinely fails, the `daemon_autostart_failed`
      signal is visible in `--format text` output, not only via `--jq '.warnings'`. The rendering
      contract (a present `warnings` array is never text-invisible) is pinned deterministically by
      the unit cores `render_warnings_handles_array_and_none` and
      `render_warnings_emits_line_for_each_entry`; the live test `live_daemon_warning_text_parity`
      asserts JSON↔text warning parity end-to-end. (A forced-failure subprocess trigger was avoided
      as slow/flaky — see the test's rationale doc-comment.)
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.
- [x] live_daemon_stop_prior_instance_targets_debug_port_not_cli_port: `launch --replace
      --debug-port N` with `--port != N` stops the daemon registered under `N`, not whichever daemon
      happens to be registered under `--port`; the decoy daemon under `--port` survives untouched
      (same PID, still `running:true`) — pins the review-found `stop_prior_instance` port-threading
      fix above. (live test `live_daemon_stop_prior_instance_targets_debug_port_not_cli_port`;
      confirmed to fail pre-fix — the port-still-listening error named the wrong/decoy port — and
      pass post-fix.)

## Design notes

- Prefer lazy per-request tab resolution over opening a placeholder tab — the daemon already
  resolves tabs per request, so the startup `tabs.first()` is an over-strict early validation.
- **FF152 headless tabless observation (as implemented):** a freshly-launched headless Firefox
  (temp profile, `browser.startup.page = 0`) can report `total: 0` on the first `listTabs` after
  `launch`, and only grows a page tab once the first navigation commits. The daemon therefore
  must NOT require a tab at startup. Tab presence is needed only for the daemon's own *resource
  watcher* (network/console buffering), never for proxying client↔Firefox traffic, so a tabless
  start is non-fatal: the registry is written and `running:true` is reached regardless.
- **Watcher establishment (single vs. second connection):** the main connection is split into
  reader/writer threads at startup, so it cannot serve the synchronous request→reply handshake
  (`listTabs`/`getWatcher`/`watchResources`) that `ResourceCommand::subscribe` needs after the
  split. The lazy `watcher-establisher` therefore opens a **dedicated second RDP connection** to
  run the handshake, then pumps that connection's messages into the shared `event_tx` so the single
  dispatcher buffers watcher events exactly as on the startup path. `watcher_actor` became a
  `Mutex<String>` (empty until established); the subscription is handed to the dispatcher over a
  one-shot rendezvous channel.
- Per-port registry files (`daemon.<port>.json`) are simpler and more concurrency-safe than a
  single map file under one lock; `find_running_daemon`/`wait_for_registry` stay keyed on
  `firefox_port` and need no logic change once the path is port-scoped.
- Kept backward-compat cleanup for a stale legacy `daemon.json` (`remove_legacy_registry_in`, fired
  from `write_registry`) so the old single-slot file is retired rather than left to confuse.

## Out of scope

- A user-facing `daemon start` subcommand (only `status`/`stop` exist today via the hidden
  `_daemon` entry) — a nice affordance, but autostart working reliably (Theme A) removes the need;
  file separately if still wanted.
- The `daemon stop` port-still-held escalation (session 60 issue §1) — separate lifecycle bug.

## Notes carried from iter-122

- iter-122's review caught a fast-path that ignored a caller-selected level (a readystate probe
  added for the default `Complete`-ish case fired even when the caller asked for an earlier
  `--wait loading`/`--wait interactive`). No direct analog exists in this plan's scope (autostart
  and registry keying are not level-gated), but if Theme A's lazy tab resolution introduces any
  "resolve early if condition X already holds" fast path, gate it explicitly on the actual request
  parameters (e.g. `--tab`) rather than assuming the fast path is always safe to take.
- No scope change otherwise — iteration 123 is daemon/registry lifecycle work, independent of
  iter-122's navigate/watcher fixes.

## References

- [[dogfooding-session-61]]
- [[iteration-100-daemon-lifecycle-hardening]]
- [[iteration-101-daemon-session-correctness]]
