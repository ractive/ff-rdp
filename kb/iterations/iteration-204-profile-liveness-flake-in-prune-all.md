---
title: "Iteration 204: `profiles prune --all` intermittently reports a live-owner profile as not live"
type: iteration
date: 2026-08-24
status: planned
branch: iter-204/profile-liveness-flake-in-prune-all
depends_on: [iteration-193-dogfood-scripts-pkill-and-path-binary]
first_call_sites: []
dogfood_path: |
  # Reproduce: launch a headless Firefox into an isolated profile root, back-date
  # the live profile past the age threshold, then ask --all to reclaim it. The
  # basename must appear in `removed_live`, because its owner is still running.
  export FF_RDP_HOME="$(mktemp -d)"
  cargo build --quiet -p ff-rdp-cli --bin ff-rdp
  B=./target/debug/ff-rdp
  J=$("$B" launch --headless --port 39111)
  P=$(echo "$J" | jq -r '.results.profile_path')
  PID=$(echo "$J" | jq -r '.results.pid')
  find "$P" -maxdepth 1 -exec touch -t "$(date -v-30d +%Y%m%d%H%M)" {} +
  "$B" profiles prune --older-than 1s | jq -c '.results'   # must NOT remove $P
  kill -0 "$PID" && echo "owner still alive"
  "$B" profiles prune --all | jq -c '.results.removed_live' # observed empty ~1 run in 2
  "$B" --port 39111 daemon stop; rm -rf "$FF_RDP_HOME"

  # Or drive the whole sequence through the iteration-97 gate, which asserts it:
  #   FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- check-dogfood-script \
  #     kb/iterations/iteration-97-profile-liveness-guard.md
tags: [iteration, profiles, flake, liveness]
---

# Iteration 204: the live-owner signal is not stable across two calls in one run

Carry-over from [[iteration-193-dogfood-scripts-pkill-and-path-binary]]'s close.

## The defect

Iteration 97's dogfood gate asserts three things in sequence against one launched Firefox:

1. `profiles prune --older-than 1s` must **skip** the live-owner profile (Theme B), and
2. `profiles prune --all` must **remove** it but list its basename in `removed_live` (Theme C).

Both read the same predicate, `profile_is_owned_by_live_process`. On 2026-08-24, running the
migrated iteration-97 gate live, Theme B passed and Theme C failed in the *same run*:

```
PASS: Theme B — live-owner profile survived age-gated prune
FAIL: Theme C — --all did not report ff-rdp-profile-1frdDIlOuyiQy3uI in removed_live
```

An immediate re-run of the identical command passed all themes, and a hand repro of the same
sequence outside the gate passed. Nothing changed in between. So within a few hundred
milliseconds the same profile read as live-owned and then as not-live-owned.

That predicate is what stops a running Firefox having its profile deleted out from under it.
Theme B is the direction that matters — an age-gated prune that reads a live owner as dead
deletes a live profile — and the observed failure was in Theme C only, but both call the same
function, so a flake in one is a flake in the other with the consequences swapped.

## What to find out first

`owner_liveness` (`crates/ff-rdp-cli/src/util/profile_dir.rs`) has four outcomes and three of
them can flip without the owner dying:

- `Dead` from `is_process_alive(pid)` returning false for a process that is alive but, say,
  momentarily a zombie during a Firefox content-process restart;
- `Dead` from `process_start_token(pid)` disagreeing with the recorded token — a *recycled PID*
  verdict, which is a hard "this is a different process";
- `Unverified` from `process_start_token` returning `None` (still counts as live, so this one
  cannot explain the observed failure);
- `Unmarked` if the marker file is unreadable at that instant — note the reproduction back-dates
  every top-level file in the profile with `touch` just before the two prune calls.

Instrument which branch fires before proposing a fix. The failure is intermittent, so a fix
justified by reasoning alone will not be distinguishable from the flake going quiet.

## Themes

- **A — Identify the flipping branch.** Add enough tracing (or a test-only accessor) to
  attribute a `false` from `profile_is_owned_by_live_process` to one of the four outcomes, and
  reproduce until the attribution is recorded.
- **B — Make the verdict stable, or make it honest.** Either the transient reading is wrong and
  should be retried/ignored, or it is a genuine "cannot tell" that must not be reported as a
  confident "not live" on the deletion path — `Unverified` already errs toward keeping the
  directory, and whatever branch fires here may belong with it.

## Tasks

### A. Attribution [0/2]
- [ ] `profile_is_owned_by_live_process` can report *which* `OwnerLiveness` it derived
- [ ] The intermittent failure is reproduced with the branch recorded

### B. Stability [0/1]
- [ ] The identified branch either stops firing spuriously or stops being treated as a
      confident negative on the prune path

## Acceptance Criteria [0/3]

- [ ] A test pins the identified transient case: given that condition, an age-gated prune does
      **not** remove the profile
- [ ] The iteration-97 dogfood gate passes ten consecutive runs
      (`FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- check-dogfood-script kb/iterations/iteration-97-*.md`)
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Out of scope

The dogfood-script harness itself — [[iteration-193-dogfood-scripts-pkill-and-path-binary]] owns
it and has landed. If the investigation shows the assertion rather than the predicate is wrong,
say so and fix the assertion; do not reword it to match whatever the run produced.

## References

- [[iteration-193-dogfood-scripts-pkill-and-path-binary]] — where this was observed, and why the
  iteration-97 gate could be executed live at all
- `crates/ff-rdp-cli/src/util/profile_dir.rs` — `owner_liveness`,
  `profile_is_owned_by_live_process`
- `crates/ff-rdp-cli/src/commands/profiles.rs` — `select_prune_targets`, `removed_live`

## Additional observation — `daemon stop` leaves the profile behind (iter-224 close, 2026-08-31)

A second reclamation path shows the same "unstable across one run" shape, so it belongs to this
plan rather than a new one. `live_96_profile_cleanup::live_daemon_stop_profile_path_matches_launch_json`
failed in **both** live sweeps taken on `iter-224/with-page-daemon-connection-reset`:

```text
profile_removed must be true — got
  {"stopped":true,"pid":18379,"port":57624,"profile_removed":false,"profile_removed_path":null}
```

`stopped: true` means the escalation ladder reported the process gone and the port free, so
`stop_daemon_and_build_result_with` did call `cleanup_profile_dir` — and got no removed path back.
It **passed alone** (`--test-threads=1`, 2 passed in 2.86 s) immediately after sweep 1. Same
predicate family as Theme B/C above (`profile_is_owned_by_live_process` guards
`cleanup_profile_dir`'s refusal), same "true once, false once, one run apart" signature, so whatever
makes the liveness read unstable is the thing to find. Worth checking whether a content process
still holding the profile dir open is enough to make the removal fail silently — the JSON reports
`profile_removed: false` with no reason attached, which is its own honesty gap.

Carried over from [[iteration-224-with-page-daemon-connection-reset]]; nothing in that iteration
touches profile reclamation.
