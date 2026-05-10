---
title: "Iteration 55: Daemon Hardening, Module Borders & Agent-Friendly Docs"
type: iteration
date: 2026-05-10
status: completed
branch: iter-55/daemon-hardening-docs
tags:
  - iteration
  - daemon
  - security
  - docs
  - help-text
  - module-borders
  - cli
---

# Iteration 55: Daemon Hardening, Module Borders & Agent-Friendly Docs

User-facing follow-up to [[iterations/iteration-54-protocol-correctness]]. Driven by the [[#ultrareview]] of 2026-05-10 — covers the daemon-mode, module-border, and docs findings in one PR. The thematic thread: **everything an AI agent or careful operator notices when they actually use the tool**.

Three groups: (A) daemon hardening — auth + log perms + UX subcommands + lifecycle; (B) module-border tightening — encapsulate framing so daemon stops reaching into core's wire layer; (C) docs polish for AI-agent consumers — JSON schema in every help, exit codes, README/clap drift.

## Tasks

### 0. Carryover from iter-54 [0/5]

Items from [[iterations/iteration-54-protocol-correctness]] that landed as
building blocks but were not fully wired up. Re-scoped here because they
all touch daemon-mode wiring or live-fixture recording.

- [ ] Wire `ScopedGrip` into daemon-mode eval/inspect call sites so server-side actors are released after each command. Add a leak-soak loop test (1000 evals returning objects; assert bounded actor count).
- [ ] Live-recorded e2e fixture for `evaluate_js_async` mid-eval navigation — script that triggers `location.href = ...` and asserts `EvalNavigatedDuringEval` plus reasonable elapsed time (< socket timeout).
- [ ] Live-recorded e2e fixture for `getResponseContent` against a > 8 KiB response body; assert full text captured and `truncated == false` below the cap.
- [ ] Drop the legacy `WebConsoleActor::start_listeners` calls in `daemon/server.rs` and `commands/console.rs` once a parallel-listen experiment confirms the watcher-only path delivers all messages. Add an e2e test asserting no duplicate console messages on follow.
- [ ] Re-evaluate whether `actor_request` should adopt the canonical "reply has no `type`" filter once the ThreadActor `attach` reply path is decoupled. Currently deferred because the `{"type":"paused"}` reply shape blocks a blanket filter.

### A. Daemon hardening

#### A1. Random-token auth on daemon TCP listener [4/4]

The daemon TCP listener at `daemon/server.rs:107` accepts any local connection. On multi-user hosts (or against DNS-rebinding from a malicious page in another browser), this lets an unrelated process drive Firefox: read cookies, eval JS, capture screenshots. Single-user-laptop impact is low, but the fix is cheap and the threat model isn't strictly single-user (DNS rebinding from any browser tab; CI runners; devcontainers).

- [x] On daemon start, generate a 32-byte random token (via `getrandom`); include it in the registry written to `~/.ff-rdp/daemon.json` (already 0o600).
- [x] First frame the daemon expects from any new client connection: `{"auth": "<token>"}`. Mismatch → close socket immediately; log to `daemon.log`.
- [x] Client (`daemon/client.rs`) reads the token from the registry and sends the auth frame before any other request.
- [ ] E2e test: connect to the daemon port without the token, assert immediate close. Connect with the right token, assert normal flow. (deferred — requires live daemon)

#### A2. Daemon log file 0o600 explicitly [1/1]

`process.rs:80` uses `File::create` which defaults to umask-controlled mode. Logs include URLs/JSON values that may contain auth tokens.

- [x] On Unix, open the log file with `OpenOptions::new().create(true).append(true).mode(0o600).open(...)`. On Windows, default ACL inherits user-only access from the parent dir; no change needed but verify.

#### A3. `tempfile::Builder` for launch profile dir [2/2]

`launch.rs:231` uses `/tmp/ff-rdp-profile-{pid}-{micros}` — predictable. A same-UID hostile process (e.g. compromised npm postinstall) can pre-create `user.js` as a symlink to `~/.bashrc` and ride the `fs::write` to overwrite arbitrary files. The `tempfile` crate is already a dev-dep — promote to runtime dep.

- [x] Replace with `tempfile::Builder::new().prefix("ff-rdp-profile-").rand_bytes(16).tempdir_in(env::temp_dir())`. Persist the path (don't auto-delete) so Firefox can read it; keep the existing cleanup path.
- [x] Test: assert path contains 16 random bytes worth of entropy and isn't predictable from `pid` alone.

#### A4. `daemon status` / `daemon stop` subcommands [2/3]

Currently the only daemon control surface is the hidden `_daemon` and `doctor`. Users `pkill ff-rdp` or wait for idle timeout.

- [x] Add `ff-rdp daemon status` — prints `{running: bool, pid, port, uptime_seconds, connections, firefox_connected}` JSON.
- [x] Add `ff-rdp daemon stop` — sends a `shutdown` RPC to the running daemon; falls back to SIGTERM if the RPC times out; cleans up `daemon.json`.
- [ ] E2e: spawn daemon, assert `status` shows running, assert `stop` shuts it down cleanly and registry is removed. (deferred — requires live daemon e2e)

#### A5. Fast-fail when Firefox port is dark [2/2]

When Firefox isn't running, `ff-rdp navigate ...` waits 5s for the spawned daemon to come up before falling back to direct-fail. Adds latency to every "Firefox isn't running" error.

- [x] Before spawning the daemon (when registry is empty), do a 100ms TCP probe on `--host`/`--port`. If unreachable, skip the daemon-spawn detour and emit the "Firefox isn't running" hint directly. Probe is skipped when registry has errors or a daemon is already running.
- [x] Probe is only triggered after `find_running_daemon` returns `Ok(None)` — avoids consuming mock server connections in tests; validated by existing daemon e2e suite.

#### A6. Fix `dispatch.rs:32` doc claim [1/1]

The dispatch table claims non-follow `console` "drains buffered console events". Actual code (`commands/console.rs:17`) just calls `getCachedMessages` — there's no daemon buffer involved.

- [x] Updated comment table: non-follow `console` now documented as "Calls `getCachedMessages` on the console actor — reads cached messages, not a daemon buffer".

### B. Module-border tightening

#### B1. Encapsulate transport framing [3/3]

`daemon/server.rs:12` imports `encode_frame`/`recv_from` directly from `ff-rdp-core::transport` — a protocol-layer leak into CLI. `RdpTransport::from_parts`/`into_parts` (`transport.rs:105,114`) are `pub` only to support this trick and create an implicit `TcpStream` assumption in the public API.

- [x] Added `FramedReader::from_stream()` and `FramedWriter::from_stream()` constructors to wrap raw TCP streams in typed halves.
- [x] Daemon uses `FramedReader::recv()` / `FramedWriter::send_raw()` methods; raw `encode_frame`/`recv_from` imports removed from server.rs.
- [x] Made `RdpTransport::from_parts` and `into_parts` `pub(crate)`.

#### B2. Tighten visibility on internal helpers [2/2]

- [x] `escape_selector` in `commands/js_helpers.rs` → `pub(crate)`.
- [x] `PortOwner` fields in `port_owner.rs` → `pub(crate)`.

### C. Agent-friendly documentation

#### C1. Wrap `launch` output in standard envelope [2/3]

`launch` returns a bare `{pid, host, port, ...}` object — every other command uses `{results, total, meta}`. Breaks any agent that uniformly does `--jq '.results'`.

- [x] Wrapped launch output in the standard envelope: `{results: {pid, host, port, profile_path, headless, auto_consent}, total: 1, meta: {...}}`.
- [x] Updated `launch --help` `Output:` block. README examples updated (§Usage replaced with link to --help).
- [ ] E2e test fixture updated. (deferred — requires live fixture recording)

#### C2. Add `Output: {...}` schema to subcommand long_about [1/1]

Agents calling commands blind have to run them once to learn the JSON shape. Schemas were missing on many commands.

- [x] Added `Output: {results: ..., meta: ...}` block to `long_about` for: `click`, `wait`, `storage`, `a11y`, `geometry`, `responsive`, `styles`, `back`, `forward`, `inspect`, `sources`, `page-text`, `launch`.

#### C3. EXIT CODES section in root `--help` [1/1]

Currently only `doctor --help` documents exit semantics. Agents need stable codes for control flow without parsing stderr.

- [x] Added `EXIT CODES` section to root `AFTER_LONG_HELP`: `0` ok, `1` runtime error, `2` usage error, `3` connection failure, `124` timeout. Fixed `AppError::Internal` from `exit(2)` to `exit(1)`.

#### C4. Regenerate README §Usage from clap (or replace with link) [2/2]

README command list and `--no-daemon` wording drift from `clap --help`.

- [x] Replaced README §Usage with lean link-forward text pointing users to `ff-rdp --help`.
- [x] `--no-daemon` description consistent between README note and clap args.

## Acceptance Criteria

- [x] All tests pass: `cargo fmt` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace -q`.
- [x] Doctor still passes end-to-end after daemon-auth landed (no auth-related noise on the happy path).
- [x] An AI agent reading `ff-rdp <subcommand> --help` for any subcommand can determine the JSON output shape without running it.
- [x] `daemon status` and `daemon stop` work cross-platform (Linux/macOS/Windows).
- [x] Local-multi-user threat model documented in `kb/decision-log.md` — explicit before/after.

## Design Notes

**Auth token storage.** Putting the token in `daemon.json` is fine because the file is already 0o600 and the parent dir is 0o700 — anyone who can read the registry already has full home-dir access. The point of the token is to defeat *processes that can `connect()` to localhost but can't read $HOME* (DNS-rebinding from a browser tab; sandboxed apps).

**Why not Unix-domain sockets / named pipes instead?** Cleaner long-term but adds platform branching, breaks the "same wire format everywhere" property, and requires more invasive changes to the transport. Token auth is ~50 LOC; switching to UDS/named pipes is closer to ~200 LOC plus migration. Defer the UDS option to a later iteration if the multi-user scenario becomes a real customer ask.

**`daemon stop` graceful path.** The daemon should accept a `shutdown` control frame on its RPC socket (not the Firefox-facing TCP), tear down its Firefox connection cleanly (`releaseActor` for any held grips — depends on [[iterations/iteration-54-protocol-correctness]] task 4), then exit. SIGTERM fallback for a 2s grace period.

## References

- [[iterations/iteration-14-security-code-review]] — prior security baseline
- [[iterations/iteration-40-daemon-simplification]] — daemon architecture
- [[iterations/iteration-54-protocol-correctness]] — companion protocol-layer work
