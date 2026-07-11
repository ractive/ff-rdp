---
title: "Iteration 120: completions subcommand + package shell completions"
type: iteration
date: 2026-07-11
status: completed
branch: iter-120/completions
depends_on: ["iter-119/linux-packages"]
firefox_refs: []
kb_refs: []
first_call_sites: []
dogfood_path: |
  # Generate completions locally and confirm they load without error:
  cargo build --release -p ff-rdp-cli
  target/release/ff-rdp completions bash | bash -n -
  target/release/ff-rdp completions zsh  > /tmp/_ff-rdp && zsh -c 'autoload -U compinit; compinit -d /tmp/zcompdump; source /tmp/_ff-rdp' 2>&1 | head -5
  target/release/ff-rdp completions fish > /tmp/ff-rdp.fish && fish -c 'source /tmp/ff-rdp.fish' 2>&1 | head -5
  # expect: no syntax errors from any shell; bash -n exits 0.
tags:
  - iteration
  - ci
  - release
  - infra
  - packaging

---

# Iteration 120: completions subcommand + package shell completions

Iteration 119 shipped `.deb`/`.rpm` packaging for ff-rdp with only the
binary + `LICENSE`/`README.md` as assets, explicitly noting that ff-rdp
(unlike `hoppy`) had no shell completions or man pages to stage. This
iteration closes that gap for completions (man pages remain out of scope —
matches the hyalo/hoppy precedent of shipping completions without man
pages). ff-rdp gains a `completions <SHELL>` subcommand (via `clap_complete`,
using hoppy's plural naming convention) and the release pipeline now stages
bash/zsh/fish completion scripts into both the release archives
(`extra-archive-paths`) and the `.deb`/`.rpm` packages
(`[package.metadata.deb]` / `[package.metadata.generate-rpm]` assets), via
the shared workflow's `pre-package-command` input.

## Tasks

- [x] Add `Command::Completions { shell: clap_complete::Shell }` to the CLI's
      `Command` enum with a house-style `long_about`.
  - Code: `crates/ff-rdp-cli/src/cli/args.rs`
- [x] Add `clap_complete` as a workspace dependency, pinned to match clap's
      `"4.6"` style (`cargo add --dry-run` resolved 4.6.7).
  - Code: `Cargo.toml`, `crates/ff-rdp-cli/Cargo.toml`
- [x] New command module `commands/completions.rs` with a pure
      `generate_to(shell, writer)` core and a `run(shell)` entry point
      writing to stdout, registered alphabetically in `commands/mod.rs`.
  - Code: `crates/ff-rdp-cli/src/commands/completions.rs`,
    `crates/ff-rdp-cli/src/commands/mod.rs`
- [x] Wire dispatch: add `Command::Completions { .. }` to the
      connection-free `None` alternation in `command_to_step`, and add the
      real dispatch arm.
  - Code: `crates/ff-rdp-cli/src/dispatch.rs`
- [x] e2e tests: each supported shell produces non-empty output referencing
      the binary name; an unknown shell value fails as a clap parse error;
      no connection flags required.
  - Code: `crates/ff-rdp-cli/tests/e2e/completions.rs`,
    `crates/ff-rdp-cli/tests/e2e/main.rs`
- [x] Unit test for the pure `generate_to` function.
  - Code: `crates/ff-rdp-cli/src/commands/completions.rs` (`#[cfg(test)] mod
    tests`)
- [x] Wire `pre-package-command` (stage bash/zsh/fish completions into
      `crates/ff-rdp-cli/completions/`, with a host-build fallback for
      cross/aarch64-pc-windows-msvc targets) and `extra-archive-paths:
      completions` in the release caller.
  - Code: `.github/workflows/release.yml`
- [x] Add three completion assets to each of `[package.metadata.deb]` and
      `[package.metadata.generate-rpm]` in `crates/ff-rdp-cli/Cargo.toml`,
      preserving the intentional deb/rpm zsh-path asymmetry
      (`vendor-completions` vs `site-functions`) from the hyalo/hoppy
      precedent.
  - Code: `crates/ff-rdp-cli/Cargo.toml`
- [x] `.gitignore` entry for the generated `completions/` staging
      directories (top-level and crate-level) — neither was covered by an
      existing pattern.
  - Code: `.gitignore`
- [x] Local verification: build deb/rpm with `cargo deb`/`cargo
      generate-rpm` and confirm all three completion files land at the
      documented paths inside the `.deb`.
- [x] Validate with `actionlint`.
- [x] Run quality gates (`cargo fmt`, `cargo clippy --workspace
      --all-targets -- -D warnings`, `cargo test --workspace -q`).

## What's new

- **`ff-rdp completions <SHELL>`**: generates a shell completion script to
  stdout for bash, zsh, fish, elvish, or powershell (`clap_complete::Shell`).
  No JSON envelope — the raw script is meant to be `eval`'d or saved
  directly, e.g. `eval "$(ff-rdp completions zsh)"`.
- **No connection required**: like `doctor`/`manifest`/`install-skill`,
  `completions` never touches Firefox or the daemon — it is listed in the
  dispatcher's connection-free command set.
- **Packaging**: the release workflow's `pre-package-command` now runs
  `ff-rdp completions {bash,zsh,fish}` on every matrix target (via the
  built binary, or a `cargo run --release` host-build fallback when the
  target binary isn't runnable on the build host — `CROSS=true` or
  `aarch64-pc-windows-msvc` on the x86_64 `windows-latest` runner) and
  stages the output into `crates/ff-rdp-cli/completions/` (asset paths in
  `[package.metadata.*]` resolve relative to the crate directory, not the
  workspace root) and `extra-archive-paths: completions` for the tarball
  archives.
- **deb/rpm completion paths**: bash →
  `usr/share/bash-completion/completions/ff-rdp` (both); zsh → deb
  `usr/share/zsh/vendor-completions/_ff-rdp`, rpm
  `/usr/share/zsh/site-functions/_ff-rdp` (intentional asymmetry, copied
  verbatim from the hyalo/hoppy precedent — not a bug); fish →
  `usr/share/fish/vendor_completions.d/ff-rdp.fish` (both).

## Acceptance Criteria [11/11]

- [x] `Command::Completions` variant compiles with a house-style
      `long_about` and dispatches without requiring an RDP connection.
  - Test evidence: `crates/ff-rdp-cli/src/cli/args.rs` lines 1064-1085 (new
    variant); `crates/ff-rdp-cli/src/dispatch.rs` line 350
    (`Command::Completions { .. }` added to the `None` alternation) and
    line 1119 (`Command::Completions { shell } => commands::completions::run(*shell)`).
- [x] `cargo build --release -p ff-rdp-cli` succeeds and the binary prints
      non-empty completion scripts for bash/zsh/fish.
  - Test evidence: `target/release/ff-rdp completions bash|zsh|fish` all
    produced non-empty output (6788, 3866, 2277 lines respectively) during
    local verification.
- [x] e2e test `completions_each_supported_shell_produces_binary_name`
      covers every supported shell producing output that references the
      binary name; `completions_unknown_shell_fails_with_clap_parse_error`
      covers the clap parse-error path; `completions_requires_no_connection_flags`
      confirms no connection flags are needed.
  - Test evidence: `cargo test -p ff-rdp-cli --test e2e completions -q` — 3
    passed.
- [x] Unit test `generate_to_bash_produces_non_empty_output` (plus
      `generate_to_zsh_produces_non_empty_output`) covers the pure
      `generate_to` writer function.
  - Test evidence: `cargo test -p ff-rdp-cli --bin ff-rdp completions -q` —
    2 passed.
- [x] `.deb` package contains all three completion files at the documented
      paths.
  - Test evidence: `ar p target/debian/*.deb data.tar.xz | tar tJf -`
    listing includes `./usr/share/bash-completion/completions/ff-rdp`,
    `./usr/share/zsh/vendor-completions/_ff-rdp`,
    `./usr/share/fish/vendor_completions.d/ff-rdp.fish`.
- [x] `.rpm` package builds successfully with the new asset entries.
  - Test evidence: `cargo generate-rpm -p crates/ff-rdp-cli` exited 0 and
    produced `target/generate-rpm/ff-rdp-cli-0.3.0-1.aarch64.rpm` (no local
    `rpm`/`rpm2cpio` tooling on this macOS machine to list contents
    directly — the asset table shape mirrors the already-verified `.deb`
    table one-for-one).
- [x] `actionlint` passes with no findings (CI clean).
  - Test evidence: `/opt/homebrew/bin/actionlint
    .github/workflows/release.yml` exit 0, no output.
- [x] `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --workspace -q` all pass (CI clean).
- [x] Generated `completions/` staging directories are excluded from git and
      leave no untracked files after local packaging verification.
  - Test evidence: `.gitignore` entries `/completions/` and
    `crates/ff-rdp-cli/completions/`; `git status` clean after `rm -rf
    completions crates/ff-rdp-cli/completions target/debian
    target/generate-rpm`.
- [x] `.github/workflows/release.yml` diff is scoped to the new
      `pre-package-command`/`extra-archive-paths` inputs, placed before
      `dry-run`/`targets`, with `dry-run`/`targets` unchanged.
  - Test evidence: `git diff --stat iter-119/linux-packages --
    .github/workflows/release.yml` touches only that file; the `targets:`
    matrix block is byte-identical to iteration-119.
- [x] `check-iteration-ready` gate passes (CI clean) for this plan against
      the correct stacked base.
  - Test evidence: `FF_RDP_LIVE_TESTS=1 cargo run -p xtask --
      check-iteration-ready --plan kb/iterations/iteration-120-completions.md
      --base iter-119/linux-packages` exit 0.
