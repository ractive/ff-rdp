---
title: "Iteration 14: Security & Code Review"
type: iteration
date: 2026-04-06
tags:
  - iteration
  - security
  - review
  - hardening
status: completed
branch: iter-14/security-review
---

# Iteration 14: Security & Code Review

Thorough security audit, code quality review, and hardening pass across the entire codebase.

## Background

ff-rdp is a CLI tool that connects to Firefox's Remote Debugging Protocol over TCP and can execute arbitrary JavaScript, read cookies (including httpOnly), capture screenshots, and navigate to URLs. This power makes security important even for a developer tool — a rogue page loaded via `navigate` could potentially exfiltrate data through the RDP connection if not handled carefully.

## Threat Model

### Attack Surfaces

1. **Malicious URL via navigate**: User navigates to attacker-controlled page → page executes JS → ff-rdp reads data from that page (cookies, DOM, eval results) → attacker steals data from other pages? **No** — each tab has its own origin sandbox in Firefox. But the *user's* subsequent commands (`cookies`, `eval`, `dom`) will operate on the attacker's page, so output may contain attacker-controlled data.

2. **Malicious RDP server**: If ff-rdp connects to a rogue server (not Firefox), it could send crafted JSON responses to exploit parsing bugs, trigger buffer overflows in length-prefix parsing, or return unexpected data types.

3. **Plaintext TCP**: No TLS — anyone on the network can intercept all RDP traffic (page content, cookies, eval results). This is a Firefox RDP protocol limitation.

4. **CLI argument injection**: User-supplied values (URLs, selectors, JS expressions) embedded in JSON/JS without proper escaping could allow injection.

5. **Output exfiltration**: ff-rdp outputs sensitive data (cookies, page content) to stdout. In a pipeline, this data flows to the next command.

## Part A: Security Fixes

### A1: URL Scheme Validation (HIGH)

- [x] Add URL validation to `navigate` command — reject `javascript:` and `data:` schemes
- [x] Allow `http:`, `https:`, `file:` (file is useful for local testing), `about:blank`
- [x] Add `--allow-unsafe-urls` escape hatch for power users
- [x] Unit tests for URL validation

**Rationale**: `javascript:` URLs execute code in the *current* page context, which could bypass expected isolation. `data:` URLs can embed arbitrary content.

### A2: Eval Expression Safety in --wait (MEDIUM)

- [x] Audit all places where user input is interpolated into JavaScript strings
- [x] `wait --eval`: already wrapped in IIFE `(function() { return !!(expr); })()` — document that this is intentional (user explicitly provides JS)
- [x] `wait --selector`: uses `js_helpers::escape_js_string()` — verify escaping covers all edge cases
- [x] `wait --text`: uses `js_helpers::escape_js_string()` — verify
- [x] `dom`, `click`, `type`: use `escape_js_string()` — verify
- [x] `storage --key`: uses `serde_json::to_string()` — verify
- [x] `perf --type`: validated against allow-list in iter-12 — verify still correct
- [x] Document in README that `eval` and `wait --eval` execute arbitrary JS by design

**Conclusion from initial audit**: The `--eval` flag is *designed* to run user JS — it's not an injection vulnerability, it's the feature. The concern is only if *other* flags (selector, text, key) could be tricked into JS injection. Current escaping via `serde_json::to_string()` and `escape_js_string()` appears solid.

### A3: Regex Complexity Limit (MEDIUM)

- [x] Add size limit to regex patterns in `--pattern` flags (console, sources)
- [x] Use `regex::RegexBuilder::new().size_limit(1_000_000)` to prevent ReDoS
- [x] Test with pathological patterns to verify limit works

### A4: Daemon Security Review (MEDIUM)

- [x] Audit daemon TCP listener — binds to 127.0.0.1 (loopback only), but any local process can connect and send RDP commands to Firefox
- [x] Review registry file (`~/.ff-rdp/daemon.json`) — writable by the user, could be tampered to redirect CLI to a rogue proxy. Consider file permissions (0600) on Unix.
- [x] Verify daemon auto-cleanup of stale registry entries (PID liveness check)
- [x] Document that the daemon has the same trust model as Firefox DevTools — localhost only

### A5: Screenshot Path Validation (LOW)

- [x] Validate `--output` path doesn't traverse with `..`
- [x] Or: canonicalize path and verify it's under CWD or an absolute path the user explicitly chose
- [x] Consider: this is a CLI tool run by the user — they can write anywhere they have permissions anyway. **Decision**: document behavior, skip validation (user already has shell access).

## Part B: Code Quality Review

### B1: Error Handling Audit

- [x] Grep for any remaining `.unwrap()` or `.expect()` outside of tests
- [x] Verify all protocol errors are properly propagated with context
- [x] Check for any panics that should be Results

### B2: Dependency Audit

- [x] Run `cargo deny check advisories` — verify no known vulnerabilities
- [x] Run `cargo deny check licenses` — verify license compliance
- [x] Review `cargo audit` output
- [x] Check for outdated dependencies with `cargo outdated`
- [x] Verify minimum Rust version compatibility

### B3: Code Consistency

- [x] Run `cargo clippy --workspace --all-targets -- -D warnings` — zero warnings
- [x] Check for dead code, unused imports, unused dependencies
- [x] Verify all public APIs have consistent patterns
- [x] Check that all commands follow the same output envelope structure

### B4: Cross-Platform Verification

- [x] Verify all path handling uses `std::path` (no hardcoded `/` separators)
- [x] Check Firefox launch command works on Linux, macOS, Windows
- [x] Verify TCP connection works on all platforms
- [x] Check that CI runs tests on all three platforms

## Part C: Repository Hygiene

### C1: Secrets Scan

- [x] Scan entire git history for accidentally committed secrets: `git log --all -p | grep -i -E "(password|secret|token|api.key|bearer)" | head -50`
- [x] Verify no personal information (emails, usernames, paths) in committed files
- [x] Check fixture files for real-world sensitive data (cookies with real values, real URLs with auth tokens)
- [x] Verify `.gitignore` covers: `.env`, `target/`, IDE files, OS files

### C2: Documentation Review

- [x] README accuracy — all commands documented, examples work
- [x] CLAUDE.md accuracy — instructions match current codebase
- [x] KB docs — verify no stale references to removed features
- [x] License file present and correct

### C3: CI/CD Review

- [x] Verify CI runs fmt, clippy, test on all platforms
- [x] Check that CI fails on warnings (not just errors)
- [x] Verify no secrets in CI configuration
- [x] Check GitHub Actions workflow for supply chain risks (pinned action versions)

## Part D: Transport Security Documentation

- [x] Add security section to README documenting:
  - Firefox RDP uses plaintext TCP (no TLS) — localhost only
  - ff-rdp can read httpOnly cookies, execute JS, capture screenshots — same power as DevTools
  - Recommended: use SSH tunneling for remote debugging
  - Not designed for untrusted network environments
- [x] Document the `--allow-unsafe-urls` flag rationale

## Acceptance Criteria

- No `javascript:` or `data:` URLs accepted by `navigate` (without escape hatch)
- All user input interpolation into JS is audited and documented
- Regex patterns have complexity limits
- Zero `.unwrap()` / `.expect()` outside tests
- `cargo deny check` passes (advisories + licenses)
- `cargo clippy` zero warnings
- No secrets in git history
- No personal data in fixtures
- README has security section
- All existing tests still pass

## Design Notes

- This is a hardening iteration, not a feature iteration — no new commands or protocols
- Many "fixes" may turn out to be non-issues after investigation (e.g., screenshot path traversal is a user CLI tool — the user already has shell access)
- The threat model is primarily about defense-in-depth, not defending against a malicious user (the user IS the operator)
- Key insight: ff-rdp has the same power as Firefox DevTools. The security model is "same as opening DevTools" — the risks are about accidental misuse, not malicious exploitation
