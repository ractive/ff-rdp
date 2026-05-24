# tools/ralph-loop — mirror of the ralph-loop skill scripts

The canonical copies of these scripts live in
`~/.claude/skills/ralph-loop/scripts/`. They are mirrored here so that:

- changes to the skill can be reviewed in a normal PR diff;
- the skill code is preserved in the project's git history alongside the
  iteration plans it operates on; and
- `cargo xtask check-discipline-regression` can verify the mirror is in sync
  with the live skill on disk (so a stale mirror can't silently diverge).

## Scripts

| File | Purpose |
|------|---------|
| `scripts/claims-vs-code.sh` | Extract verb-symbol claims from iteration commit messages, grep the branch diff for evidence, emit a markdown "Claims vs code" section. Exit 1 if any ❌ remain unannotated. |
| `scripts/ac-fidelity-check.sh` | Parse the iteration plan's `## Acceptance Criteria` block; for each ticked checkbox, verify the diff contains a matching test function, symbol, or `[deferred — new plan: …]` annotation. Exit 1 otherwise. |
| `scripts/run-iteration.sh` | Drives a single iteration through cmux. Supports a `--replay <iter-id>` mode that re-runs the two checks above against an already-merged branch — used by `cargo xtask check-discipline-regression`. |

## Editing workflow

Edit the canonical copy first:

```sh
$EDITOR ~/.claude/skills/ralph-loop/scripts/<file>
```

Then refresh the mirror and run the regression target:

```sh
cp ~/.claude/skills/ralph-loop/scripts/*.sh tools/ralph-loop/scripts/
cargo run -p xtask -- check-discipline-regression
```

The xtask diffs the mirror against the live skill and fails if they drift, so
CI catches the case where the skill was edited but the mirror wasn't (or vice
versa). It also runs `run-iteration.sh --replay iter-61v` (expected: fails)
and `--replay iter-61t` (expected: passes) as a behavioural regression check
against the live scripts.

## Why a mirror and not a symlink?

The skill directory lives outside the repo (`~/.claude/skills/ralph-loop/`),
so a symlink would only work on the maintainer's machine. The mirror lets CI
and other contributors see the scripts without needing the skill installed.

The trade-off: edits must be made in two places. The `check-discipline-regression`
xtask is the load-bearing safeguard that catches drift.
