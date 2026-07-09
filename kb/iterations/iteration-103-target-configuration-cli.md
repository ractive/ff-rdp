---
title: "Iteration 103: emulate command — expose the target-configuration actor (UA, color-scheme, DPR, print, touch, JS, offline, cache)"
type: iteration
date: 2026-07-09
status: in-progress
branch: iter-103/emulate-target-configuration
depends_on: []
firefox_refs:
  - lines: 14-29
    path: devtools/shared/specs/target-configuration.js
    why: >-
      SUPPORTED_OPTIONS dict — the full set of configuration fields the
      TargetConfigurationActor accepts (cacheDisabled, colorSchemeSimulation,
      customUserAgent, javascriptEnabled, overrideDPPX,
      printSimulationEnabled, setTabOffline, touchEventsOverride, …).
kb_refs:
  - kb/research/deep-review-2026-07-fable5.md
first_call_sites:
  - primitive: >-
      emulate CLI command (clap subcommand + JSON envelope) driving
      TargetConfigurationFront::update_configuration
    site: crates/ff-rdp-cli/src/commands/emulate.rs
  - primitive: >-
      TargetConfigurationFront setters beyond cache/color-scheme (user agent,
      dppx, print, touch, javascript, offline)
    site: crates/ff-rdp-core/src/fronts/target_configuration.rs
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate https://example.com
  ff-rdp emulate --color-scheme dark --user-agent "ff-rdp-test/1.0"
  ff-rdp eval 'matchMedia("(prefers-color-scheme: dark)").matches'   # → true
  ff-rdp eval 'navigator.userAgent'                                  # → ff-rdp-test/1.0
  ff-rdp emulate --reset
  ff-rdp eval 'matchMedia("(prefers-color-scheme: dark)").matches'   # → false (system)
tags: [iteration, emulate, target-configuration, cli, review-2026-07]
---

# Iteration 103: `emulate` — expose the target-configuration actor

## CI-wait policy (2026-07-09, per James)

When waiting on PR checks before merging, wait ONLY for the required lanes:
fmt, clippy, discipline, supply-chain, fuzz, test (ubuntu-latest),
test (macos-latest), verify-attestation. Do NOT wait for or block on:
- `live-tests` — advisory by design (continue-on-error); failures belong to
  [[iteration-106-live-test-masking-cascade]] / [[iteration-107-post-105-live-sweep]].
- `test (windows-latest)` — known-red with 5 pre-existing failures tracked in
  [[iteration-108-windows-ci-preexisting-reds]]. Do glance at its failure
  list once: if it shows failures OTHER than those 5, that IS a regression —
  stop and fix before merging.

## Live-test policy (2026-07-09, per James)

Do NOT run the full live Firefox suite (`cargo test-live`, or `--test live --
--include-ignored` without a filter) during this iteration — neither while
implementing nor while reviewing. Run ONLY (1) the specific live tests this
plan's ACs name, filtered (e.g. `cargo test -p ff-rdp-cli --test live
<filter> -- --include-ignored`), and (2) this iteration's dogfood script
(required by check-iteration-ready). Full-suite validation is deferred to
[[iteration-107-post-105-live-sweep]], which runs once after iteration 105
merges and fixes all fallout there.

The cheapest high-value gap from the 2026-07 review
([[deep-review-2026-07-fable5]]): the `TargetConfigurationFront` **already
exists** in core (`fronts/target_configuration.rs`, with
`set_cache_disabled`/`set_color_scheme` and live coverage in `live_61u.rs`)
but has **zero CLI consumers** — the classic dead-primitive shape the
project's own gates exist to catch. One `updateConfiguration` call unlocks
server-side emulation an agent-facing CLI badly wants: custom user agent,
`prefers-color-scheme` simulation, device-pixel-ratio override, print-media
simulation, touch-event override, JavaScript-disabled testing, tab-offline
(PWA/offline UX), and cache-disabled (cold-load perf). This also gives
[[iteration-98-media-query-truthfulness]]-style responsive work a
server-side lever, and closes several rows of the firefox-devtools-mcp
comparison (its `set_viewport`/emulation features) without touching BiDi.

## Themes

- **A — Front completion.** Extend the existing front to the full
  SUPPORTED_OPTIONS surface.
- **B — CLI command.** `ff-rdp emulate` with one flag per option, `--reset`,
  and honest lifetime semantics (config lives with the RDP connection).
- **C — Live proof.** Each option asserted by an in-page probe.

## Tasks

### A. Front completion [2/2]
- [x] Extend `TargetConfigurationFront` with the remaining supported fields:
      `customUserAgent`, `overrideDPPX`, `printSimulationEnabled`,
      `touchEventsOverride`, `javascriptEnabled`, `setTabOffline` (all
      nullable — patch only what the user set, per the spec dict).
      Implemented as `set_custom_user_agent`, `set_override_dppx`,
      `set_print_simulation_enabled`, `set_touch_events_override`,
      `set_javascript_enabled`, `set_tab_offline` in
      `fronts/target_configuration.rs`. Also fixed the pre-existing
      `colorScheme` → `colorSchemeSimulation` wire-name bug.
- [x] Support a reset call (send defaults / restore) for `--reset`:
      `TargetConfigurationFront::reset` sends every documented default in one
      request (unit test `reset_sends_all_defaults`).

### B. CLI command [3/3]
- [x] Add `ff-rdp emulate` (clap): `--user-agent <s>`,
      `--color-scheme light|dark|none`, `--dppx <f>`, `--print on|off`,
      `--touch on|off`, `--js on|off`, `--offline on|off`,
      `--cache on|off`, `--reset`; JSON envelope echoes the applied
      configuration. See `commands/emulate.rs` + `EmulateArgs`.
- [x] Lifetime honesty: `ONE_SHOT_LIFETIME_WARNING` is attached to the
      envelope (`results.lifetime_warning`) on the `--no-daemon` one-shot path
      and omitted on the daemon path (asserted by
      `e2e_emulate_one_shot_lifetime_warning`).
- [x] Wire into `--help` (`long_about` + `AFTER_LONG_HELP` reference block),
      the command dispatch table (`Command::Emulate`), and a
      `kb/rdp/actors/target-configuration.md` note (linked from the actors
      README).

### C. Live proof [2/2]
- [x] Land the live tests listed in the ACs (one probe per option; JS-off
      and offline reload between set and probe) — `tests/live/live_103_emulate.rs`.
- [x] Extend the dogfood flow (see `dogfood_path`) — the `emulate --color-scheme`
      / `--user-agent` / `--reset` sequence is exercised by
      `live_emulate_color_scheme_dark` + `live_emulate_user_agent`.

## Acceptance Criteria [7/7]

- [x] live_emulate_color_scheme_dark: after `emulate --color-scheme dark`,
      `matchMedia("(prefers-color-scheme: dark)").matches` evaluates true;
      after `--reset` it reverts.
- [x] live_emulate_user_agent: `navigator.userAgent` equals the override
      string after `emulate --user-agent`.
- [x] live_emulate_dppx: `devicePixelRatio` equals the `--dppx` override.
- [x] live_emulate_js_disabled: with `--js off` + reload, an inline
      script's DOM side-effect is absent; with `--js on` + reload it returns.
- [x] live_emulate_offline: with `--offline on`, `navigator.onLine === false`;
      restored after `--offline off`.
- [x] `e2e_emulate_one_shot_lifetime_warning`: `emulate --no-daemon …` envelope
      carries the connection-lifetime warning (`ONE_SHOT_LIFETIME_WARNING`);
      daemon-path envelope does not.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- Note (iter-102 carry-over): `crates/ff-rdp-core/tests/no_string_actor_ids.rs`
  now has `unit_no_production_expect_in_core`, a source-scan gate that fails
  the build on any `.expect(` in non-test `ff-rdp-core/src/` code. New
  `TargetConfigurationFront` setters and the `emulate` command's core-side
  plumbing must use `?`/`ok_or_else`/typed errors from the start — don't reach
  for `.expect()` during development and clean it up later, the gate will
  catch it in CI anyway. (`ff-rdp-cli` is not covered by this gate, but the
  project's general "no `.unwrap()`/`.expect()` outside tests" rule still
  applies there.)
- The actor is obtained via `watcher.getTargetConfigurationActor()` — the
  plumbing already exists for the two implemented setters; this is field
  completion, not new protocol work. All fields are nullable in the spec
  dict, so partial updates are the natural CLI semantics.
- `print` simulation composes with `screenshot` for print-stylesheet audits —
  worth one line in the command docs, no extra code.
- Command name: `emulate` (not `config`) — it configures the *page
  environment*, not ff-rdp itself, and matches the vocabulary agents know
  from other tools.

## Out of scope

- True viewport sizing (no RDP actor exists — [[project_viewport_protocol]]
  constraint stands; `responsive` remains the width tool).
- Geolocation/locale/timezone overrides (not in SUPPORTED_OPTIONS).
- Persisting emulation across daemon restarts.

## References

- [[deep-review-2026-07-fable5]] — gap C1 (top-ranked, effort S).
- `crates/ff-rdp-core/src/fronts/target_configuration.rs` — the existing
  front this iteration finally consumes.
- `crates/ff-rdp-core/tests/live_61u.rs` — existing live coverage to extend.
