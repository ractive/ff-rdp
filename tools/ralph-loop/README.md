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
| `scripts/run-iteration.sh` | Drives a single iteration through cmux. |

iter-162b deleted `ac-fidelity-check.sh` and `claims-vs-code.sh` from this
directory, from `tools/new-ralph-loop/scripts/` and from both canonical skill
directories, along with `run-iteration.sh`'s `--replay` mode, which existed only
to re-run them against an already-merged branch. `crates/xtask/tests/iter_162b_scripts_deleted.rs`
asserts they stay gone.

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
versa). It used to also replay iter-61v (expected: fails) and iter-61t
(expected: passes) against the live scripts; that behavioural baseline pinned
the heuristics in `ac-fidelity-check.sh` and `claims-vs-code.sh` and went with
them in iter-162b.

## Why a mirror and not a symlink?

The skill directory lives outside the repo (`~/.claude/skills/ralph-loop/`),
so a symlink would only work on the maintainer's machine. The mirror lets CI
and other contributors see the scripts without needing the skill installed.

The trade-off: edits must be made in two places. The `check-discipline-regression`
xtask is the load-bearing safeguard that catches drift.
