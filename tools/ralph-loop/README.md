# tools/ralph-loop — mirror of the ralph-loop skill scripts

The canonical copies of these scripts live in
`~/.claude/skills/ralph-loop/scripts/`. They are mirrored here so that:

- changes to the skill can be reviewed in a normal PR diff;
- the skill code is preserved in the project's git history alongside the
  iteration plans it operates on; and
- the mirror stays reviewable even though the canonical copy lives outside the
  repo.

There is **no automated drift check**. `cargo xtask check-discipline-regression`
verified the mirror until iter-162b deleted it, along with the two scripts whose
four-way duplication was the only reason it existed. Keeping the mirror in sync
is now a manual discipline.

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

Then refresh the mirror and confirm it matches:

```sh
cp ~/.claude/skills/ralph-loop/scripts/*.sh tools/ralph-loop/scripts/
diff -r ~/.claude/skills/ralph-loop/scripts/ tools/ralph-loop/scripts/
```

Nothing enforces this. Until iter-162b, `check-discipline-regression` diffed the
mirror against the live skill in CI and also replayed iter-61v (expected: fails)
and iter-61t (expected: passes); that behavioural baseline pinned the heuristics
in the two deleted scripts and could not outlive them.

## Why a mirror and not a symlink?

The skill directory lives outside the repo (`~/.claude/skills/ralph-loop/`),
so a symlink would only work on the maintainer's machine. The mirror lets CI
and other contributors see the scripts without needing the skill installed.

The trade-off: edits must be made in two places, and since iter-162b nothing
checks that you did. A 3-of-4 edit is exactly what went wrong in iter-140/146
(`3dc5330`) — run the `diff -r` above.
