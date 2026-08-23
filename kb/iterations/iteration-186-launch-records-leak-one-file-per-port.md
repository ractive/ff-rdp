---
title: "Iteration 186: launch records leak one file per port, and lazy reaping keyed on a random port never fires"
type: iteration
date: 2026-08-23
status: done
branch: iter-186/launch-records-leak-per-port
depends_on: []
first_call_sites: []
dogfood_path: |
  # Unbounded disk growth in the product's own state dir. Measured, not inferred.
  
  # 1. See the leak. 4040 files / 17 MB on the dev machine, 2026-08-23,
  #    spanning Aug 12 -> Aug 22. Every one is 212 bytes.
  ls ~/.ff-rdp | sed -E 's/[0-9]+/N/g' | sort | uniq -c | sort -rn
  #    expected today: 4040 launch-record.N.json, 1 daemon.log
  du -sh ~/.ff-rdp
  
  # 2. Confirm they are ALL stale — i.e. no live daemon owns any of them.
  #    Every record carries a pid; none of those pids should still exist.
  for f in ~/.ff-rdp/launch-record.*.json; do
    p=$(sed -E 's/.*"pid":([0-9]+).*/\1/' "$f" 2>/dev/null)
    kill -0 "$p" 2>/dev/null && echo "LIVE: $f pid=$p"
  done | head
  #    expected: no output. If a line appears, that record is legitimately held.
  
  # 3. Read the two mechanisms that were supposed to reclaim them, and see
  #    why neither does:
  #    crates/ff-rdp-cli/src/daemon_record.rs:170  remove_in()  — clean stop only
  #    crates/ff-rdp-cli/src/daemon_record.rs:120  read_in()    — reaps a stale
  #                                                 record, but only when that
  #                                                 exact port is read again
  #    crates/ff-rdp-cli/src/daemon/registry.rs:464 gc_stale_spawn_locks_in()
  #                                                 — sweeps the REGISTRY dir,
  #                                                 which is not where launch
  #                                                 records live
  
  # 4. Prove the reaper never fires, which is the actual defect. Ports come
  #    from an ephemeral bind(:0), so a given port recurs essentially never,
  #    and a reaper keyed on "someone reads this port again" is a no-op.
  ls ~/.ff-rdp | sed -E 's/launch-record\.([0-9]+)\.json/\1/' | sort -n | uniq -d | wc -l
  #    a low count here = ports almost never repeat = lazy reaping cannot work
  
  # 5. After the fix, a launch must leave the dir bounded:
  cargo run -p ff-rdp-cli -- launch --headless --auto-consent
  ls ~/.ff-rdp/launch-record.*.json | wc -l   # must not grow without bound
tags:
  - iteration
  - disk-growth
  - daemon
  - cleanup
  - gc
---

# Iteration 186: give launch records the GC that throttle files already have

## The defect

`~/.ff-rdp/launch-record.<port>.json` accumulates one 212-byte file per launch and nothing
reclaims it. Measured 2026-08-23: **4040 files, 17 MB**, oldest 2026-08-12, newest 2026-08-22 —
ten days of ordinary test and dogfood traffic on one machine.

Two reclamation paths exist and neither covers this case:

- `daemon_record::remove_in` (`daemon_record.rs:170`) deletes the record on a **clean** daemon
  stop. A killed, crashed, or harness-abandoned daemon never reaches it — and the live suite
  kills daemons routinely.
- `daemon_record::read_in` (`daemon_record.rs:120`) deletes a record whose pid is dead, but only
  as a side effect of **reading that same port again**.

The second is the interesting one, and it is the reason this went unnoticed for ten days:
**the reaper is keyed on a value that never recurs.** Ports are handed out by an ephemeral
`bind(:0)`. The chance that a later run asks for the exact port a dead record is filed under is
negligible, so "reap it lazily on next read" means "never". A lazy reaper keyed on a random
identifier is not a slow reaper; it is not a reaper.

## Why this is a regression of a fix, not an oversight

Before [[iteration-142]] this was a single global `launch-record.json`. Two concurrent daemons
clobbered each other's record, so 142 split it per-port (`daemon_record.rs:18`). That fixed the
clobber and introduced the leak: one shared file that gets overwritten cannot grow, and N
per-port files with no sweep can only grow.

142 clearly understood the shape of this problem — it added a dedicated GC for stale
`daemon.<port>.throttle.json` files, wired into every `launch`, precisely because
`gc_stale_spawn_locks_in` skipped them. See `live_142_disk_growth.rs`'s module docs. Launch
records simply were not on the list, and they live in `record_base_dir()` rather than the
registry dir that `gc_stale_spawn_locks_in` (`registry.rs:464`) sweeps.

So the fix has a working precedent in the same codebase, from the same iteration that created
the leak. This is not new machinery.

## Themes

- **A — Sweep launch records at launch.** Mirror what 142 did for throttle files: on every
  `launch`, sweep `record_base_dir()` for `launch-record.*.json` whose recorded pid is dead and
  remove them. Bounded work, runs where the leak is created.
- **B — Decide what "stale" means, and say so.** A dead pid is the obvious test, but pids are
  recycled. 142's own reasoning for profiles ("a dead-owner profile is reclaimed immediately
  regardless of age") is the precedent to follow or to consciously depart from. Record which,
  and why — a wrong answer here deletes a live daemon's record.
- **C — Prove it stays bounded.** A test that launches N times and asserts the file count does
  not grow with N is the only thing that stops this recurring a third time.

## Tasks

### A. The sweep [3/3]
- [x] A GC pass removes `launch-record.*.json` whose owning pid is dead
- [x] It is wired into `launch`, matching how 142 wired the throttle-file GC
- [x] It sweeps `record_base_dir()`, and honours `FF_RDP_HOME` the way `daemon_record` already
      does — note that `secure_profile_root()` does **not**, which
      [[iteration-188-live-sweep-cost-and-parallelism]] records as an inconsistency

### B. Staleness [2/2]
- [x] The staleness test is written down with its pid-recycling risk stated
- [x] A record whose pid is alive is never removed — unit test with a live pid

### C. Bounded growth [2/2]
- [x] A test launches repeatedly and asserts the record count does not grow with launch count
- [x] The existing 4040 files are reclaimed by the first run of the new sweep, not left to be
      cleaned by hand

## Acceptance Criteria [3/3]

- [x] `ls ~/.ff-rdp/launch-record.*.json | wc -l` does not grow across repeated launches, shown
      by a recorded before/after
- [x] A live daemon's record survives a sweep that runs while it is up
- [x] The reason lazy-on-read reaping never fired is written down in the code that replaces it,
      so the next person does not reintroduce a reaper keyed on a random port

## Outcome

Implemented on `iter-186/launch-records-leak-per-port`.

### The sweep

`daemon_record::gc_stale_launch_records_in` (`crates/ff-rdp-cli/src/daemon_record.rs`) reads
`record_base_dir()`, matches only the exact `launch-record.<PORT>.json` shape, parses the record,
and removes it when `process::is_process_alive(rec.pid)` is false. Wired into
`commands::launch::run` on the same line block as iter-142's three existing sweeps
(`gc_stale_spawn_locks`, `gc_legacy_spawn_lock`, `gc_stale_throttle_states`), with the same
best-effort contract: every error swallowed, never a reason a launch fails.

Note the filename guard is what keeps the sweep safe: `launch-record.<port>.json.tmp` — the file
`write_in` renames from, i.e. a write in flight *right now* — does not match, and neither does the
pre-142 port-less `launch-record.json`.

### Staleness, and why pid liveness alone is safe here (Theme B)

Stale == recorded pid not alive. No age gate, following iter-142's precedent for profiles ("a
dead-owner profile is reclaimed immediately regardless of age"): an age gate is exactly what stops
a same-day workload from ever being reclaimed, which is the mistake 142 already paid for.

The plan's `dogfood_path` step 2 predicted "expected: no output" from the liveness loop — that
prediction was wrong, and the way it was wrong is the useful part. Pid recycling was **observed,
not hypothesised**. Of the 4803 records on the dev machine, 13 had a
pid that `kill -0` said was alive; spot-checking four of them found one real Firefox
`plugin-container` and three unrelated processes that had inherited the pid (a `zsh`, an
`extensionkitservice`, a `SafariPlatformSupport.Helper`). That is the harmless direction: a stale
record looks live, survives this sweep, and is collected by a later one — cost is one 212-byte file
for one more launch. The dangerous direction (removing a *live* instance's record) would require
`is_process_alive` to report false for a running process, which it does not do. The asymmetry is
written into the code's section comment, alongside the reason a port-keyed reaper could never fire.

### Measured before/after (AC 1, Task C second box)

Real `~/.ff-rdp` on the dev machine, 2026-08-23, via `cargo run -p ff-rdp-cli -- launch --headless
--auto-consent`:

| point | `launch-record.*.json` | `du -sh ~/.ff-rdp` |
|---|---|---|
| before (plan written) | 4040 | 17 MB |
| before (implementation) | **4803** | **20 MB** |
| after one launch carrying the sweep | **14** | **1.0 MB** |
| after `daemon stop` | 13 | — |
| after three further launch/stop cycles | 13, 13, 13 | — |

The backlog was reclaimed by the first run, not by hand. The residual 13 are the recycled-pid
records described above; they drain as those pids die (the count had already fallen to 12 by the
end of the cycles).

### Tests

- `daemon_record.rs` unit tests: `unit_gc_stale_launch_records_removes_dead_keeps_live` (live pid
  == this test process, so Theme B's second box is asserted against a genuinely running process),
  `unit_gc_stale_launch_records_ignores_other_files`,
  `unit_gc_stale_launch_records_leaves_corrupt_record_alone`,
  `unit_gc_stale_launch_records_tolerates_missing_dir`,
  `unit_parse_launch_record_port_matches_only_exact_shape`, and
  `unit_launch_record_count_stays_bounded_across_repeated_launches` — 50 simulated launches on 50
  *distinct* ports (the ephemeral-`bind(:0)` condition under which lazy-on-read reaping collects
  nothing) must leave 2 files, not 51.
- `crates/ff-rdp-cli/tests/live/live_186_launch_record_gc.rs`:
  `live_186_launch_record_gc_collects_dead_spares_live` and
  `live_186_launch_record_growth_bounded`, both under an isolated `FF_RDP_HOME` so they never touch
  the real `~/.ff-rdp/`.

### Live sweep

Two runs, both `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1`, both with a browser on port 6000
so the `preexisting` tier executed.

```text
run 1  LIVE_SWEEP_SUMMARY executed=286 skipped=0 preexisting=0 vanished=0 launch_timeout=0 total=286
       ff-rdp-cli --test live: 276 passed / 1 failed; all four ff-rdp-core live targets green
run 2  LIVE_SWEEP_SUMMARY executed=286 skipped=0 preexisting=0 vanished=0 launch_timeout=0 total=286
       ff-rdp-cli --test live: 276 passed / 1 failed; all four ff-rdp-core live targets green
```

`live_186_launch_record_gc_collects_dead_spares_live` and `live_186_launch_record_growth_bounded`
both `ok` in **both** runs.

The two runs failed on *different* tests, and neither failure is this iteration's code:

- run 1 — `live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_firefox_running`,
  "precondition violated — 1 ff-rdp-managed profile dir ... still owned by a live process (pid
  95896)". Self-inflicted: the port-6000 browser had been started with `ff-rdp launch`, which
  creates a *managed* profile, and this test requires no managed profile to have a live owner.
  Re-run 2 started port 6000 as a raw `firefox -profile … --start-debugger-server 6000` with
  hand-written devtools prefs; `live_96` then passed.
- run 2 — `live_137_daemon_mode_parity::live_137_consent_accept_via_daemon`, "daemon never reported
  live frame targets" with `target_count: 1, live_target_count: 0`. Passed in run 1 and passes in
  **6.6 s in isolation**; load-sensitive.

Both are filed as watch conditions 8 and 9 on
[[iteration-192-live-sweep-watch-conditions-carried-forward]] — see Carry-over below.

### Out-of-scope check-in

Orphaned Firefox processes between sessions are untouched by this change: the sweep only ever
unlinks `launch-record.*.json`, never signals a process, and `prune_orphan_profiles` reads the
`.ff-rdp-owner-pid` marker inside the profile directory rather than these records. One second-order
effect worth stating rather than leaving implicit: removing a stale record removes what `daemon
stop --port <p>` would have read, but a record whose pid is dead already returned `None` from
`read_in`, so no reachable behaviour changes.

## Carry-over

Every non-green line from both sweeps, plus everything else this iteration produced.

| Row | Source | Disposition |
|---|---|---|
| `live_96_..._prune_removes_all_when_no_firefox_running` FAILED (run 1) | sweep run 1 | **folded** into [[iteration-192-live-sweep-watch-conditions-carried-forward]] as watch condition 9. Diagnosed as my own setup, not a product defect — but "environmental" is a diagnosis, not a disposition, and the collision between the `preexisting` tier's requirement and `live_96`'s precondition is real and will bite the next runner |
| `live_137_..._consent_accept_via_daemon` FAILED (run 2), passed run 1, passes isolated in 6.6 s | sweep run 2 | **folded** into [[iteration-192-live-sweep-watch-conditions-carried-forward]] as watch condition 8, with the captured status envelope. Not forked into its own plan: one load-sensitive observation with no second data point is exactly what 192 exists to hold until it fires again |
| `live_62_page_map_index` passed both runs | run-wide note that it fails under load on purpose until iteration 181 lands | **no plan needed here** — already owned by iteration 181. Recorded because one green run is not evidence a load-sensitive defect is fixed; it must not be read as 181 being unnecessary |
| Two ff-rdp-managed profile dirs left behind by dead owners, seen while checking preconditions | manual inspection during setup | **no plan, stated reason** — `prune_orphan_profiles` already owns this and `live_96` already asserts it; both dirs had dead owners and were reclaimable. If a profile with a *live* owner is ever found orphaned, that needs its own plan |
| iteration 191 (`launch --replace` kills a recycled pid off a stale record) is narrowed but not closed by this PR | this iteration's own analysis | **no plan needed** — 191 already exists and already `depends_on` 186. Recorded so nobody reads "4803 → 13 records" as 191 being fixed: a record is still stale for the whole window between its Firefox dying and the next launch sweeping |
| The plan's `dogfood_path` step 2 predicted "no live pids" among the leaked records; 13 had live pids, mostly recycled | this iteration's measurement | **closed in this PR** — the prediction is corrected in the Outcome section above and the asymmetry is written into `daemon_record.rs`'s GC section comment |
| Unticked acceptance criteria | — | none. All 3 ACs and all 7 task boxes ticked, each against a measured result recorded above |

## Out of scope

- **Orphaned Firefox processes between sessions.** Related symptom, different owner: those are
  browsers, not records, and `prune_orphan_profiles` (`util/profile_dir.rs:571`) already owns
  that area. If this iteration's sweep makes that worse or better, say so and stop there.
- **The `pgrep -f 'ff-rdp-profile'` self-match** in the documented pre-sweep orphan check — that
  pattern matches the checking process itself. It lives in house rules and the `iteration-close`
  skill, not in this repo, and skill edits cannot run through ralph-loop.

## References

- `crates/ff-rdp-cli/src/daemon_record.rs` — writer, `read_in`, `remove_in`
- `crates/ff-rdp-cli/src/daemon/registry.rs:464` — `gc_stale_spawn_locks_in`, the precedent
- `crates/ff-rdp-cli/tests/live/live_142_disk_growth.rs` — 142's throttle-file GC and its reasoning
- `crates/ff-rdp-cli/src/util/profile_dir.rs:571` — `prune_orphan_profiles`
