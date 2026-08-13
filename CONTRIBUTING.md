# Contributing to ff-rdp

## Quality gates

Before committing or opening a PR, run these **in order** and fix all issues:

```sh
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace -q
```

Never skip a step. Never commit code that fails any of these.

## Test layout

Integration tests for `ff-rdp-cli` are organized into a few consolidated
targets rather than one binary per file — every extra top-level `tests/*.rs`
file is a separate test binary that `cargo test` must compile, link, and run,
which dominates the iteration loop's wall-clock cost.

- **Live-Firefox tests** (anything gated behind `FF_RDP_LIVE_TESTS=1`) go in
  `crates/ff-rdp-cli/tests/live/<slug>.rs` **plus a `mod` line in
  `crates/ff-rdp-cli/tests/live/main.rs`**. They compile into the single
  `live` test target. A new top-level `crates/ff-rdp-cli/tests/live_*.rs` file
  is a **review defect** — it re-introduces the ~45-binary linking cost
  iter-100b removed. The `check-live-test-layout` xtask gate (run in the CI
  `discipline` job) fails the build if one reappears.
- **Every `#[test]` under `tests/live/` must carry `#[ignore]`** (iter-113
  Theme B). A plain `cargo test` must stay Firefox-free and fast; a bare
  (ungated) live test hangs a Firefox-less CI job for the whole job budget
  before the job timeout fires — exactly the iter-112 failure. The
  `check-live-test-layout` gate now also scans `tests/live/` and fails on any
  `#[test]` that is neither `#[ignore]`-gated nor annotated. Convention:
  `#[test]` immediately followed by `#[ignore = "requires a live Firefox
  instance — set FF_RDP_LIVE_TESTS=1"]` (an intervening `#[cfg(unix)]` between
  the two is fine). For the rare runtime-gated fast probe that *must* run by
  default — a Firefox-free mock probe that carries its own runtime guard — add
  an `// allow-ungated-live: <reason>` comment in the attribute block above the
  `#[test]` instead. Reach for it sparingly: an ungated live test's early
  return is counted by libtest as a pass, which is exactly the false-green
  `live-sweep` exists to eliminate (iter-155).
- **Launch waits are bounded and env-overridable** (iter-113 Theme A). The
  live launchers wait for Firefox's remote-debugging port via a bound that
  defaults to 30 s and is overridable with `FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS`
  (whole seconds). `common::wait_for_debugger_port_within` panics with a
  message naming the launcher binary and port when the port never opens, so a
  wedged or absent Firefox fails fast and self-describingly instead of
  hanging.
- Shared live-test helpers live in `crates/ff-rdp-cli/tests/common/mod.rs`,
  declared once from `tests/live/main.rs` via
  `#[path = "../common/mod.rs"] mod common;`; suites refer to them as
  `use crate::common::…` (e.g. `live_tests_enabled`,
  `live_network_tests_enabled`).
- **Mock-server e2e tests** go under `tests/e2e/` as modules of
  `tests/e2e/main.rs` (the `e2e` target) — see iter-46.
- Run one migrated live suite:
  `FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live <module> -- --include-ignored`.
  Enumerate every live test name (no Firefox needed):
  `cargo test -p ff-rdp-cli --test live -- --list`.
- **Do not run the full live suite with `FF_RDP_LIVE_TESTS=1 cargo test-live` and trust the
  `N passed; 0 failed` summary line.** Every live test additionally checks its own env gate at
  runtime and `return`s early when unset; libtest counts an early `return` as `ok`, not
  `ignored`, so that summary line cannot tell "N tests exercised Firefox" apart from "N tests
  no-op'd because `FF_RDP_LIVE_NETWORK_TESTS` was never set" (iter-155). Use
  `cargo run -p xtask -- live-sweep` instead: it classifies every `#[ignore]`-gated live test from
  its own ignore-reason text, runs only the tests whose required env var(s) are actually set (with
  `--include-ignored`, so libtest reports genuine `ok`/`FAILED`), and runs the rest *without*
  `--include-ignored` so libtest reports them `ignored` using its own vocabulary. It ends with a
  machine-readable `LIVE_SWEEP_SUMMARY executed=N skipped=M total=T` line — quote `executed=N` in
  a `[verified: <date>, …]` AC annotation instead of the `cargo test` summary line. Add
  `--dry-run` to see the qualified/unqualified split without invoking `cargo test`.

## Iteration discipline tooling

### Review rules that are no longer gates

Two long-standing rules survive as **review** rules. iter-162a deleted the xtask
subcommands that enforced them, for reasons worth keeping on the record:

- **Every new `pub` item needs a non-test consumer in the same PR.**
  `check-dead-primitives` enforced this and was gamed: a `DemuxReader::new()` was
  constructed in `daemon/server.rs` for no reason other than to satisfy it, while 425
  lines of genuinely dead public API shipped and survived every CI run until a human
  review found them. See the comment at `crates/ff-rdp-cli/src/daemon/server.rs:750-756`.
- **Every `TODO`/`FIXME`/`XXX` needs a GitHub issue link, a `WORD-123` ticket, or an
  explicit `// allow-todo: <reason>`.** `check-todo-annotations` (plus a 91-line
  pre-commit hook duplicating it, plus a CI step) guarded a set that was empty for its
  entire lifetime: zero hits in `crates/ff-rdp-{core,cli}/src`.

### Validate an iteration plan

```sh
cargo run -p xtask -- check-iteration-plan kb/iterations/iteration-NN-slug.md
```

This validates:
- `status` is one of: `planned`, `in-progress`, `in-review`, `done`
- If the plan body mentions `pub fn/struct/enum/trait/mod`, `first_call_sites` must be non-empty
  with `primitive` and `site` keys per entry
- A `dogfood_path` frontmatter key or a `## Dogfood path` body section is present

### Validate firefox_refs in an iteration plan

If a plan has a `firefox_refs:` frontmatter key, validate that the cited line ranges
exist in the local Firefox checkout:

```sh
FF_RDP_FIREFOX_PATH=/Users/james/devel/firefox \
  cargo run -p xtask -- check-firefox-refs kb/iterations/iteration-NN-slug.md
```

Set `FF_RDP_FIREFOX_PATH` to your Firefox source tree. The default is `/Users/james/devel/firefox`.
Plans with no `firefox_refs:` key are accepted silently. Added in iter-73.

Local-only since iter-162a: CI runners have no Firefox checkout, so the CI step's own
name said `(no-op in CI)` and it ran against a plan with no `firefox_refs`. Run it by
hand — it is the only gate that checks a claim against ground truth outside this
repository, and both of its catches were false Firefox citations stopped before merge.

### Check actor ↔ kb sync

If any `crates/ff-rdp-core/src/actors/<X>.rs` was changed, the corresponding
`kb/rdp/actors/<X>.md` must also be updated (or a `// allow-actor-kb-skip: <reason>`
annotation added to the first 20 lines of the actor file):

```sh
cargo run -p xtask -- check-actor-kb-sync --since origin/main
```

Added in iter-73. See the ACTOR_KB_MAP constant in `crates/xtask/src/check_actor_kb_sync.rs`
for the full actor → kb path mapping.

Local-only since iter-162a. It fired three times (`18146ff`, `e5e58e3`, `36f1c63`) and
every response was "write the missing doc" — a working docs-sync reminder, not a defect
gate, and it already carries 8 `// allow-actor-kb-skip:` escape hatches.

### Check source invariants

Three regex scans of product source under one subcommand, each reporting its own named
result line (merged from `check-daemon-locks`, `check-error-envelope-paths` and
`check-stderr-annotations` in iter-162a):

```sh
cargo run -p xtask -- check-source-invariants
```

- **daemon-locks** (iter-63) — no `.lock().unwrap()` under
  `crates/ff-rdp-cli/src/daemon/`; use `lock_or_recover!` so a poisoned mutex doesn't
  take the whole daemon process down. Rustfmt-split chains are caught too.
  `.lock().expect(...)` is deliberately out of scope: `#[cfg(test)]` modules use it
  where panic-on-poison is the desired behaviour.
- **error-envelope-paths** (iter-145 Theme C) — no `eprintln!` in
  `crates/ff-rdp-cli/src/commands/` immediately followed by a bare `AppError::Exit(N)`,
  the print-then-bypass idiom that let click-time JS exceptions skip the JSON error
  envelope.
- **stderr-annotations** (iter-148) — every `eprintln!` under `commands/` (outside
  `#[cfg(test)]`) carries a `// stderr-ok: <reason>` justification comment.

The `// stderr-ok:` comment must be on the `eprintln!` line or within the two lines
above it; it exempts a site from both `eprintln!` invariants. This runs in the CI
`discipline` job.

### Runnable dogfood script (Theme M, iter-85)

Iteration plans may include a `dogfood_script` key in their YAML frontmatter pointing to
a sibling shell script:

```yaml
dogfood_script: iteration-85-dogfood-57-carryovers-and-runnable-dogfood-path.dogfood.sh
```

The script lives next to the `.md` plan file and is executed by:

```sh
cargo run -p xtask -- check-dogfood-script kb/iterations/iteration-NN-slug.md
```

Requirements:
- The script **must** write the sentinel file `/tmp/ff-rdp-iter-<N>-dogfood-ok` before
  exiting 0 (where `N` is the iteration number extracted from the plan filename).
- The gate is silently skipped if `FF_RDP_LIVE_TESTS` is not set to `"1"`.
- Plans with no `dogfood_script` field are also skipped (pass) — existing iterations
  without the field continue to work.
- `dogfood_path` and `dogfood_script` may coexist; a warning is emitted but it is not
  a hard failure.

`check-dogfood-script` also runs the `lint-dogfood-script` sub-check — a static lint of
the referenced `.dogfood.sh` (`tools/lint-dogfood-script.sh`) that runs regardless of
`FF_RDP_LIVE_TESTS` and fails the subcommand on any rule violation. It has no CI step:
the Live Tests workflow's dogfood step was removed in iter-117 (see the comment at the
end of `live.yml`), so this gate is local-only and runs pre-PR.

Windows: the bash invocation is skipped on non-unix platforms (CI runs on ubuntu-latest).

### rdp-spec-reviewer agent

A `rdp-spec-reviewer` subagent is installed at `~/.claude/agents/rdp-spec-reviewer.md`
(mirrored from `tools/agents/rdp-spec-reviewer.md`). When a PR touches actor files, the
`/create-pr` skill invokes it and appends a `## Spec drift` section to the PR body.

To invoke manually:
```sh
claude --agent rdp-spec-reviewer --input tools/agents/fixtures/synthetic-watcher-diff.patch
```

The agent mirror follows the same pattern as the ralph-loop scripts mirror: edit both
`~/.claude/agents/rdp-spec-reviewer.md` and `tools/agents/rdp-spec-reviewer.md` in sync.

## Iteration plan template

New iteration plans live in `kb/iterations/`. Use the template:

```sh
cp kb/iterations/_template.md kb/iterations/iteration-NN-slug.md
```

Then edit the frontmatter:
- `title`: `"Iteration NN: Short title"`
- `date`: today's date
- `branch`: `iter-NN/short-description`
- `first_call_sites`: list any new `pub` items with their first call site
- `dogfood_path`: describe how to manually exercise the iteration's output

The plan linter (`cargo xtask check-iteration-plan`) enforces these fields.

### Pre-PR discipline gates

There is no aggregator subcommand. iter-162a deleted `check-iteration-ready`: it
hard-coded its own sub-check count in four places, so every gate added or removed cost
a count bump plus assertion-text edits, and a test asserted that three documentation
files still named it — which made editing prose a build failure.

Before calling `/create-pr` on any `iter-*` branch, enumerate the gates xtask actually
ships and run each one:

```sh
# Resolve the plan automatically from the current branch:
BRANCH=$(git branch --show-current)
PLAN=$(cargo run -q -p xtask -- find-iteration-plan --branch "$BRANCH" 2>/dev/null || true)

# List the check-* subcommands this xtask offers — do not invent names.
cargo run -q -p xtask -- --help

cargo run -p xtask -- check-live-test-layout
cargo run -p xtask -- check-source-invariants
cargo run -p xtask -- check-discipline-regression
cargo run -p xtask -- check-actor-kb-sync --since origin/main
[ -n "$PLAN" ] && cargo run -p xtask -- check-iteration-plan "$PLAN"
[ -n "$PLAN" ] && cargo run -p xtask -- check-firefox-refs "$PLAN"
[ -n "$PLAN" ] && cargo run -p xtask -- check-dogfood-script "$PLAN"
[ -n "$PLAN" ] && bash tools/ralph-loop/scripts/ac-fidelity-check.sh \
  --plan "$PLAN" --base origin/main
```

`ac-fidelity-check.sh` checks that ticked ACs *reference* evidence that resolves in the
diff, declare no non-execution, and carry `[verified: <YYYY-MM-DD>, <measured result>]`
where they name a `live_*` test. It reads a plan and a diff only: it cannot verify a
test ran (iter-154).

Fix every reported failure before pushing. The `/create-pr` skill runs these
automatically on iter-* branches.

## PR discipline

- One iteration = one branch = one PR
- Branch naming: `iter-N/short-description`
- Self-review the diff before requesting review — catch fmt, clippy, dead code yourself
- The `discipline` CI job runs the xtask gates that work without a Firefox checkout

## Supply-chain checks

`cargo audit` (RustSec advisory DB) and `cargo deny check` (advisories +
licences + bans + sources) run on every PR via the `supply-chain` job in
`.github/workflows/ci.yml`. They are required checks.

When a new advisory lands and breaks CI, choose one path:

1. **Yank-and-upgrade (preferred).** Run `cargo update -p <crate>` to a
   patched version, regenerate `Cargo.lock`, commit.
2. **Pin a working version.** If the maintainer hasn't released a fix yet
   but a known-good prior version exists, pin it with
   `<crate> = "=X.Y.Z"` in `Cargo.toml` and link the upstream issue.
3. **Ignore with reason.** If the advisory does not apply to our use of
   the crate (e.g. a `dev-dependency`, or a code path we never invoke),
   add the advisory ID to `[advisories].ignore` in `deny.toml` *with* a
   `# advisory ID — short justification, link to upstream issue` comment.
   Never ignore without a written reason.

License or ban regressions follow the same rule of thumb: prefer
removing the offending dep; only widen the allow-list if the licence is
genuinely compatible.

## Fuzzing

Parser-surface fuzz harnesses live in `fuzz/` (`transport_recv_from`,
`parse_page_map_str`, `parse_script_file`). They run for 60 s each on
every PR via the `fuzz` job.

Local setup (nightly only):

```sh
rustup install nightly
cargo install cargo-fuzz
cd fuzz
cargo +nightly fuzz run transport_recv_from seeds/transport_recv_from -- -max_total_time=60
```

When CI reports a fuzz crash:

1. Download the minimised input from the failed job's artifacts.
2. Reproduce locally with `cargo +nightly fuzz run <target> <input>`.
3. Open a GitHub issue tagged `fuzz-finding` with the minimised input
   attached.
4. Fix the parser, then check the input into `fuzz/seeds/<target>/` as a
   permanent regression seed.

See `fuzz/README.md` for the full target list.

## ralph-loop (automated iteration runs)

When running iterations via the ralph-loop skill, each agent also runs the xtask discipline
checks before invoking `/create-pr`. See the ralph-loop `SKILL.md` for details.

## Branch protection

`main` is not currently protected. The former `tools/branch-protection.sh` checker and
this section's instructions were deleted in iter-162a: the script required `live-tests`
as a status context, but `live.yml` stopped running per-PR in iter-117, so applying the
rule it verified would have made *every* PR unmergeable — the required context can never
report. It also shelled out to `python3` in a Rust-only repo, and `d6f31c4` records that
`main` was unprotected the whole time it was supposedly being checked.

If protection is reintroduced, pick contexts from the jobs in `.github/workflows/ci.yml`
that actually run on `pull_request` — `fmt`, `clippy`, `test`, `discipline` — and verify
with `gh api repos/ractive/ff-rdp/branches/main/protection`.
