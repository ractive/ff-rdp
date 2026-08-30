---
title: "Iteration 212: ambient context — a content-first no-args view and a session hook that prints it"
type: iteration
date: 2026-08-29
status: done
branch: iter-212/ambient-context
depends_on:
  - 210
first_call_sites:
  - primitive: ff_rdp_cli::commands::home::run
    site: >-
      crates/ff-rdp-cli/src/main.rs (no-subcommand dispatch; also
      commands/install_hook.rs)
dogfood_path: |
  ff-rdp
  # no browser: exit 0, prints bin path, one-line description, daemon/port state, and the
  # three commands that get a browser up — NOT the clap usage dump (exit 2) it prints today
  ff-rdp launch --headless && ff-rdp navigate https://example.com
  ff-rdp
  # browser up: same header, then tabs (index, title, url) and the current tab's a11y summary
  # with refs (from iter-210), then ≤5 hints with literal syntax, e.g.
  #   -> ff-rdp click --ref e3
  #   -> ff-rdp page-text --query "<text>"
  ff-rdp --jq '.results.tabs | length'
  # JSON form of the same view, so the hook and scripts can consume it
  ff-rdp install-hook --claude --dry-run
  # prints the SessionStart hook entry it would add to ~/.claude/settings.json, using the
  # PATH-resolved binary name when it is this executable, else the absolute path
  ff-rdp install-hook --claude && ff-rdp install-hook --claude
  # second call: "already installed (no-op)", exit 0; settings.json byte-identical
  cargo run -p xtask -- check-skill-drift
  # expected: exit 0 — the committed SKILL.md matches the generator's output
tags:
  - iteration
  - cli
  - agent-ergonomics
  - skills
---

# Iteration 212: ambient context

In the axi.md benchmark ([[axi-benchmark-comparison]]) **all 84 runs on both sides spent
turn 1 on `--help`**, and ff-rdp's help is 2 000 tokens of it. Bare `ff-rdp` today prints the
clap usage and exits 2 — an agent that tries it learns nothing about the browser it already has.
The AXI principles this answers are "content first" (no-args shows live state) and "ambient
context" (a session hook injects that state at session start); axi's own CLI has neither turned
on in the benchmark, so this is a lead, not a catch-up.

A second, self-inflicted drift risk sits next to it: `crates/ff-rdp-cli/skills/ff-rdp-debug/SKILL.md`
is hand-written and nothing checks it against what the CLI actually does.

## Themes

- **A — Content-first home view.** `ff-rdp` with no subcommand prints identity + live state +
  hints; exit 0. JSON under `--jq`/`--format json`, text otherwise.
- **B — `install-hook`.** Opt-in `SessionStart` hook for Claude Code (and the same file shape for
  Codex / OpenCode where their hook formats allow), printing the Theme A view. Idempotent,
  path-repairing, `--dry-run`, `--uninstall`.
- **C — One source of truth for the skill.** The agent-facing guidance in the home view and
  `SKILL.md` come from the same generator; an xtask check fails CI when the committed skill is
  stale.

## Tasks

### A. Home view [4/4]
- [x] `commands/home.rs`: `{bin, description, version, daemon: {running, pid, port},
      browser: {reachable, firefox_version}, tabs: [{index, title, url, selected}],
      page: <a11y summary of the selected tab, with refs> | null, hints: [...]}`; wired from
      `main.rs` when no subcommand is given; exit 0 always (a missing browser is state, not an
      error)
- [x] Text renderer: `bin: ~/.cargo/bin/ff-rdp` (home collapsed to `~`), one description line,
      then the same tab table `tabs` prints, then the `a11y summary` text block, then `-> ` hint
      lines; total under ~60 lines on a typical page (the `page` block reuses `a11y summary`'s
      existing 50-entry cap)
- [x] Hints are state-dependent: no browser → `launch --headless`, `doctor`; browser but blank
      tab → `navigate <URL>`; page loaded → `click --ref <ref>`, `page-text --query "<text>"`,
      `snapshot --query "<text>"`; never more than 5, placeholders in `<>` for runtime values
- [x] `ff-rdp --help` unchanged; `ff-rdp help` unchanged; only the bare invocation changes

### B. `install-hook` [3/4]
- [x] `install-hook --claude [--project] [--dry-run] [--uninstall]`: adds a `SessionStart` entry
      to `~/.claude/settings.json` (or `<git-root>/.claude/settings.json`) whose command is the
      PATH-resolved binary name if `which ff-rdp` is this executable, else the absolute path;
      merges into existing `hooks` without touching other entries
- [x] Idempotent: re-running with the same resolved path is a no-op (exit 0, says so); a changed
      path is repaired in place; `--uninstall` removes only the entry it owns (marked with a
      `ff-rdp-managed` comment/key)
- [ ] `--codex` and `--opencode` variants following the AXI §7 file locations
      (`~/.codex/hooks.json` + `[features].hooks = true`; `~/.config/opencode/plugins/`), each
      behind the same `--dry-run`/`--uninstall` surface; if a target's format cannot express a
      session-start command, `install-hook` says so and exits 1 rather than writing something
      inert
- [x] Hook output is the Theme A text view with `--no-hints` off and the `page` block limited to
      headings + the first 15 interactive entries — this runs on every session, so it must stay
      small

### C. Skill from one source [2/2]
- [x] `commands/skill_doc.rs` generates the static part of `SKILL.md` (command groups, the
      quick-start, the `--with-page`/`--ref`/`--query` idioms) from the same tables the home view
      uses; `install-skill` installs the generated content; the committed
      `skills/ff-rdp-debug/SKILL.md` is regenerated by `cargo run -p xtask -- gen-skill`
- [x] `cargo run -p xtask -- check-skill-drift` diffs committed vs generated and fails on
      mismatch; wired into `.github/workflows/ci.yml`

## Acceptance Criteria [8/9]

- [x] `home_without_browser_exits_zero_and_names_launch` (unit, no daemon): `ff-rdp` → exit 0,
      `results.browser.reachable == false`, hints include `ff-rdp launch`
- [x] `live_home_with_page_lists_tabs_and_refs`: after `navigate <fixture>`, bare `ff-rdp`
      JSON has `results.tabs[0].url == <fixture>` and `results.page.interactive[0].ref`
      accepted by `click --ref`
- [x] `home_text_view_is_bounded` (unit, fixture page with 300 links): text output ≤ 80 lines
- [x] `install_hook_is_idempotent` (unit, `HOME` redirected to a temp dir): two installs produce a
      byte-identical `settings.json`; second run reports no-op
- [x] `install_hook_repairs_a_moved_binary_path` (unit): an existing entry with a stale absolute
      path is rewritten, other hooks untouched
- [x] `install_hook_uninstall_removes_only_its_entry` (unit)
- [x] `check_skill_drift_fails_on_stale_skill` (xtask unit): editing the committed `SKILL.md` by
      one line makes the check exit non-zero
- [ ] Benchmark: re-run [[axi-benchmark-comparison]] `--repeat 3` with the hook installed in the
      harness's `--settings`-equivalent (the harness passes `--setting-sources ""`, so this
      needs the hook output supplied via `--append-system-prompt` — document how in the Outcome
      section); ≥ 80% of ff-rdp runs make a browser command, not `--help`, their first tool call
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- **Exit code of bare `ff-rdp`.** 0. Today it is 2 (clap "missing subcommand"). Anything that
  scripted `ff-rdp` with no args and relied on exit 2 was relying on an error path; none exists in
  this repo. **Result of the sweep** (run before the change,
  `grep -rnE '(^|[^-[:alnum:]_/])ff-rdp[[:space:]]*$' kb tools crates`): every hit was prose ending
  in the product name ("… Gotchas for ff-rdp", "… agents pipe ff-rdp"), not an invocation. The one
  real consumer was `crates/ff-rdp-cli/tests/e2e/exit_codes.rs::exit_2_missing_subcommand`, which
  encoded the behaviour this iteration retires; it is replaced by the pair
  `exit_0_no_subcommand_is_the_home_view` + `exit_2_unknown_subcommand` so the usage-error path
  stays pinned.
- **Hints in JSON.** The home view is the one JSON payload that *does* carry `hints`: it is the
  orientation surface, and the hook consumes it. Every other command keeps hints out of JSON
  (agents read JSON through `--jq`/pipes).
- **`--version` fast path** is already fast (Rust binary, no lazy graph); nothing to do.
- **Why hook first, skill second.** AXI §7's ordering, and the measured cost: a skill loads on
  demand and still leaves the agent one `--help` turn short of the live state; the hook removes
  the turn. Both stay available; users install either.

## Out of scope

- Changing what `--help` prints. It is the reference; the home view is the orientation.
- Session-end capture (AXI's "lifecycle capture"). Nothing in ff-rdp needs cross-session memory
  yet.
- Turning hints on for other commands' JSON output — see [[axi-benchmark-comparison]] conclusion 6
  (dropped on review).

## Outcome

Shipped: Theme A whole, Theme B minus the third task, Theme C whole. Decision record:
[[decision-log]] DEC-050.

**What landed.** `crates/ff-rdp-cli/src/commands/home.rs` (the view, its hints, its bounded text
renderer), `commands/install_hook.rs` (`--claude`, `--project`, `--dry-run`, `--uninstall`),
`commands/skill_doc.rs` (the shared tables plus the `skill-doc` hidden subcommand), and
`crates/xtask/src/skill_drift.rs` (`gen-skill`, `check-skill-drift`, wired into the `discipline`
job in `.github/workflows/ci.yml`). Bare `ff-rdp` is dispatched by a *parse retry* in `main.rs`
(`parse_as_home`) rather than by making `Cli::command` optional, so `--help`, `help`, `ff-rdp
scroll` and every unknown-flag error keep their existing behaviour byte for byte.

Text is the home view's default output, not JSON: `format_is_explicit` reads argv, because clap
has already substituted its `"json"` default by the time the command sees `Cli::format`.
`--format json` and any `--jq` filter still get the envelope, which is what the dogfood path and
the hook consume.

**A `find_subcommand_token` gap, found during PR #232 review.** `VALUE_GLOBALS` (the allowlist
`parse_as_home` and `find_subcommand_token` use to skip past value-taking global flags) was
missing `--max-frame-mb` and `--redact-threshold`, both added earlier in this same iteration —
`ff-rdp --max-frame-mb 512` with no real subcommand mistook `512` for one and fell through to
clap's usage dump instead of the home view every other global flag gets. Fixed in this PR
(`crates/ff-rdp-cli/src/main.rs`, commits 50b1b11/a47a6ee) with regression tests covering both the
`find_subcommand_token`/`is_type_invocation` heuristic and `parse_as_home` directly.

Two counting guards from [[iteration-162b-ac-fidelity-shrink]] had to be updated, deliberately and
in place: `unit_162b_xtask_help_lists_eight` (renamed
`unit_162b_xtask_help_matches_the_pinned_list`, 8 → 10 subcommands) and
`ci_162b_discipline_job_two_xtask_steps` (renamed `…_xtask_steps_are_pinned`, 2 → 3 CI steps).
Those guards exist so growing the xtask surface is a deliberate edit rather than a drive-by, which
is exactly what happened here. 162b's own acceptance criteria are left as written — they were true
when that iteration closed, and editing a merged plan to match a later change is the reflex this
repo's discipline rules exist to stop.

**Two ACs are not ticked, and neither was reworded.**

1. *Theme B task 3 — `--codex` / `--opencode`.* Both exit 1 naming their file location and the
   reason, which is the shape the task itself prescribes for a target that cannot be written
   correctly — but the task asked for working variants, and these are not that. The reason is
   narrower than the task's own wording: it is not that the formats *cannot express* a
   session-start command, it is that neither entry schema could be **verified** from this build,
   and an entry with the wrong shape parses, never fires, and looks installed forever. OpenCode
   additionally wants a JavaScript plugin module, which CLAUDE.md's "all code stays in Rust" rule
   forbids shipping. `e2e_212_unsupported_targets_refuse_and_write_nothing` pins that neither
   target writes anything. Carried over — see below.

2. *The benchmark AC.* Not run. It needs a full `--repeat 3` re-run of the axi harness
   (~2 hours of live agent time and real API spend) that this iteration did not have, and the
   mechanism it depends on is still unsettled: the harness passes `--setting-sources ""`, so an
   installed `SessionStart` hook is *not loaded*, and the hook output would have to be injected
   through `--append-system-prompt` instead — which measures the hook's payload, not the hook.
   Filed as its own iteration rather than left as a half-answer here.

**Live sweep** (2026-08-30, macOS, gates `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1`, with a
raw `firefox -no-remote --start-debugger-server 6000 --headless` on port 6000):

```
LIVE_SWEEP_SUMMARY executed=300 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=300
```

300 passed / 0 failed, so `passed + failed == executed` reconciles exactly. All three
`live_212_ambient_context` tests ran and passed. `preexisting=0` because the port-6000 browser was
up before the sweep started, so the `ff-rdp-core` tier folded into `executed` rather than being
reported `ignored`.

**Live sweep.** The three live tests are
`crates/ff-rdp-cli/tests/live/live_212_ambient_context.rs`:
`live_home_with_page_lists_tabs_and_refs` (the AC — tabs name the loaded URL, the first `ref` is
accepted by `click --ref`, and the click actually navigates), `live_home_with_blank_tab_asks_for_a_navigate`,
and `live_home_hook_form_is_trimmed`.

**Carry-over**, all filed before this PR merges:

- [[iteration-217-hook-targets]] — `install-hook --codex` / `--opencode`, gated on pinning each
  target's entry schema against its published docs first.
- [[iteration-213-act-and-see-benchmark-rerun]] — the benchmark AC, folded in as its Theme D
  rather than filed as a third plan: it is the same harness, the same 42 tasks and the same money
  as the 210/211 re-run already queued there. That theme also carries the unsettled part — the
  harness passes `--setting-sources ""`, so it must decide and *record* whether it is measuring
  the hook or an `--append-system-prompt` paste of its output.
- [[iteration-218-home-view-single-connect]] — a fifth finding from PR #232's local review pass
  (a code-review subagent): `home.rs` opens two independent RDP connections per invocation
  (`browser_and_tabs`, then `page_block`) where one would do, which matters because the
  `SessionStart` hook runs this command on every agent session. Fixing it means adding a primitive
  to the shared `connect_tab.rs` module nearly every command depends on, which is more regression
  surface than a same-day review-fix pass on an already-green PR should take on — filed instead of
  fixed. The other four review findings (the `--with-page` idiom silently duplicating `--query`'s
  shape, a non-atomic `settings.json` write, unescaped shell metacharacters in a resolved binary
  path, and an undocumented type-coercion asymmetry in `apply_install`) were fixed directly in this
  PR; see the branch diff for `commands/skill_doc.rs`, `commands/install_hook.rs`.

## References

- [[axi-benchmark-comparison]] — the `--help`-first measurement and the hints decision
- [[iteration-210-act-and-see]] — the `a11y summary` refs the home view embeds
- `crates/ff-rdp-cli/src/main.rs:160` (no-subcommand handling), `commands/doctor.rs` (the
  probes the home view reuses), `commands/install_skill.rs` (the managed-file header and
  `--dry-run`/`--uninstall` surface to mirror), `skills/ff-rdp-debug/SKILL.md`
