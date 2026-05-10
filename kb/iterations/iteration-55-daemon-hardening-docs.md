---
title: "Iteration 55: Daemon Hardening, Module Borders & Agent-Friendly Docs"
type: iteration
date: 2026-05-10
status: planned
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

### A. Daemon hardening

#### A1. Random-token auth on daemon TCP listener [0/4]

The daemon TCP listener at `daemon/server.rs:107` accepts any local connection. On multi-user hosts (or against DNS-rebinding from a malicious page in another browser), this lets an unrelated process drive Firefox: read cookies, eval JS, capture screenshots. Single-user-laptop impact is low, but the fix is cheap and the threat model isn't strictly single-user (DNS rebinding from any browser tab; CI runners; devcontainers).

- [ ] On daemon start, generate a 32-byte random token (via `rand::rngs::OsRng`); include it in the registry written to `~/.ff-rdp/daemon.json` (already 0o600).
- [ ] First frame the daemon expects from any new client connection: `{"auth": "<token>"}`. Mismatch → close socket immediately; log to `daemon.log` (rate-limited).
- [ ] Client (`daemon/client.rs`) reads the token from the registry and sends the auth frame before any other request.
- [ ] E2e test: connect to the daemon port without the token, assert immediate close. Connect with the right token, assert normal flow.

#### A2. Daemon log file 0o600 explicitly [0/1]

`process.rs:80` uses `File::create` which defaults to umask-controlled mode. Logs include URLs/JSON values that may contain auth tokens.

- [ ] On Unix, open the log file with `OpenOptions::new().create(true).append(true).mode(0o600).open(...)`. On Windows, default ACL inherits user-only access from the parent dir; no change needed but verify.

#### A3. `tempfile::Builder` for launch profile dir [0/2]

`launch.rs:231` uses `/tmp/ff-rdp-profile-{pid}-{micros}` — predictable. A same-UID hostile process (e.g. compromised npm postinstall) can pre-create `user.js` as a symlink to `~/.bashrc` and ride the `fs::write` to overwrite arbitrary files. The `tempfile` crate is already a dev-dep — promote to runtime dep.

- [ ] Replace with `tempfile::Builder::new().prefix("ff-rdp-profile-").rand_bytes(16).tempdir_in(env::temp_dir())`. Persist the path (don't auto-delete) so Firefox can read it; keep the existing cleanup path.
- [ ] Test: assert path contains 16 random bytes worth of entropy and isn't predictable from `pid` alone.

#### A4. `daemon status` / `daemon stop` subcommands [0/3]

Currently the only daemon control surface is the hidden `_daemon` and `doctor`. Users `pkill ff-rdp` or wait for idle timeout.

- [ ] Add `ff-rdp daemon status` — prints `{running: bool, pid, port, uptime_seconds, connections, firefox_connected}` JSON.
- [ ] Add `ff-rdp daemon stop` — sends a `shutdown` RPC to the running daemon; falls back to SIGTERM if the RPC times out; cleans up `daemon.json`.
- [ ] E2e: spawn daemon, assert `status` shows running, assert `stop` shuts it down cleanly and registry is removed.

#### A5. Fast-fail when Firefox port is dark [0/2]

When Firefox isn't running, `ff-rdp navigate ...` waits 5s for the spawned daemon to come up before falling back to direct-fail. Adds latency to every "Firefox isn't running" error.

- [ ] Before spawning the daemon, do a 100ms TCP probe on `127.0.0.1:6000` (or `--host`/`--port`). If unreachable, skip the daemon-spawn detour and emit the "Firefox isn't running" hint directly.
- [ ] Assert the hint output is unchanged in the happy "Firefox is running" path.

#### A6. Fix `dispatch.rs:32` doc claim [0/1]

The dispatch table claims non-follow `console` "drains buffered console events". Actual code (`commands/console.rs:17`) just calls `getCachedMessages` — there's no daemon buffer involved.

- [ ] Update the comment table to reflect actual behavior. (Optional: add a real daemon-side buffer for non-follow console mirroring the network path — out of scope for this iteration.)

### B. Module-border tightening

#### B1. Encapsulate transport framing [0/3]

`daemon/server.rs:12` imports `encode_frame`/`recv_from` directly from `ff-rdp-core::transport` — a protocol-layer leak into CLI. `RdpTransport::from_parts`/`into_parts` (`transport.rs:105,114`) are `pub` only to support this trick and create an implicit `TcpStream` assumption in the public API.

- [ ] Add `RdpTransport::split(self) -> (FramedReader, FramedWriter)` returning typed framed halves backed by an opaque inner reader/writer.
- [ ] Daemon uses the typed `FramedReader::recv()` / `FramedWriter::send()` methods; remove the raw function imports.
- [ ] Make `RdpTransport::from_parts` and `into_parts` `pub(crate)`.

#### B2. Tighten visibility on internal helpers [0/2]

- [ ] `escape_selector` in `commands/js_helpers.rs:76` → `pub(crate)`.
- [ ] `PortOwner` fields in `port_owner.rs:13-17` → `pub(crate)`.

### C. Agent-friendly documentation

#### C1. Wrap `launch` output in standard envelope [0/2]

`launch` returns a bare `{pid, host, port, ...}` object — every other command uses `{results, total, meta}`. Breaks any agent that uniformly does `--jq '.results'`.

- [ ] Wrap launch output in the standard envelope: `{results: {pid, host, port, profile_path}, total: 1, meta: {...}}`.
- [ ] Update `launch --help` `Output:` block. Update README examples.
- [ ] E2e test fixture updated.

#### C2. Add `Output: {...}` schema to subcommand long_about [0/1]

Agents calling commands blind have to run them once to learn the JSON shape. Schemas are missing on: `click`, `wait`, `storage`, `responsive`, `a11y`, `scroll`, `geometry`, `snapshot`, `styles`, `console`, `tabs`, `reload`, `back`, `forward`, `inspect`, `page-text`, `computed`, `sources`.

- [ ] Add an `Output: {results: ..., meta: ...}` line to each command's `long_about` describing the field shape. Keep terse — one line per top-level field.

#### C3. EXIT CODES section in root `--help` [0/1]

Currently only `doctor --help` documents exit semantics. Agents need stable codes for control flow without parsing stderr.

- [ ] Add a top-level `EXIT CODES` section: `0` ok, `1` runtime error, `2` usage error, `3` connection failure, `124` timeout. Audit and tag the actual exit paths to match.

#### C4. Regenerate README §Usage from clap (or replace with link) [0/2]

README command list and `--no-daemon` wording drift from `clap --help`.

- [ ] Either generate the command table at build time from `clap`'s help output, or replace the README §Usage with `Run \`ff-rdp --help\` for the full command surface.` Lean toward the latter — less to maintain.
- [ ] Align `--no-daemon` description across README and `clap`.

## Acceptance Criteria

- [ ] All tests pass: `cargo fmt` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace -q`.
- [ ] Doctor still passes end-to-end after daemon-auth landed (no auth-related noise on the happy path).
- [ ] An AI agent reading `ff-rdp <subcommand> --help` for any subcommand can determine the JSON output shape without running it.
- [ ] `daemon status` and `daemon stop` work cross-platform (Linux/macOS/Windows).
- [ ] Local-multi-user threat model documented in `kb/decision-log.md` — explicit before/after.

## Design Notes

**Auth token storage.** Putting the token in `daemon.json` is fine because the file is already 0o600 and the parent dir is 0o700 — anyone who can read the registry already has full home-dir access. The point of the token is to defeat *processes that can `connect()` to localhost but can't read $HOME* (DNS-rebinding from a browser tab; sandboxed apps).

**Why not Unix-domain sockets / named pipes instead?** Cleaner long-term but adds platform branching, breaks the "same wire format everywhere" property, and requires more invasive changes to the transport. Token auth is ~50 LOC; switching to UDS/named pipes is closer to ~200 LOC plus migration. Defer the UDS option to a later iteration if the multi-user scenario becomes a real customer ask.

**`daemon stop` graceful path.** The daemon should accept a `shutdown` control frame on its RPC socket (not the Firefox-facing TCP), tear down its Firefox connection cleanly (`releaseActor` for any held grips — depends on [[iterations/iteration-54-protocol-correctness]] task 4), then exit. SIGTERM fallback for a 2s grace period.

## References

- [[iterations/iteration-14-security-code-review]] — prior security baseline
- [[iterations/iteration-40-daemon-simplification]] — daemon architecture
- [[iterations/iteration-54-protocol-correctness]] — companion protocol-layer work
