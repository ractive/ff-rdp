---
title: "Iteration 107: Windows CI pre-existing reds — install-skill HOME redirection + reload-idle timing"
type: iteration
date: 2026-07-09
status: planned
branch: iter-107/windows-ci-preexisting-reds
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
tags: [iteration, ci, windows, install-skill, nav-action, preexisting-red]
---

# Iteration 107: Windows CI pre-existing reds

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

- [ ] Find the skills-install-directory resolution call (likely
      `dirs::home_dir()` or similar in the `install-skill` command path) and
      confirm it does not honor `HOME` on Windows.
- [ ] Either (a) make the resolution explicitly check `HOME` first on all
      platforms (matching what the tests already assume), or (b) switch the
      *tests* to override the platform-correct variable
      (`USERPROFILE` on Windows) via a small `test_home_env_var()` helper —
      pick whichever matches how a real end user overrides their home
      directory today; document the choice.
- [ ] Serialize or fully isolate the `install_skill` tests so state from one
      test's (possibly-real) install location cannot leak into another
      (`--test-threads=1` already used in the fixture harness — verify this
      is set for this specific binary on Windows, or add a mutex/guard).

### B. `nav_action::reload_wait_idle_no_traffic_returns_idle_quickly` timing

Distinct failure mode (empty stderr, "expected success") — likely a
Windows-specific timing race in the idle-detection window rather than the
environment-variable bug in Theme A. Needs its own repro.

- [ ] Reproduce locally in a Windows VM or CI-equivalent small-stack/slow-IO
      environment; capture the actual exit code and stdout (the CI log only
      shows the assertion, not the command's own JSON error envelope).
- [ ] Root-cause: likely the idle-timeout window is too tight for
      Windows' slower process/socket teardown, mirroring the kind of
      platform-timing gap iter-99 found in the stack-size case.

## Acceptance Criteria [0/2]

- [ ] `install_skill::*` (all 4): pass on windows-latest CI with the
      isolated `home_tmp` actually used (no leakage into the real profile).
- [ ] `nav_action::reload_wait_idle_no_traffic_returns_idle_quickly`: passes
      on windows-latest CI.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

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
