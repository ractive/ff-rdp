---
branch: iter-142/session-hygiene
date: 2026-08-09
depends_on:
  - kb/iterations/iteration-132-cli-polish.md
dogfood_path: |
  ff-rdp launch --headless --port 6100
  ff-rdp daemon stop --port 6100
  # → must not report a false "port still listening" error
  ff-rdp profiles prune
  # → must reclaim stale temp profiles (2.7GB observed)
  ff-rdp launch --headless --auto-consent --port 6100
  ff-rdp navigate https://www.bbc.com/news --port 6100
  # → non-Sourcepoint CMP: must either dismiss it or warn that it did not
first_call_sites: []
status: done
---

# Iteration 142: session hygiene — daemon stop, disk growth, consent coverage, eval edges

From [[dogfooding-session-63]]. Individually small, collectively the difference between a tool
that feels reliable and one that doesn't.

## Themes

### Theme A — `daemon stop` false-negative, 3/3 reproducible

```
{"error":"stopped Firefox (pid N) but port 6100 is still listening after 8 s —
 another process may be holding it..."}
```
Exits non-zero, yet `lsof -nP -iTCP:6100 -sTCP:LISTEN` shows the port free 2–3 s later. Two
defects: the readiness check gives up too early (or checks the wrong thing), and **the reported
pid is the daemon/proxy pid, not the `launch`-reported Firefox pid** — a second session saw
`"stopped Firefox (pid 45835)"` where 45835 was the daemon and Firefox 45581 stayed up.

Report the right pid, and make the port check reflect reality.

### Theme B — disk growth

- `~/Library/Application Support/ff-rdp/profiles/`: **62 temp profiles, 2.7 GB** after a day of
  use. `doctor` detects this and suggests `profiles prune`, but nothing prunes automatically.
- `~/.ff-rdp/daemon.*.throttle.json` accumulates for dead daemon pids (5 observed) — iter-132's
  GC deliberately leaves these alone, so they leak by the same pattern one iteration later.
- Legacy `daemon.spawn.lock` (no port in the name) is uncollectable: `parse_spawn_lock_port`
  requires `daemon.<u16>.spawn.lock`.
- iter-132's lock GC only runs on the daemon-spawn path, so a mostly-non-daemon workload never
  sweeps anything.

Decide a policy and apply it consistently: prune on launch, prune on stop, or age-based GC.
Cover throttle files and the legacy lock name.

### Theme C — `--auto-consent` silently fails on non-Sourcepoint CMPs

On BBC News, `launch --auto-consent` reported `"auto_consent": true` while the banner still
covered the page (verified in a screenshot). `consent accept` returned `{"cmp":null,"action":null}`
in both connection modes. The control was trivially findable —
`#bbccookies-continue-button` ("Yes, I agree") — and a plain `click` dismissed it instantly.

Two asks: add coverage for common non-Sourcepoint CMPs (BBC's is a good first case), and
**never report `auto_consent: true` when nothing was dismissed** — warn instead. iter-129
taught the tool to be honest about scroll lock; the same honesty belongs here.

Also: `--auto-consent` leaves a permanent `Consent-O-Matic Options` tab in every `tabs`
listing, which pollutes tab indices for anything targeting `--tab N`.

### Theme D — full-page screenshot duplicates the sticky header

`--full-page` on BBC News (1366×6598) contains a second complete header + nav bar at y≈3290,
overlapping article content. Verified by cropping at full resolution. The stitching pass
re-captures the sticky element at each scroll offset. Freeze sticky/fixed elements after the
first band, or capture in a single pass where possible.

### Theme E — `eval` wrapper edges

- The async-IIFE wrapper's heuristic is `;`-based, so ASI-separated statements leak the wrapper
  into the error: `await Promise.resolve(1)\n42` → `missing ) in parenthetical (lineNumber 3)`
  for a 2-line script. The user gets an unexplainable error pointing past the end of their input.
- Multi-statement completion-value semantics **invert** on the presence of `await`: without it
  the trailing expression is the result and `return` is a SyntaxError; with it the trailing
  expression is silently dropped (`{"type":"undefined"}`) and `return` is required. `--help`
  documents the rule, but silent `undefined` is the worst failure mode for an agent.

Make the wrapper robust to ASI, and make the await path either honor the trailing expression or
say why it can't — not return `undefined` silently.

**Adaptation from iter-141 review (output hygiene):** `eval.rs`'s own inline JS-exception
handler was changed by iter-141's review pass — a raw exception now returns
`Err(AppError::User(msg))` (routed through the standard JSON error envelope) instead of the old
`eprintln!("error: ...")` + `AppError::Exit(1)` that bypassed it entirely. Build any ASI/await
wrapper changes on top of that block, not the old bare-stderr version — `git log -p` on
`crates/ff-rdp-cli/src/commands/eval.rs` around the exception check if the shape looks
unfamiliar. The same envelope-bypass anti-pattern (bare `eprintln!` + `AppError::Exit(1)`, no
JSON on stdout at all) still exists in `click.rs` (`run`'s and the per-frame scan's "genuine JS
failure" paths, ~2 call sites) and `scroll.rs` (`run_until`'s timeout path) — same shape as the
bug iter-141 Theme E fixed in `eval_or_bail`/`poll_js_condition`/`eval.rs`, but out of scope for
iter-141 (no live-Firefox test coverage existed for those paths, and the comment above
`click_element_not_found_exits_nonzero` in `tests/e2e/click.rs` documents the stderr-vs-stdout
divergence as deliberate — verify that's still true before touching it). Worth sweeping here only
if Theme E's ASI/await work leaves runway; otherwise file a follow-up plan rather than rushing it
in.

### Theme F — reproducibility and diagnostics

- Console messages come back in the **system locale** (German observed), because the ephemeral
  profile inherits it. `launch` should pin `intl.accept_languages` so output is reproducible
  across machines.
- `wait` has no plain sleep form: `--time 6000` errors suggesting `--timeout`, but `--timeout`
  requires one of `--selector/--text/--eval/--ref`. Dogfooders shell out to `sleep`.
- `doctor` reports `binary_staleness: skipped — not in an ff-rdp checkout` while inside the
  checkout (it keys off cwd rather than the binary's path).
- `ff-rdp consent` with no subcommand lists subcommands; `ff-rdp scroll --to bottom` does not —
  inconsistent clap config.
- `dom --format text` hints suggested a selector that would have navigated away
  (`ff-rdp click "#bbccookies-prompt button, #cookiePrompt button, #cookiePrompt a"` — the third
  alternative is a policy link). Hints should prefer buttons over links.

## Acceptance Criteria [6/10]

- [x] live_142_daemon_stop_no_false_error: `daemon stop` after `launch` exits 0 and reports the
      Firefox pid that `launch` returned — see
      `crates/ff-rdp-cli/tests/live/live_142_daemon_stop_pid_honesty.rs`; root cause was
      `launch-record.json` being a single global file clobbered by concurrent ports, fixed by
      scoping it per port (`daemon_record.rs`)
- [x] live_142_profile_growth_bounded: repeated launch/stop cycles do not leave unbounded temp
      profiles — name the chosen policy in the test — see
      `crates/ff-rdp-cli/tests/live/live_142_disk_growth.rs::live_142_profile_growth_bounded`;
      policy: a dead-owner-pid profile is reclaimed by the very next `launch`, not gated by the
      7-day age threshold (`util/profile_dir.rs::prune_orphan_profiles`)
- [x] live_142_throttle_json_gc: throttle state files for dead pids are collected; live ones are not
      — see `crates/ff-rdp-cli/tests/live/live_142_disk_growth.rs::live_142_throttle_json_gc`
      (`daemon/throttle_state.rs::gc_stale_throttle_states`)
- [x] unit_legacy_spawn_lock_collected: the port-less `daemon.spawn.lock` name is collectable —
      see `crates/ff-rdp-cli/src/daemon/registry.rs::unit_legacy_spawn_lock_collected`
- [deferred — new plan: kb/iterations/iteration-144-session-hygiene-followup.md] live_142_auto_consent_honest:
      on a page whose CMP is not handled, `auto_consent` does not report success — a warning
      names what was found. `launch`'s `auto_consent` field is set from the CLI flag alone
      (`commands/launch.rs:603`) and `launch` returns before any page loads, so it structurally
      cannot attest to a real dismiss within this iteration's scope — see the follow-up plan's
      "Why these three were deferred" section
- [deferred — new plan: kb/iterations/iteration-144-session-hygiene-followup.md] live_142_bbc_cmp_dismissed:
      BBC's cookie banner is dismissed by `consent accept`
- [deferred — new plan: kb/iterations/iteration-144-session-hygiene-followup.md] live_142_full_page_no_duplicate_header:
      full-page capture of a sticky-header page has no repeated header band — deferred to avoid
      touching iteration-135's stitching fix without dedicated pixel-level before/after
      verification
- [x] e2e_eval_asi_await_script (`e2e_eval_asi_await_script` in `tests/e2e/eval.rs`,
      `live_142_eval_asi_await_script` in `tests/live/live_142_eval_asi_await.rs`): an
      ASI-separated await script parses without wrapper leakage — fixed in
      `commands/eval.rs::top_level_statement_boundaries` / `wrap_top_level_await`
- [x] e2e_wait_sleep_form: a plain duration wait exists and works — see `e2e_wait_sleep_form` in
      `tests/e2e/wait.rs` (`commands/wait.rs`, `sleep_ms` / legacy `--time` alias)
- [deferred — new plan: kb/iterations/iteration-144-session-hygiene-followup.md] live_142_console_locale_pinned:
      console output is locale-stable regardless of system locale. `launch`'s `USER_JS` already
      pins `intl.accept_languages`/`intl.locale.requested`/`intl.locale.matchOS` (since
      iter-61j) yet dogfooding session 63 still observed German output — the implementation
      environment for this iteration has no non-English-locale Firefox to reproduce with, so a
      fix cannot be verified here; see the follow-up plan

## Notes

- Themes are independent; if the iteration runs long, land A–D and defer E/F to a sibling plan
  rather than ticking ACs that were not verified.

## Run guidance (batch 138–142, from dogfooding session 63)

Non-negotiable working rules for whoever implements this plan:

1. **Do not trust the root cause stated above.** In iterations 135, 136 and 137 the real
   cause differed from the plan's hypothesis three times running, and twice it was our bug,
   not Firefox's. Reproduce the symptom and verify the mechanism **on the wire** (actual RDP
   packets / actual command output) before writing the fix. If the diagnosis here turns out
   to be wrong, fix the real cause and correct this section.
2. **A live test that passes `--no-daemon` proves nothing about the default path.** That is
   exactly how iter-129 shipped a feature that did not work at all. Every live test added
   here must exercise the default (daemon) path. iter-137 added the guard at
   `crates/ff-rdp-cli/tests/no_daemon_live_test_guard.rs` with a shrink-only grandfather
   list — **do not add entries to that list.**
3. Evidence for every finding in this plan — exact command and exact output — is in
   [[dogfooding-session-63]]. Read it before diagnosing.
