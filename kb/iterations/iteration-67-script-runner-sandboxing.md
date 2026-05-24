---
title: "Iteration 67: Script runner sandboxing — env allowlist + run depth + path containment"
type: iteration
date: 2026-05-24
status: done
branch: iter-67/script-sandboxing
depends_on:
  - iteration-61-script-runner-recorder
  - iteration-65-safe-write-and-path-traversal-hardening
first_call_sites:
  - primitive: ff_rdp_cli::script::vars::EnvPolicy
    site: >-
      crates/ff-rdp-cli/src/script/runner.rs::resolve_step_vars (gates
      `{{env.X}}` resolution via `opts.env_policy.check`)
  - primitive: ff_rdp_cli::script::runner::MAX_RUN_DEPTH
    site: >-
      crates/ff-rdp-cli/src/script/runner.rs::run_script_file (refuses entry
      when `call_stack.len() + 1` exceeds the cap)
dogfood_path: |
  # 1. Untrusted env var resolution is refused by default.
  echo '{"steps":[{"navigate":"https://x/{{env.AWS_SECRET_ACCESS_KEY}}"}]}' > /tmp/evil.json
  ff-rdp run /tmp/evil.json
  # Expected: exits non-zero, "env var AWS_SECRET_ACCESS_KEY not in allowlist (use --allow-env)"

  # 2. Explicit allowlist works.
  ff-rdp run /tmp/safe.json --allow-env LANG,USER

  # 3. Nested run depth cap.
  # Construct a 20-deep run chain; expect bail at depth 16.
  ff-rdp run /tmp/deep.json
  # Expected: exits non-zero, "run nesting depth 17 exceeds MAX_RUN_DEPTH=16"

  # 4. Absolute sub-script path refused without flag.
  echo '{"steps":[{"run":{"path":"/etc/passwd"}}]}' > /tmp/abs.json
  ff-rdp run /tmp/abs.json
  # Expected: exits non-zero, "sub-script path must be relative to top-level script dir"
tags: [iteration, security]
---

# Iteration 67: Script runner sandboxing — env allowlist + run depth + path containment

The script runner accepts untrusted input by design (recorded scripts,
shared `.script.json` files). Two gaps make that risky: `{{env.X}}`
resolves any env var without an allowlist (a hostile script exfiltrates
`AWS_SECRET_ACCESS_KEY` by interpolating it into a navigate URL), and
nested `run:` steps have cycle detection but no depth limit and no path
containment (a 1000-deep chain blows the stack; absolute paths probe
arbitrary readable files).

## Themes

- **A — Env-var allowlist.** Refuse `{{env.X}}` unless `X` is on a
  caller-supplied allowlist or in a tiny safe default set
  (`HOME`/`USER`/`LANG`). Refuse names matching `is_secret_name`
  unconditionally.
- **B — Sub-script depth + containment.** Cap nesting depth at 16; require
  sub-script paths to be descendants of the top-level script's directory
  unless `--allow-unsafe-script-paths` is set.

## Tasks

### A. Env-var allowlist
- [x] In `crates/ff-rdp-cli/src/script/vars.rs`, gate `std::env::var(name)` on an `EnvPolicy { allowlist: HashSet<String> }`.
- [x] Define `SAFE_DEFAULTS: &[&str] = &["HOME", "USER", "LANG", "LC_ALL", "TZ"]`.
- [x] Unconditionally refuse any name matched by `is_secret_name` (already defined nearby), even if explicitly allowlisted — fail closed.
- [x] Add `--allow-env <comma-list>` CLI flag on `ff-rdp run`; thread through.
- [x] Update the substitution error to name the variable and suggest the flag.

### B. Sub-script depth + containment
- [x] Add `const MAX_RUN_DEPTH: usize = 16;` at the top of `crates/ff-rdp-cli/src/script/runner.rs`.
- [x] In `run_script_file`, bail with a typed error when `call_stack.len() + 1 > MAX_RUN_DEPTH`.
- [x] At the path-resolution site in `execute_run`, reject absolute paths and `..`-traversing relative paths unless `--allow-unsafe-script-paths` is set. Containment ancestor = top-level script's parent dir.
- [x] Plumb the flag through `Cli` into the runner context.

## Acceptance Criteria [6/6]

- [x] `env_substitution_rejects_unallowed`: a `{{env.FFRDP_TEST_VAR_67A}}` reference with no allowlist returns `Err("env var ... not in allowlist (use --allow-env ...)")`. (`crates/ff-rdp-cli/src/script/vars.rs::tests::env_substitution_rejects_unallowed`)
- [x] `env_substitution_allowlist_works`: with `EnvPolicy::from_names(["FFRDP_TEST_FOO_67"])`, `{{env.FFRDP_TEST_FOO_67}}` resolves; `{{env.FFRDP_TEST_BAR_67}}` still fails. (`tests::env_substitution_allowlist_works`)
- [x] `env_substitution_refuses_secret_names`: `{{env.AWS_SECRET_ACCESS_KEY}}` fails even when allowlisted (secret-name policy is unconditional). (`tests::env_substitution_refuses_secret_names`)
- [x] `run_depth_capped`: pre-populating the call-stack with 16 entries causes the next `run_script_file` entry to bail with `AppError::User("run nesting depth 17 exceeds MAX_RUN_DEPTH=16")`. End-to-end coverage via `run_depth_chain_eventually_fails`. (`crates/ff-rdp-cli/src/script/runner.rs::tests::run_depth_capped` + `tests::run_depth_chain_eventually_fails`)
- [x] `run_path_containment`: `check_sub_script_containment("/etc/passwd", …)` refuses absolute paths and `..` traversal; `--allow-unsafe-script-paths` skips the check (`tests::run_path_containment_rejects_absolute`, `tests::run_path_containment_rejects_parent_traversal`, `tests::run_path_containment_accepts_relative_within_top`).
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

Fail-closed on secret-name patterns is intentional: the typical incident
shape is a teammate sharing a "harmless looking" script that happens to
read a credential via interpolation. Even an explicit allowlist shouldn't
override the secret-name shape, because the operator approving the
allowlist may not have noticed the secret in their env.

`MAX_RUN_DEPTH = 16` matches the kind of depth a legitimate script ever
needs (top-level → suite → subtest → fixture-setup → action). Configurable
via env var if a real use case appears.

`--allow-unsafe-script-paths` exists for the dogfooding case (running a
script that includes a shared lib under `~/scripts/lib/`). Documented as
"only enable when you author every file in the include chain".

## Out of scope

- A signed-script format (would solve the trust problem more cleanly but
  needs separate design work; file a research note).
- Per-step capability gating (e.g. "this script may not call eval"). Bigger
  redesign of the runner; out of scope here.

## References

- [[iteration-61-script-runner-recorder]]
- Security review report (2026-05-24), findings F-7, F-8
