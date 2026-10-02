# Contributing to ff-rdp

ff-rdp is an LLM agent's eyes on a page: navigate, then inspect text, DOM, styles, console,
network, screenshots, accessibility and performance over Firefox's Remote Debugging Protocol.
`CLAUDE.md` holds the short rules; this file holds the detail.

## Quality gates

Run these **in order**, once, after your last code change, and fix all issues:

```sh
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace -q
```

Never commit code that fails any of these. There is no need to repeat them in a later step
(opening or reviewing the PR) if the tree has not changed since the last green run.

### A local pass is not a CI pass

CI's `fmt`/`clippy` jobs use a SHA-pinned `dtolnay/rust-toolchain` on the `stable` channel —
the *action* is pinned, the *toolchain* is not, so it resolves to whatever stable is current on
the day the job runs. When a new stable lands, clippy gains lints and fails in CI on code you did
not touch.

- Run `rustup update stable` before treating a green local clippy as evidence about CI.
- Read `gh pr checks <PR>`. CI is the authority on green.
- Lint and build failures mask each other — re-check after each fix.

`.github/workflows/toolchain-watch.yml` runs `fmt` + `clippy` against `main` weekly for the case
no PR can catch: a stable release that red-lines `main` with zero commits pushed
(`kb/decision-log.md` DEC-044 explains why the repo does not pin `rust-toolchain.toml`).

## Test layout

Integration tests for `ff-rdp-cli` are organized into a few consolidated targets rather than one
binary per file — every extra top-level `tests/*.rs` file is a separate binary to compile and
link.

- **Mock-server e2e tests** go under `crates/ff-rdp-cli/tests/e2e/` as modules of
  `tests/e2e/main.rs` (the `e2e` target).
- **Live-Firefox tests** go in `crates/ff-rdp-cli/tests/live/<slug>.rs` plus a `mod` line in
  `tests/live/main.rs` (the single `live` target). A new top-level `tests/live_*.rs` file fails
  `cargo run -p xtask -- check-live-test-layout` (CI `discipline` job).
- **Every `#[test]` under `tests/live/` carries `#[ignore]`** naming its env gate, e.g.
  `#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]`. A plain
  `cargo test` stays Firefox-free and fast. A Firefox-free probe that must run by default carries
  an `// allow-ungated-live: <reason>` comment above the `#[test]` instead.
- Shared helpers live in `crates/ff-rdp-cli/tests/common/mod.rs`, declared from
  `tests/live/main.rs` via `#[path = "../common/mod.rs"] mod common;` (`live_tests_enabled`,
  `live_network_tests_enabled`, `live_sites_tests_enabled`, `LiveFirefox`, …).
- Launch and kill waits in the harness are bounded and env-overridable
  (`FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS`, `FF_RDP_TEST_DAEMON_READY_TIMEOUT_S`,
  `FF_RDP_TEST_KILL_WAIT_TIMEOUT_MS`). A live test that cannot launch Firefox fails; it does not
  silently return.
- A live test that asserts a global property of the machine (e.g. "no managed Firefox is
  running") isolates itself with its own `$FF_RDP_HOME`, because the sweep runs tests in parallel.

## Live tests

| Gate | Meaning |
|---|---|
| `FF_RDP_LIVE_TESTS=1` | launches headless Firefox locally |
| `FF_RDP_LIVE_NETWORK_TESTS=1` | also makes real network requests |
| `FF_RDP_LIVE_SITES_TESTS=1` | also drives third-party sites (BBC, Guardian, HN, Wikipedia, MDN, …) |

**Per PR**, run the live tests for the modules you touched:

```sh
FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live <module> -- --include-ignored
cargo test -p ff-rdp-cli --test live -- --list   # enumerate live test names, no Firefox needed
```

A test whose env gate is unset returns early and libtest still reports it `ok`. Set every gate
the module's `#[ignore]` reasons name (`FF_RDP_LIVE_NETWORK_TESTS=1`, …) before treating the
result as a pass; `live-sweep` below reports unmet gates as `ignored` instead.

**The full sweep is not a per-PR gate.** `.github/workflows/live.yml` runs it nightly on `main`,
on `workflow_dispatch` and at release; a weekly `sites` job adds `FF_RDP_LIVE_SITES_TESTS=1`.
To run it by hand:

```sh
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep [--dry-run]
```

`live-sweep` classifies each live test from its `#[ignore]` reason, runs only those whose env
gates are set (so an unmet gate reports `ignored`, not a fake `ok`), runs self-launching targets
in parallel (`--jobs`, default min(6, cores)) and ends with a `LIVE_SWEEP_SUMMARY executed=N
skipped=M …` line. `ff-rdp-core` live tests need a Firefox someone else started on port 6000
(`firefox -no-remote --start-debugger-server 6000 --headless`); without one they are counted as
`preexisting`, not run.

**Flakes.** A live test that fails and then passes on one re-run is quarantined with
`#[ignore = "flaky: issue #N"]` and a GitHub issue. It does not get an iteration plan or an
attribution investigation. A red nightly gets one GitHub issue.

Third-party-site tests are never gating: those pages change and go down on their own schedule.
A file that navigates to a third-party host must gate on `common::live_sites_tests_enabled()`;
`tests/iter_242_third_party_dependencies.rs` checks that, and that every such host is declared.

## Test fixtures

All e2e fixtures (`tests/fixtures/*.json`) are recorded from a real Firefox — never hand-crafted:

```sh
firefox -no-remote -profile /tmp/ff-rdp-test-profile --start-debugger-server 6000 --headless
FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored
```

Actor IDs are normalized (`conn\d+` → `conn0`) and written to both `ff-rdp-core/tests/fixtures/`
and `ff-rdp-cli/tests/fixtures/`. Add a fixture with a live test in
`crates/ff-rdp-core/tests/live_record_fixtures.rs` using `save_cli_fixture()`/`save_core_fixture()`.

## xtask checks

List what exists with `cargo run -q -p xtask -- --help`; do not invent names.

| Command | Where it runs | What it checks |
|---|---|---|
| `check-iteration-plan <plan or dir>` | CI `discipline` | `status` vocabulary; no two plans share a number (`dogfood_path`/`first_call_sites` are optional warnings) |
| `check-live-test-layout` | CI `discipline` | no top-level `tests/live_*.rs`; every live `#[test]` is `#[ignore]`-gated |
| `check-source-invariants` | CI `discipline` | no `.lock().unwrap()` in the daemon; no `eprintln!` + `AppError::Exit(N)` bypass; every `eprintln!` under `commands/` has `// stderr-ok: <reason>` |
| `check-skill-drift` / `gen-skill` | CI `discipline` | the generated region of `skills/ff-rdp-debug/SKILL.md` matches the CLI's tables |
| `check-help-idioms` | CI `discipline` | `ff-rdp --help` still carries the quick-start idioms |
| `check-vendored-js` | CI `discipline` | vendored Readability bundle matches the hashes in its `VERSION` |
| `check-firefox-refs <plan>` | local | a plan's `firefox_refs` line ranges exist in `$FF_RDP_FIREFOX_PATH` |
| `check-actor-kb-sync --since origin/main` | local | a changed `actors/<X>.rs` has its `kb/rdp/actors/<X>.md` updated (or `// allow-actor-kb-skip:`) |
| `find-iteration-plan --branch <b>` | local | resolves `iter-N/slug` to its plan file |
| `live-sweep` | nightly CI, local | see [Live tests](#live-tests) |

Run the CI ones once before opening a PR; CI runs them again.

Validate the whole plan directory:

```sh
cargo run -p xtask -- check-iteration-plan kb/iterations
# expected: check-iteration-plan: swept N plan(s) in kb/iterations: 0 failed, M with warnings only
```

The two historical number collisions (44 and 73) are exempt by exact file-name pair.

After editing plan frontmatter, `hyalo lint --rule HYALO005` confirms hyalo can still parse every
plan (hyalo skips unparseable documents silently otherwise). It is local-only: hyalo is not
installed on the runners.

## Review rules

- **Every new `pub` item needs a non-test consumer in the same PR.**
- **Every `TODO`/`FIXME`/`XXX` needs a GitHub issue link, a `WORD-123` ticket, or
  `// allow-todo: <reason>`.**
- **Spec drift** — a field or method ff-rdp sends that the published Firefox spec does not
  declare — needs `// allow-spec-drift: bug NNNN` on the call site.
- Commit-message claims must be backed by the diff.

These were once xtask gates; `kb/discipline-rationale.md` records why they are review rules now.

### rdp-spec-reviewer agent

A `rdp-spec-reviewer` subagent lives at `~/.claude/agents/rdp-spec-reviewer.md` (mirrored from
`tools/agents/rdp-spec-reviewer.md`; edit both). For a PR touching actor files it produces a
`## Spec drift` section for the PR body:

```sh
claude --agent rdp-spec-reviewer --input tools/agents/fixtures/synthetic-watcher-diff.patch
```

## Iteration plans

Plans live in `kb/iterations/iteration-NN-slug.md`; start from `kb/iterations/_template.md`.
Fill in `title`, `date`, `branch` (`iter-NN/short-description`) and `status`
(`planned | in-progress | in-review | done | obsolete`). Acceptance criteria describe
user-visible behaviour. Never reword a criterion to match what happened — leave it unticked and
say why. Plan ticks and `status: done` are committed together with the code, so the PR runs CI
once.

## PR discipline

- One iteration = one branch = one PR; branch `iter-N/short-description`.
- Self-review the diff before requesting review — fmt, clippy, dead code.
- One review pass; no re-review for style-only fixes.
- Merge on GitHub with `gh pr merge <n> --merge --delete-branch`; never a local merge + push,
  never `--squash`.

## Supply-chain checks

`cargo audit` (RustSec advisory DB) and `cargo deny check` (advisories, licences, bans, sources)
run on every PR via the `supply-chain` job in `.github/workflows/ci.yml`.

When a new advisory breaks CI, choose one path:

1. **Upgrade (preferred).** `cargo update -p <crate>` to a patched version, commit `Cargo.lock`.
2. **Pin a working version** with `<crate> = "=X.Y.Z"` in `Cargo.toml` and link the upstream issue.
3. **Ignore with reason.** If the advisory does not apply (a `dev-dependency`, a code path never
   invoked), add its ID to `[advisories].ignore` in `deny.toml` with a
   `# advisory ID — short justification, link` comment. Never ignore without a written reason.

Licence or ban regressions: prefer removing the dependency; widen the allow-list only if the
licence is genuinely compatible.

## Fuzzing

Parser fuzz harnesses live in `fuzz/` (`transport_recv_from`, `parse_page_map_str`,
`parse_script_file`) and run 60 s each on every PR via the `fuzz` job.

```sh
rustup install nightly
cargo install cargo-fuzz
cd fuzz
cargo +nightly fuzz run transport_recv_from seeds/transport_recv_from -- -max_total_time=60
```

When CI reports a crash: download the minimised input from the job artifacts, reproduce with
`cargo +nightly fuzz run <target> <input>`, open a GitHub issue tagged `fuzz-finding`, fix the
parser, then check the input into `fuzz/seeds/<target>/` as a regression seed. See
`fuzz/README.md`.

## Branch protection

If protection is (re)configured, pick required contexts from the jobs in
`.github/workflows/ci.yml` that run on `pull_request` — `fmt`, `clippy`, `test`, `discipline`, … —
never `live-tests`, which no longer runs per PR. Verify with
`gh api repos/ractive/ff-rdp/branches/main/protection`.
