---
title: "Iteration 73: Move iteration-plan validation into hyalo schema"
type: iteration
date: 2026-05-24
status: obsolete
branch: iter-73/hyalo-schema
depends_on:
  - iteration-61y-iteration-discipline-tooling
first_call_sites: []
dogfood_path: |
  # 1. hyalo lint catches missing required frontmatter fields on a fresh plan.
  cp kb/iterations/_template.md /tmp/bad.md
  hyalo lint /tmp/bad.md      # exits non-zero, reports missing branch/dogfood_path/etc.
  
  # 2. hyalo lint catches a status=completed plan with open checkboxes (HYALO002).
  hyalo lint kb/iterations/iteration-72-transport-polish.md   # clean while open
  # then flip frontmatter to status=completed without ticking → fails
  
  # 3. xtask check-iteration-plan now only enforces the conditional
  #    "body mentions pub items → first_call_sites must be non-empty" rule.
  cargo run -p xtask -- check-iteration-plan kb/iterations/iteration-72-transport-polish.md
  # Expected: OK (hyalo lint covers the rest; xtask is now a thin xtask).
  
  # 4. CI runs both:
  #    - `hyalo lint --type iteration --rule-prefix HYALO --rule-prefix FM` as a required check
  #    - `cargo run -p xtask -- check-iteration-plan` for the conditional rule
tags:
  - iteration
  - tooling
---

# Iteration 73: Move iteration-plan validation into hyalo schema

`crates/xtask/src/check_iteration_plan.rs` reinvents what hyalo's `lint` +
`types` surface already does: required-property enforcement, enum validation,
pattern matching, the cross-cutting `HYALO002` rule that ties
`status: completed` to all-checkboxes-ticked. `.hyalo.toml` exists in the
repo but defines **no** `[schema.types.iteration]` block, so `hyalo lint`
runs without frontmatter rules and the xtask carries the load alone. Define
the schema, wire `hyalo lint` into CI, and shrink the xtask to the one rule
hyalo cannot express natively — *"if the plan body names new `pub` items,
`first_call_sites:` must be non-empty"*.

## Themes

- **A — Define `[schema.types.iteration]` in `.hyalo.toml`** mirroring the
  current `PlanFrontmatter` enforcement (required fields, status enum,
  branch pattern, etc.).
- **B — Wire `hyalo lint` into CI** as a required check (alongside the
  existing xtask gate during the transition).
- **C — Shrink `check-iteration-plan`** to the conditional rule
  hyalo can't express, removing duplicated YAML parsing.
- **D — Replace shell/sed frontmatter mutations with `hyalo set`.** The
  ralph-loop scripts (and the post-merge `chore(iter-NN): mark status=done`
  step) currently flip frontmatter via shell text rewriting. Use
  `hyalo set --property status=done --file kb/iterations/iteration-NN-*.md`
  instead — type-aware, idempotent, schema-validated.

## Tasks

### A. Define the hyalo schema
- [ ] Edit `.hyalo.toml`; add `[schema.types.iteration]` with:
  - `required = ["title", "type", "status", "branch", "date", "dogfood_path", "tags"]`
  - `[schema.types.iteration.properties.status]` enum (current values: `planned`, `in-progress`, `in_progress`, `in-review`, `done`, `completed`).
  - `[schema.types.iteration.properties.branch]` pattern `^iter-[0-9]+[a-z]*/`
  - `[schema.types.iteration.properties.date]` type=date.
- [ ] Run `hyalo lint kb/iterations/**/*.md` once locally; fix any existing plans that drift from the schema (one normalisation pass — likely the `in_progress` vs `in-progress` divergence the recent iterations introduced).

### B. CI wiring
- [ ] Add a `hyalo-lint` job to `.github/workflows/ci.yml` running
  `hyalo lint kb/iterations/**/*.md --format json` and failing on any error.
- [ ] Make it a required check (branch protection update if the repo enforces).
- [ ] Document in `CONTRIBUTING.md` how to install hyalo locally and how to
  interpret `hyalo lint` failures.

### C. Shrink the xtask
- [ ] Strip `check_iteration_plan.rs` down to a single rule: parse the body,
  detect mentions of new `pub` items, require `first_call_sites` non-empty.
  Delete the required-fields / enum / pattern logic now living in the
  hyalo schema.
- [ ] Update `crates/xtask/Cargo.toml` to drop the now-unused `serde_yaml`
  if no other subcommand needs it.
- [ ] Update the CLI help text to reflect the narrower scope.
- [ ] Run `cargo run -p xtask -- check-iteration-plan` against every plan
  in `kb/iterations/`; assert each passes.

### D. `hyalo set` for frontmatter mutations
- [ ] Audit ralph-loop scripts (`~/.claude/skills/ralph-loop/scripts/`
  mirrored to `tools/ralph-loop/scripts/`) for shell/sed/python that mutates
  iteration-plan frontmatter (status flips, date stamps, ticking ACs).
- [ ] Replace each with `hyalo set --property K=V --file <path>` (scalars
  and string-lists) where the schema allows. Note: `hyalo set` does not
  accept structured objects like `first_call_sites: [{primitive, site}]`
  (only scalars and string-lists), so that field stays in author-edited
  YAML for now; capture as a hyalo follow-up if it becomes painful.
- [ ] Add a smoke test in `tools/ralph-loop/tests/` (or as a shell test
  alongside the scripts) that exercises the status-flip path end-to-end
  via `hyalo set` rather than the legacy sed pipeline.

## Acceptance Criteria [0/7]

- [ ] `hyalo_schema_defined_for_iteration_type`: `hyalo types show iteration` returns a schema with required fields, status enum, and branch pattern.
- [ ] `hyalo_lint_catches_missing_required_field`: a synthetic plan with `branch:` removed exits non-zero with a frontmatter error pointing at `branch`.
- [ ] `hyalo_lint_catches_completed_with_open_tasks`: a plan with `status: completed` and an unticked checkbox triggers `HYALO002`.
- [ ] `ci_hyalo_lint_required`: `.github/workflows/ci.yml` includes a `hyalo-lint` job and it is named in the branch-protection required set.
- [ ] `xtask_check_iteration_plan_narrowed`: `crates/xtask/src/check_iteration_plan.rs` no longer parses `status`, `branch`, `date`, `tags` directly; it only checks the `pub items mentioned → first_call_sites` rule. Unit test `pub_mention_requires_first_call_sites` covers the positive and negative paths.
- [ ] `ralph_loop_status_flip_uses_hyalo_set`: the post-merge "mark status=done" step in `tools/ralph-loop/scripts/` (and the skill-dir mirror) shells out to `hyalo set --property status=done --file …` instead of sed/awk. Smoke test asserts the resulting frontmatter parses and the body is byte-identical to the pre-flip version except for the `status:` line.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

Hyalo already ships the right primitives — schema definitions in
`.hyalo.toml`, the HYALO native rules (HYALO001 bare `[]` → `- [ ]`,
HYALO002 status/task coherence), and the mdbook-lint MD001..MD059 body
rules. Carrying duplicated YAML-parsing in xtask is pure tooling debt that
the early iter-61y discipline build-out didn't notice was redundant.

The conditional rule (*"if body mentions `pub` items, `first_call_sites`
must be non-empty"*) is genuinely cross-cutting and ff-rdp-specific — it
ties markdown body to Rust source convention. Hyalo can't express it via
schema alone, so it stays in xtask. If hyalo grows a plugin / custom-rule
surface in the future, this could move there too.

Migration is safe: hyalo lint failing doesn't break Phase 2 of ralph-loop
because the xtask still runs first (during the transition). Once we've
seen two or three iterations land cleanly under both, drop the duplicated
fields from xtask.

Two name-collision risks worth watching:
- `status` values currently include both `in-progress` and `in_progress`
  in the wild (different agents picked different forms). The schema enum
  must accept both, or task A.2 must normalise them all first.
- `_template.md` uses placeholder values (`iter-NN/short-description`,
  `YYYY-MM-DD`). Either exempt the template from lint or list it in
  `[lint]` exclusions.

## Out of scope

- A hyalo plugin surface for the `first_call_sites` conditional rule —
  current hyalo doesn't expose one. Stay in xtask.
- Replacing `claims-vs-code.sh` / `ac-fidelity-check.sh` with hyalo. Those
  parse `## Acceptance Criteria` body content and check diff evidence, a
  fundamentally different shape; out of scope here.
- A general kb-wide schema pass for non-iteration types (decision-log,
  research, rdp/, etc.). File separately once iteration plans are clean.

## References

- [[iteration-61y-iteration-discipline-tooling]]
- `hyalo lint --help` / `hyalo types --help`
- `.hyalo.toml`
- Post-iter-62 session conversation on tooling debt
