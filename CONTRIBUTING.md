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
- `status` is one of: `planned`, `in-review`, `done`
- If the plan body mentions `pub fn/struct/enum/trait/mod`, `first_call_sites` must be non-empty
  with `primitive` and `site` keys per entry
- A `dogfood_path` frontmatter key or a `## Dogfood path` body section is present

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

## PR discipline

- One iteration = one branch = one PR
- Branch naming: `iter-N/short-description`
- Self-review the diff before requesting review — catch fmt, clippy, dead code yourself
- The `discipline` CI job runs `check-dead-primitives` and `check-todo-annotations` on
  every PR

## ralph-loop (automated iteration runs)

When running iterations via the ralph-loop skill, each agent also runs the xtask discipline
checks before invoking `/create-pr`. See the ralph-loop `SKILL.md` for details.
