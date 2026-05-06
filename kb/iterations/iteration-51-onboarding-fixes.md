---
title: "Iteration 51: Connection Diagnostics & Onboarding"
type: iteration
date: 2026-05-06
status: done
branch: iter-51/connection-diagnostics
tags:
  - iteration
  - dx
  - ai
  - onboarding
  - bugfix
  - launch
  - doctor
  - hints
---

# Iteration 51: Connection Diagnostics & Onboarding

First of three iterations addressing [[../dogfooding/dogfooding-session-40]]. Focus: close the gap between "user runs first command" and "first successful page interaction." Companion iterations: [[iteration-52-input-eval-ergonomics]], [[iteration-53-stability-fixes]].

The dogfooder spent ~10 minutes flailing because `launch` silently no-op'd against a port already held by a 13-day-old daily-driver Firefox, and every error pointed back at "run launch" — exactly what they had already done. This iteration fixes the diagnostic blind spot so the next first-time user (or AI agent) doesn't repeat it.

## Motivation

The single biggest pain point: **port collision is silent**. `launch --temp-profile` returned a healthy-looking JSON envelope with a fresh PID and `port: 6000`, but the new Firefox was a ghost — its `--start-debugger-server 6000` collided with an existing listener and quietly went nowhere. Meanwhile `tabs` happily talked to the *other* Firefox, which had no debuggable tabs because session-restore had unloaded them. Result: empty `[]` from `tabs`, indistinguishable from "your launch failed."

The fix has two halves:

1. **Loud failure on the actual fault** (port collision, stale connection).
2. **Discoverability of the diagnostic tool.** A `doctor` command is useless if no one knows to run it. Promotion is a first-class deliverable, not an afterthought — every error path that *might* be a connection problem must mention `ff-rdp doctor` by name.

## Tasks

### 1. Detect port collisions in `launch` [3/3]

- [x] Before spawning Firefox, check whether `localhost:<port>` already accepts a TCP connection. If yes, identify the listener (`lsof -nP -iTCP:<port> -sTCP:LISTEN` on Unix; `Get-NetTCPConnection` / `netstat -ano -p tcp` on Windows) and abort with: `error: port <N> is already in use by <process> (PID <pid>). hint: pass --port <N+10> to use a different port, run \`ff-rdp doctor\` for a full report, or stop the existing listener.`
- [x] When the pre-launch check is inconclusive (no `lsof`, ambiguous PID), still poll the port for ~3s after spawn. If the port was occupied before spawn but our spawned Firefox didn't bring up its own listener, fail with the same error.
- [x] E2e test: simulate occupied port (start a dummy TCP listener on a free port, attempt `launch --port <that>`, assert the structured error and exit code).

### 2. Add `ff-rdp doctor` subcommand [4/4]

One command to run when stuck. Probes the connection top-to-bottom and reports each failure mode in plain English with a tailored hint.

- [x] New `doctor` subcommand. Probes (in order):
  1. **Daemon status** — registry present? socket reachable? when started?
  2. **Port owner** — who is listening on `--port`? PID, process name, uptime.
  3. **RDP handshake** — can we connect and complete `getRoot`?
  4. **Tab count** — how many tabs are exposed by the connected target? Which are debuggable?
  5. **Firefox version compatibility** — parse from RDP `getRoot` / `applicationType`. Compare against actor minimums (screenshot actor, etc.).
- [x] Output a structured report (JSON envelope + text-mode "checklist" rendering with ✓/✗/⚠ and a one-line hint per failure).
- [x] Exit 0 if all probes pass, 1 if any fail. Make it CI-friendly.
- [x] E2e tests: doctor on a working daemon (all green), doctor with no Firefox running (port owner = none, suggests `launch`), doctor with stale connection (no tabs, surfaces PID/uptime).

### 3. Include connected-target metadata in every response [3/3]

A 13-day-old daily-driver Firefox would have been an instant red flag if `tabs` had reported `connected_pid` and `uptime_s`. Embed this in the JSON envelope `meta` field for all commands that talk to the browser.

- [x] Resolve and cache (per-process / per-daemon-session) the connected target's `pid`, `port`, `profile_path` (from launch arguments where known), and Firefox `version`. Read once at handshake; refresh on connection re-establishment.
- [x] Include `meta.connection = { host, port, connected_pid, uptime_s, firefox_version }` in every command's JSON envelope.
- [x] Update at least one e2e test per command family (browser-touching) to assert the new `meta.connection` fields are populated.

### 4. Improve the canonical "no tabs" error [3/3]

The current message — `error: no tabs available — is a page open in Firefox? Use \`ff-rdp launch --headless --temp-profile\` to start one` — is actively misleading after the user has *already* launched. Branch on root cause and **always mention `doctor`**.

- [x] Distinguish three states and tailor the hint accordingly:
  - **No Firefox connected** (handshake fails / port not listening) → suggest `launch`. Append: *run `ff-rdp doctor` for a full diagnostic*.
  - **Firefox connected but zero tabs** (handshake succeeds, listTabs empty) → report connected PID + uptime; suggest opening a tab manually or relaunching with `--temp-profile`. Append: *run `ff-rdp doctor` to see why this connection has no tabs*.
  - **Firefox connected, tabs exist but none debuggable** (e.g. all unloaded by session-restore) → suggest `--tab N` and report which tab indices were seen but unavailable. Append: *run `ff-rdp doctor` to inspect tab state*.
- [x] E2e test for each branch (assert error text and that "doctor" is mentioned).
- [x] Same treatment for the related `actor error from server1.conn*/...` family of errors when they fire on a fresh launch — append doctor hint.

### 5. Promote `doctor` so it actually gets used [4/4]

A diagnostic only works if people know it exists. Five surfaces to hit:

- [x] **Contextual hint after `launch`** (via the iter-50 hints system): include `-> ff-rdp doctor  # Verify the connection is healthy` as the first hint, before `tabs`. This means the very first thing a new agent sees post-launch is the diagnostic command.
- [x] **Error hint integration** (covered in task 4 above): every "no tabs," "actor error," "port in use," and connection-timeout error appends a `hint: run \`ff-rdp doctor\` ...` line.
- [x] **`--help` troubleshooting section**: add a top-level "Troubleshooting" block to `ff-rdp --help` listing the three most common failure modes, each pointing to `doctor`.
- [x] **README first-contact section** for AI agents: a short "If anything goes wrong, run `ff-rdp doctor` first — it pinpoints connection, port, and version issues in one shot."

### 6. Make hints worth reading on the failure path [3/3]

Iter-50 added contextual hints to *successful* command output. The dogfooding session showed the *failure* path is where hints matter most. Audit and harden them.

- [x] Audit every error message in `ff-rdp-cli` for an actionable hint. If the error is a known failure mode (port, no-tabs, actor, daemon, version-mismatch), the message must end with a `hint: ...` line that names the next concrete command (not generic advice).
- [x] When an error is suspected to be a connection problem, the *primary* hint must be `ff-rdp doctor` — the user should not have to read a wall of text to find it.
- [x] E2e test asserting that each known-failure-mode error includes a `hint:` line.

## Acceptance Criteria

- [x] `launch` fails loudly with PID + hint when the target port is already in use.
- [x] `ff-rdp doctor` reports green on a healthy daemon and pinpoints the failing layer otherwise.
- [x] Every browser-touching command's JSON envelope includes `meta.connection` with `connected_pid`, `uptime_s`, `firefox_version`.
- [x] "no tabs" error branches by root cause and always mentions `ff-rdp doctor`.
- [x] `launch` success output's first hint is `ff-rdp doctor`.
- [x] `ff-rdp --help` has a "Troubleshooting" block that surfaces `doctor`.
- [x] README has a "First contact" section aimed at AI agents.
- [x] Every known-failure-mode error message ends with a `hint:` line; connection-related ones name `doctor` first.
- [x] All quality gates pass: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace -q`.
