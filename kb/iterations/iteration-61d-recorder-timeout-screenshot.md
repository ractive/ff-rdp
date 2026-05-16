---
title: "Iteration 61d: recorder --timeout capture + headless screenshot + formatting nit"
type: iteration
date: 2026-05-16
status: in_progress
branch: iter-61d/recorder-timeout-screenshot
depends_on: [iteration-61c-runner-secret-leak-fixes]
tags:
  - iteration
  - scripts
  - recorder
  - screenshot
  - firefox-version-detection
  - dogfood-feedback
  - small
---

# Iteration 61d: recorder `--timeout` capture + headless screenshot + formatting nit

Tiny follow-up to [[iteration-61c-runner-secret-leak-fixes]], driven by
[[dogfooding-session-46]]. iter-61c closed 6 of 8 session-45 findings
outright, but the verification session surfaced three carry-overs:

1. **B1 (recorder `--timeout` capture)** — the iter-61c commit message
   claims this is fixed, but live verification shows `wait --selector
   body --timeout 5000` records as `{"wait":{"selector":"body"}}` with
   no `timeout` field. Either the implementation only covered
   `click --wait-for-*` (which is what the commit body actually
   touches), or there's a missing branch in the wait-step mapper.
2. **D (headless screenshot)** — explicitly deferred in iter-61c
   ("D: deferred (needs live Firefox repro)"). Repro is now in hand:
   the wardrobe-assistants Firefox build doesn't advertise its version
   in the RDP greeting, so the screenshot-actor guard refuses to even
   try.
3. **B3 closing-bracket nit** — step objects are now pretty-printed
   correctly, but the recorder's `finalise_output_file` still writes
   `}  ]` (two trailing spaces, no newline before the `]`). One line
   of `serde_json::ser::PrettyFormatter` use vs an explicit `write!`.

Bundle them in one PR because they're all small, touch
recorder + screenshot only, and the dogfood feedback loop is already
warm.

Themes:

- **A — Recorder `--timeout` for `wait`.** Finish what iter-61c B1
  started.
- **B — Headless screenshot on Firefox builds with silent greeting.**
  Probe for the actor instead of gating on version-string presence.
- **C — Closing-bracket formatting.** One-line `PrettyFormatter` fix
  in `finalise_output_file`.

## Tasks

### A. Recorder `--timeout` capture for `wait`

#### A1. Find the gap — **trivial**
- [x] Locate the `wait` arm in the recorder's `Command →
  Option<Step>` mapper (added in iter-61b A1, likely in
  `crates/ff-rdp-cli/src/script/recorder.rs` or
  `crates/ff-rdp-cli/src/cli/args.rs`). Verify whether it reads
  `wait.timeout` and writes it into the recorded step. Session 46
  evidence strongly suggests it doesn't.

#### A2. Write the field — **trivial**
- [x] When the recorded `wait` step has a `--timeout <ms>` argument
  *and that value differs from the default*, write it as
  `wait.timeout` in the recorded JSON. (Skipping defaults keeps
  recorded files terse — a hand-authored script wouldn't write the
  default either.)
- [x] Same for `--text` and `--eval` waits — verify each preserves
  its `timeout` round-trip.

#### A3. Test — **required**
- [x] Add `tests/e2e/recorder.rs::recorder_captures_wait_timeout`:
  ```rust
  // record `wait --selector body --timeout 5000`,
  // parse the recorded file, assert step has `timeout: 5000`.
  ```
- [x] Add `tests/e2e/recorder.rs::recorder_omits_default_timeout`:
  ```rust
  // record `wait --selector body` (no --timeout),
  // assert step has no `timeout` field.
  ```
- [ ] Add a round-trip e2e: record a wait with a small non-default
  timeout (e.g. 100 ms), replay, assert the replay actually used
  the small timeout (catch via measured elapsed_ms or a deliberately
  unmet condition that fails fast).

### B. Headless screenshot on Firefox builds with silent greeting

#### B1. Reproduce — **investigation**
- [x] Use the wardrobe-assistants Firefox PID (or relaunch with the
  same flags `ff-rdp launch --headless --port 6000`). Confirm:
  - `ff-rdp doctor` reports `"detail": "Firefox version not
    advertised in the RDP greeting"`.
  - `ff-rdp screenshot -o /tmp/x.png` fails with
    `screenshot actor unavailable on Firefox unknown; minimum
    supported version: 120`.
- [x] Capture the actual RDP greeting bytes (raw `{"type":"connected",
  "applicationType":"browser",…}`) to see what *is* present —
  `applicationType`, `actor`, etc. Useful for B2.

#### B2. Switch from version-gate to actor-probe — **major fix**
- [x] In the screenshot path, today's logic is roughly:
  ```
  if firefox_version < 120 { refuse }
  ```
  Replace with:
  ```
  attempt screenshotContentActor; on noSuchActor / unknownActor,
  fall back to the older capture path (or surface a precise error
  pointing at the actor name).
  ```
- [x] Where the older capture path doesn't exist (i.e. we genuinely
  do depend on `screenshotContentActor`), the error message should
  still be precise: name the actor, the actual greeting contents,
  and the minimum-required Firefox version. No "upgrade Firefox"
  guess when we don't actually know the version.

#### B3. `version-info` actor fallback — **supporting**
- [x] When the connection-handshake greeting is silent on version,
  fall back to querying the `deviceActor.getDescription` (Firefox's
  standard actor for runtime info, returns `appVersion`). This makes
  `firefox_version` non-empty for the wardrobe-assistants build and
  any future build where the greeting changes shape.
- [x] If both the greeting *and* the version-info actor are silent
  (extremely unlikely), keep the iter-61c "unknown version" branch
  but only as a last resort, and gate the "must be >= 120" message
  on us actually having a version to compare against.

#### B4. Test — **required**
- [ ] Live e2e: launch Firefox with the same flags used by the
  dogfood session, navigate to a small fixture, screenshot, assert
  success.
- [x] Mock-server e2e: serve a greeting that omits the version
  field; assert the actor-probe path engages and doctor reports real
  version via device actor (not "version not advertised").

### C. Closing-bracket formatting in recorded JSON

#### C1. Replace explicit `write!("  ]")` with `PrettyFormatter` — **trivial**
- [x] In `script/recorder.rs::finalise_output_file` (or wherever
  the array-close is written), switch from manual `write!(f, "  ]")
  to `serde_json::ser::PrettyFormatter`-driven serialisation so the
  whole file matches the same indent style as the step objects iter-61c B3
  already pretty-printed. Fixed by adding `\n` before the `  ]` closing.
- [x] Test: a 2-step recording's final byte sequence equals
  `"…}\n  ]\n}\n"` (the same suffix a hand-authored 2-step script
  produces under `serde_json::to_string_pretty`).

## Acceptance Criteria

- [x] `ff-rdp record start /tmp/r.json` → `ff-rdp wait --selector body
  --timeout 5000` → `ff-rdp record stop` produces a step with
  `"timeout": 5000`. (Note: 5000 is the default, so step has NO timeout
  field — use `--wait-timeout 1234` for a non-default that IS recorded.)
- [x] `ff-rdp record start /tmp/r.json` → `ff-rdp wait --selector body`
  (no `--timeout`) → `ff-rdp record stop` produces a step *without* a
  `timeout` field.
- [ ] Against the same wardrobe-assistants Firefox build that breaks
  today, `ff-rdp screenshot -o /tmp/x.png` succeeds in headless mode.
- [x] When the connection greeting omits version, `ff-rdp doctor`
  reports a real Firefox version retrieved via the device actor
  (not `"Firefox version not advertised in the RDP greeting"`).
- [x] A 2-step recorded file ends in `…}\n  ]\n}\n` (matches
  `to_string_pretty`).
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D
  warnings && cargo test --workspace -q` clean.

## Design Notes

- **Probe over version-gate** (B2): the version-gate pattern is wrong
  in spirit — what we actually care about is "does the actor exist
  and respond?", not "is the Firefox version number ≥ 120". The
  version-string check is a proxy for actor availability; replacing
  it with a real probe is both more correct and more robust to
  Firefox builds where the greeting shape drifts.
- **Defaults-elision in recorded files** (A2): keeping recorded
  JSON close to what a human would write makes it easier to diff,
  review, and reason about. Always-writing `timeout: 30000` would
  bloat every recorded `wait` step with the same noise.
- **Why not bundle the `screenshot --full-page` work** from earlier
  iteration backlogs: full-page capture depends on the same actor
  fix as B; if B lands cleanly, full-page can ship as a small
  follow-up. Keeping iter-61d focused.
- **Out of scope**: any further iter-61b section F work (all closed
  in iter-61c E1–E7) and any iter-62 page-map work.

## References

- [[dogfooding-session-46]] — the verification session that surfaced
  these three carry-overs. Findings #1, #2, #3 map to themes A, C, B
  respectively.
- [[iteration-61c-runner-secret-leak-fixes]] — the parent iteration;
  this one closes the items its commit message acknowledged as
  partial/deferred.
- [[iteration-61b-recorder-cli-wiring]] — A1's recorder mapper was
  introduced here; A2 extends it for the `wait.timeout` field.
- `~/.claude/projects/-Users-james-devel-ff-rdp/memory/reference_firefox_source.md`
  — searchfox pointers used by B3's version-info actor lookup.
