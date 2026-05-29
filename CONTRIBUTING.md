# Contributing to ff-rdp

## Quality gates

Before committing or opening a PR, run these **in order** and fix all issues:

```sh
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace -q
```

Never skip a step. Never commit code that fails any of these.

## Iteration discipline tooling

### Check for dead primitives

Every new `pub` item introduced in a PR must have at least one non-test consumer in the
same PR. The `check-dead-primitives` command enforces this:

```sh
cargo run -p xtask -- check-dead-primitives --since origin/main
```

This diffs against `origin/main` (fallback: `main`), finds new `pub fn/struct/enum/trait/mod`
declarations, and uses ripgrep to confirm at least one non-test caller exists. Exit 1 if any
new pub items are unwired.

Ripgrep (`rg`) must be on your PATH. Install via your package manager:
- macOS: `brew install ripgrep`
- Ubuntu: `sudo apt-get install ripgrep`
- Windows: `winget install BurntSushi.ripgrep.MSVC`

### Check TODO annotations

Every `TODO`, `FIXME`, or `XXX` comment in new code must be accompanied by either:
- A GitHub issue link: `https://github.com/ractive/ff-rdp/issues/N`
- A Jira-style ticket: `WORD-123`
- An explicit allow annotation: `// allow-todo: <reason>`

```sh
cargo run -p xtask -- check-todo-annotations --since origin/main
```

Exit 1 if any unannotated TODOs are found in the diff.

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

### Check actor ↔ kb sync

If any `crates/ff-rdp-core/src/actors/<X>.rs` was changed, the corresponding
`kb/rdp/actors/<X>.md` must also be updated (or a `// allow-actor-kb-skip: <reason>`
annotation added to the first 20 lines of the actor file):

```sh
cargo run -p xtask -- check-actor-kb-sync --since origin/main
```

Added in iter-73. See the ACTOR_KB_MAP constant in `crates/xtask/src/check_actor_kb_sync.rs`
for the full actor → kb path mapping.

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

`check-dogfood-script` is the 7th sub-check run by `check-iteration-ready`. It is also
a required CI step on `iter-*` branches in the Live Tests workflow (`live.yml`) when
`FF_RDP_LIVE_TESTS=1`.

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

## Pre-commit hook

A pre-commit hook that enforces the TODO annotation rules is included in `.githooks/`.
To install it:

```sh
git config core.hooksPath .githooks
```

The hook scans the staged diff for unannotated `TODO`/`FIXME`/`XXX` and exits non-zero
with the offending file:line if any are found.

**Bypass (emergencies only):**
```sh
git commit --no-verify
```

Note: the CI `discipline` job is the load-bearing gate. The pre-commit hook is a
developer convenience — bypassing it locally doesn't skip CI.

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

### One-shot pre-PR discipline gate

Before calling `/create-pr` on any `iter-*` branch, run the aggregator that
wraps every discipline gate into a single command:

```sh
# Resolve the plan automatically from the current branch:
BRANCH=$(git branch --show-current)
PLAN=$(cargo run -q -p xtask -- find-iteration-plan --branch "$BRANCH" 2>/dev/null || true)
if [ -n "$PLAN" ]; then
  cargo run -p xtask -- check-iteration-ready --plan "$PLAN" --base origin/main
fi
```

`check-iteration-ready` runs:
1. `check-dead-primitives --since <base>` — no unwired new pub items
2. `check-todo-annotations --since <base>` — no bare TODO/FIXME/XXX <!-- allow-todo: documents the check itself -->
3. `check-actor-kb-sync --since <base>` — actor `.rs` changes paired with kb updates
4. `check-firefox-refs <plan>` — `firefox_refs:` line ranges valid
5. `check-discipline-regression` — mirror sync + replay baselines
6. `ac-fidelity-check.sh` — ticked ACs backed by diff evidence

Fix every reported failure before pushing. The `/create-pr` skill runs this
automatically on iter-* branches.

Note: CI still runs each gate individually as separate required checks for
clearer per-check GitHub status attribution.

## PR discipline

- One iteration = one branch = one PR
- Branch naming: `iter-N/short-description`
- Self-review the diff before requesting review — catch fmt, clippy, dead code yourself
- The `discipline` CI job runs `check-dead-primitives` and `check-todo-annotations` on
  every PR

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

## Branch protection — `live-tests` required check

The `live-tests` GitHub Actions job must be a **required** status check on `main` so that
a red live-tests run blocks merging (iter-87).

**Verify current state:**

```sh
bash tools/branch-protection.sh ractive/ff-rdp
```

**Apply the protection rule (requires admin access):**

```sh
gh api repos/ractive/ff-rdp/branches/main/protection \
  --method PUT \
  --field required_status_checks='{"strict":false,"contexts":["live-tests","fmt","clippy"]}' \
  --field enforce_admins=false \
  --field required_pull_request_reviews=null \
  --field restrictions=null
```

After applying, re-run `bash tools/branch-protection.sh` to confirm it exits 0.
