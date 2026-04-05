---
title: "Iteration 1: Project Scaffolding + Transport"
type: iteration
date: 2026-04-06
tags:
  - iteration
  - scaffolding
  - transport
  - ci
status: completed
branch: iter-1/scaffolding
---

# Iteration 1: Project Scaffolding + Transport

Set up the project structure, CI pipeline, development conventions, and the core TCP transport layer for Firefox RDP communication.

## Tasks

- [x] Initialize workspace Cargo.toml (edition 2024, resolver 3, release profile with LTO)
- [x] Create `crates/ff-rdp-core/` with Cargo.toml (tokio, serde, serde_json, thiserror)
- [x] Create `crates/ff-rdp-cli/` with Cargo.toml (clap, anyhow, jaq-core/json/std)
- [x] Add `deny.toml` (adapted from hyalo: MIT/Apache/BSD/ISC licenses, deny unknown registries)
- [x] Add `.github/workflows/ci.yml` (fmt + clippy + test, 3-OS matrix, pinned actions)
- [x] Add `.github/workflows/release.yml` (build matrix: linux x86_64/aarch64, macos aarch64, windows x86_64 + GitHub Release upload)
- [x] Write project-level `CLAUDE.md` with Rust conventions (adapted from [[../../../CLAUDE.md|hyalo's CLAUDE.md]])
- [x] Create `.claude/agents/rust-developer.md` (adapted from hyalo)
- [x] Create `.claude/agents/rust-release-engineer.md` (adapted from hyalo)
- [x] Create `.claude/settings.local.json` with permission whitelist
- [x] Implement `ff-rdp-core/src/transport.rs` — `RdpTransport` struct: TCP connect, send (length:JSON), recv (parse length prefix, read payload)
- [x] Implement `ff-rdp-core/src/error.rs` — `ProtocolError` enum with thiserror (ConnectionFailed, SendFailed, RecvFailed, InvalidPacket, Timeout, ActorError)
- [x] Implement `ff-rdp-core/src/types.rs` — `ActorId(String)`, `Grip` enum, basic protocol value types
- [x] Implement `ff-rdp-core/src/lib.rs` — re-export public API
- [x] Implement `ff-rdp-cli/src/error.rs` — `AppError` enum (User, Internal, Clap, Exit) with From impls
- [x] Implement `ff-rdp-cli/src/output.rs` — JSON envelope builder, jq filter compilation/execution (from hyalo's jaq integration)
- [x] Implement `ff-rdp-cli/src/output_pipeline.rs` — `OutputPipeline` with `finalize()` method
- [x] Implement `ff-rdp-cli/src/cli/args.rs` — root `Cli` struct with global flags: `--host`, `--port`, `--tab`, `--tab-id`, `--jq`, `--timeout`
- [x] Implement `ff-rdp-cli/src/main.rs` — entry point calling `run()`
- [x] Implement `ff-rdp-cli/src/dispatch.rs` — command routing skeleton (empty match arms for future commands)
- [x] Unit tests for transport framing (mock TCP stream with known packets)
- [x] Verify: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
- [x] Verify: `cargo run -p ff-rdp-cli -- --help` produces clean help output

## Acceptance Criteria

- `cargo build --workspace` succeeds
- `cargo test --workspace` passes all tests
- `cargo clippy --workspace --all-targets -- -D warnings` is clean
- `ff-rdp --help` shows usage with global flags
- CI workflow would pass (fmt, clippy, test on 3 OSes)
- Transport can serialize/deserialize length-prefixed JSON packets
