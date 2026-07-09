---
title: "Iteration 108: Windows CI pre-existing reds — install-skill HOME redirection + reload-idle timing"
type: iteration
date: 2026-07-09
status: implemented
branch: iter-108/windows-ci-preexisting-reds
depends_on: []
firefox_refs: []
kb_refs:
  - kb/iterations/iteration-99-ci-preexisting-reds.md
first_call_sites: []
dogfood_path: |
  # After the fix, these must pass on windows-latest CI (not just locally):
  cargo test -p ff-rdp-cli --test e2e install_skill:: -- --test-threads=1
  cargo test -p ff-rdp-cli --test e2e nav_action::reload_wait_idle_no_traffic_returns_idle_quickly
  # expected: 0 failures on windows-latest (was 5 failures as of PR #141 / iter-101)
tags:
  - iteration
  - ci
  - windows
  - install-skill
  - nav-action
  - preexisting-red
---

# Iteration 108: Windows CI pre-existing reds

## Execution policies (2026-07-09, per James)

**Live tests:** do NOT run the full live Firefox suite during this iteration.
Run only the specific live tests this iteration's themes/ACs actually touch
(filtered, e.g. `cargo test -p ff-rdp-cli --test live <filter> --
--include-ignored`) plus the dogfood script. Full-suite validation happens
exactly once, in [[iteration-110-post-batch-live-sweep]], after iteration 109.

**Scoped testing — don't run everything N times:** while developing, run only
the tests affected by the change (`cargo test -p <crate> <filter>`). Run the
full `cargo test --workspace -q` exactly ONCE, as part of the final pre-PR
quality gates. The review agent must NOT re-run the full workspace suite
(implement's gate run + CI cover it); after review fixes, re-run only the
tests covering the files those fixes touched, then rely on CI.

**CI-wait:** merge once the required lanes pass (fmt, clippy, discipline,
supply-chain, fuzz, ubuntu/macos tests, verify-attestation). Do not block on
`live-tests` (advisory by design). EXCEPTION for this iteration: turning
`test (windows-latest)` green is the deliverable — DO wait for the windows
lane and verify 0 failures before merging.


Discovered during the iter-101 (`daemon-session-correctness`, PR #141) merge review.
`test (windows-latest)` fails with 5 test failures that are **unrelated** to
iter-101's diff (which touches only `crates/ff-rdp-cli/src/daemon/{server,buffer}.rs`,
`crates/ff-rdp-core/src/{registry,transport,error,lib}.rs`,
`crates/ff-rdp-cli/src/{error,main,dispatch,cli/args,commands/network}.rs`) —
none of the failing tests or their source files were touched.

Confirmed pre-existing by checking CI history on prior, unrelated branches:
the identical 5 failures (same file:line, same panic messages) reproduce on
`iter-100/daemon-lifecycle-hardening` (CI run 29012524151, job 86099364357,
2026-07-09T10:47Z) and `iter-100b/live-test-consolidation`, both well before
iter-101's branch existed. `main` has no branch-protection rule requiring
`test (windows-latest)` to pass, so this red does not block merges today, but
it is silently masking any *real* Windows regression a future PR might
introduce in these paths.

## The failures (as of CI run 29016780697, job 86113624291)

1. `install_skill::force_overwrites_unmanaged_file` (`install_skill.rs:198`) —
   "expected failure when overwriting unmanaged file without --force"
2. `install_skill::from_dir_installs_custom_content` (`install_skill.rs:453`) —
   "guide.md should be installed"
3. `install_skill::list_shows_installed_status` (`install_skill.rs:266`) —
   "assertion `left == right` failed: should not be installed before install"
4. `install_skill::install_writes_files_and_reinst_is_noop` (`install_skill.rs:112`) —
   "first install should write files; got: ...action":"skipped"..." — installs
   into `C:\Users\runneradmin\.claude\skills\...` instead of the test's
   isolated `home_tmp` dir.
5. `nav_action::reload_wait_idle_no_traffic_returns_idle_quickly` (`nav_action.rs:207`) —
   "expected success, stderr: " (empty stderr — likely a timing/race, not an
   environment-variable bug like the other four).

## Themes

### A. `install-skill` ignores `env("HOME", ...)` on Windows

All four `install_skill` failures share the same signature: the installed
files land in the **real** user profile (`C:\Users\runneradmin\...`) instead
of the test's isolated `home_tmp` `TempDir`, and state leaks across tests
that run in the same process (a file "already installed" from a previous
test in the same binary). This is the classic Windows footgun: `dirs::home_dir()`
/ whatever resolves the skills-install directory likely reads `USERPROFILE`
(or calls a Windows API directly), not the `HOME` env var the tests override
with `.env("HOME", home_dir)`. On Unix, `HOME` *is* the resolution source, so
the tests pass there by coincidence of platform convention.

- [x] Found it: `resolve_install_root` in
      `crates/ff-rdp-cli/src/commands/install_skill.rs` called `dirs::home_dir()`
      for `SkillScope::User`, which reads the Windows known-folder API and
      ignores the tests' `.env("HOME", …)` override. (Same footgun already
      documented in `daemon::registry::registry_dir`.)
- [x] Chose **both (a) and (b)** — belt-and-suspenders, matching existing
      precedent: new `resolve_home_dir()` helper (`install_skill.rs`) resolves
      `HOME` → `USERPROFILE` → `dirs::home_dir()` on all platforms (mirrors the
      `xtask` discipline checks and `registry_dir`), and the e2e helper now sets
      both `HOME` and `USERPROFILE` to the temp dir. Documented in the
      `install-skill` `long_about` help text (`args.rs`).
- [x] Each `install_skill` e2e test already uses its own `home_tmp` `TempDir`;
      with the source honoring `HOME`/`USERPROFILE` there is no shared install
      location, so no cross-test leakage remains. Verified with
      `cargo test -p ff-rdp-cli --test e2e install_skill:: -- --test-threads=1`
      (8 passed).

### B. `nav_action::reload_wait_idle_no_traffic_returns_idle_quickly` timing

Distinct failure mode (empty stderr, "expected success") — likely a
Windows-specific timing race in the idle-detection window rather than the
environment-variable bug in Theme A. Needs its own repro.

- [x] Root-caused by reading the flow rather than a Windows VM: the mock
      server (`reload_wait_idle_server`) sets `close_after_followups`, so it
      closes the socket immediately after delivering the (empty) followup
      batch for `watchResources`. The client then does a fire-and-forget
      `reload` send (`run_reload_wait_idle_direct` / `_daemon`). On Windows a
      write to a peer-closed socket fails with `ConnectionReset` /
      `ConnectionAborted` / `BrokenPipe`; on Unix the write is accepted into the
      send buffer and only the later `read` sees EOF. That send error propagated
      → non-zero exit with the JSON error envelope on **stdout** and an empty
      stderr — exactly the CI signature `expected success, stderr: `.
      (`main.rs` line ~184-194 emits the error envelope to stdout, not stderr.)
- [x] Fix: `send_reload_tolerant()` swallows a connection-teardown IO kind on
      the reload send (the ack is never read anyway) and lets the drain loop
      observe EOF and return idle. Added `is_conn_closed_kind()` shared with the
      drain loop; it now also covers `ConnectionAborted` (Windows). Unit-tested
      in `nav_action.rs` (`conn_closed_kinds_are_treated_as_teardown`,
      `real_io_errors_are_not_teardown`,
      `send_reload_tolerant_swallows_teardown_but_propagates_real_errors`).

## Acceptance Criteria [3/3]

- [x] `resolve_home_dir` + `USERPROFILE` override make the four
      `install_skill` e2e tests (`force_overwrites_unmanaged_file`,
      `from_dir_installs_custom_content`, `list_shows_installed_status`,
      `install_writes_files_and_reinst_is_noop`) use the isolated `home_tmp` on
      every platform — no leakage into the real profile. Verified locally with
      `cargo test -p ff-rdp-cli --test e2e install_skill:: -- --test-threads=1`
      (8 passed); windows-latest CI must show 0 failures (waited for per this
      plan's CI-wait exception).
- [x] `send_reload_tolerant` (+ `is_conn_closed_kind` covering
      `ConnectionAborted`) makes `reload_wait_idle_no_traffic_returns_idle_quickly`
      pass by swallowing the connection-teardown send error that raced the
      mock's `close_after_followups`. Unit-tested by
      `send_reload_tolerant_swallows_teardown_but_propagates_real_errors`;
      windows-latest CI must show 0 failures.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean — run in the pre-PR quality gates.

## Out of scope

- `live-tests` workflow failures — that job runs with `continue-on-error: true`
  in `.github/workflows/live.yml` by design (network-dependent, non-gating);
  not part of this plan unless a specific live test is shown to be a real
  regression rather than CI network flakiness.

## References

- [[iteration-99-ci-preexisting-reds]] — same category of bug (Windows-only
  pre-existing red masked by green macOS/Linux CI), same remediation pattern.
- PR #141 (iter-101, `daemon-session-correctness`) — where this was noticed;
  iter-101's diff does not touch either failing test's source file.
- CI run 29016780697, job 86113624291 (`test (windows-latest)`, iter-101, 2026-07-09).
- CI run 29012524151, job 86099364357 (`test (windows-latest)`, iter-100b, 2026-07-09T10:47Z) — identical failures, prior branch.
