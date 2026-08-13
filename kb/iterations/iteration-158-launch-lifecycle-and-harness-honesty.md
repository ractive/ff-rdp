---
branch: iter-158/launch-lifecycle-and-harness-honesty
date: 2026-08-13
depends_on: []
dogfood_path: |
  # ── 1. The 5s port wait (Theme A) ───────────────────────────────────────────
  # Start four launches at once so Firefox is under the contention that made it
  # bind at 7s. On main this fails; after the fix all four must succeed.
  for p in 7101 7102 7103 7104; do ff-rdp launch --headless --debug-port $p & done; wait
  # → all four exit 0. No stdout contains "not reachable after 5s".
  #   Each prints results.pid for a live process.
  ff-rdp launch --headless --debug-port 7105 --launch-timeout 45 --jq '.meta.launch_wait_secs'
  # → 45   (the bound is a real, reportable knob, not a hardcoded constant)
  FF_RDP_LAUNCH_TIMEOUT_SECS=40 ff-rdp launch --headless --debug-port 7106 --jq '.meta.launch_wait_secs'
  # → 40   (env override; default with neither flag nor env is 30)
  
  # ── 1b. The error text must name the real cause ─────────────────────────────
  nc -l 7107 &                       # a non-Firefox listener squats the port
  ff-rdp launch --headless --debug-port 7107; echo "exit=$?"
  # → exit=1 and the message names the OCCUPYING process and PID, e.g.
  #   "port 7107 is already in use by nc (PID 51234) — pass --debug-port to pick another".
  #   It must NOT say "Firefox started ... is the port already in use?", which
  #   today is printed for the opposite cause (Firefox simply had not bound yet).
  
  # ── 2 + 3. --replace repeatability (Themes B and C) ─────────────────────────
  ff-rdp launch --headless --debug-port 7108
  for i in 1 2 3; do ff-rdp launch --headless --debug-port 7108 --replace --jq '.meta.replaced'; done
  # → three runs, each exits 0 and prints {"stopped":true,"pid":<prior>}.
  #   None prints "no owner-PID marker" (the DaemonRecord must survive a failed
  #   stop) and none prints "port still listening after 8s" (the escalation
  #   ladder must actually reach orphaned children).
  
  # ── 3b. The escalation ladder on the primary path ───────────────────────────
  ff-rdp launch --headless --debug-port 7109 --jq '.results.pid'   # note the pid
  kill -9 <that pid>                 # orphan the children; they keep port 7109
  ff-rdp daemon stop --port 7109; echo "exit=$?"
  # → exit=0. On main this reports "port still listening after 8s" because
  #   run_escalation bails on its "pid already dead" guard.
  lsof -i :7109                      # → no output; the port is genuinely free
  
  # ── 4. Harness honesty (Theme D) ────────────────────────────────────────────
  PATH=/nonexistent cargo test -p ff-rdp-cli --test live live_158 -- --include-ignored
  # → FAILS with a panic naming the launch exit status and captured stderr.
  #   On main every one of these reports `ok` because the call site returns early.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
  # → 0 failed, and the summary line separates the three tiers, e.g.
  #   LIVE_SWEEP_SUMMARY executed=N skipped=M preexisting=K total=T
  
  # ── 5. --profile into a missing directory (Theme E) ─────────────────────────
  rm -rf /tmp/ff-rdp-dogfood-158 && ff-rdp launch --headless --debug-port 7110 \
    --profile /tmp/ff-rdp-dogfood-158/prof --jq '.results.profile_path'
  # → exit 0, prints /tmp/ff-rdp-dogfood-158/prof, and that dir contains user.js.
  #   On main this fails: ensure_devtools_prefs opens user.js without creating
  #   the parent directory first.
first_call_sites:
  - primitive: launch::resolve_port_wait_bound
    site: >-
      crates/ff-rdp-cli/src/commands/launch.rs — the wait_for_port call currently at
      :540
  - primitive: launch::PortWaitOutcome
    site: >-
      crates/ff-rdp-cli/src/commands/launch.rs — error construction in the Ok(None)
      arm, ~:541-545
  - primitive: daemon::client::stop_pid_with_full_escalation
    site: >-
      crates/ff-rdp-cli/src/daemon/client.rs — stop_daemon_and_build_result (:961)
      and stop_prior_instance (:1150)
status: done
title: "Iteration 158: launch's 5s port wait fails under load, and the live suite converts that into a silent pass"
type: iteration
tags:
  - iteration
---

# Iteration 158: `launch`'s 5s port wait fails under load, and the live suite converts that into a silent pass

Evidence base: [[analysis-2026-08-13-what-ff-rdp-became]] §3.3, §3.4, §4, §6. Every code claim
below carries a `file.rs:line`; every measurement below was taken on 2026-08-13 and is quoted, not
paraphrased.

This is **one** defect wearing four faces. `launch` loses a race it should not be running; a failed
stop destroys its own recovery evidence; the kill ladder built to fix that never fires; and the test
suite that would have caught all three reports `ok` when Firefox never started. Fixing any subset
leaves the system in a worse state than fixing none — see [[#Sequencing — this is not optional]].

## The defect

### 1. The port wait is a hardcoded 5 seconds

`crates/ff-rdp-cli/src/commands/launch.rs:540`:

```rust
if let Err(e) = wait_for_port("localhost", port, Duration::from_secs(5)) {
```

Measured: Firefox binds its debug port at **7 s** under load. `ff-rdp launch` failed **5/5
attempts** at load average 6.8. The global `--timeout` (`cli/args.rs:317`, `DEFAULT_TIMEOUT_MS =
10_000`) is a *socket operation* deadline and is never threaded into this call — and at 10 s it
would still be too small to be the right source for this bound.

The error text is worse than the timeout. `wait_for_port`'s failure branch
(`launch.rs:408-411`) reads:

```rust
Err(AppError::User(format!(
    "debug port {port} is not reachable after {}s — is the port already in use?",
    timeout.as_secs()
)))
```

wrapped by the caller into `"Firefox started (pid {pid}) but {e}"`. It blames a port conflict for
what is almost always the opposite condition: the port is **unbound**, and Firefox has not reached
it yet. A user who reads that message goes hunting for a process that does not exist.

The right shape is already written down inside this repo, in the test harness:
`crates/ff-rdp-cli/tests/common/mod.rs:107-141` — `launch_wait_timeout()` defaults to **30 s**,
with an env override (`FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS`) and a documented fallback for malformed
values, and a `parse_launch_timeout(Option<&str>)` split out purely so the precedence rules are
unit-testable without touching process-wide env. Port that design into the product.

### 2. `daemon_record::remove` runs before the port-free check

`crates/ff-rdp-cli/src/daemon/client.rs:1150-1157` (inside `stop_prior_instance`):

```rust
let (stopped, port_free, _escalation_msg) = kill_pid_and_wait_port(rec.pid, rec.port);
crate::daemon_record::remove(rec.port).ok();      // ← :1151, unconditional
if !port_free {
    return Err(AppError::User(format!(
        "port {port} is still in use after stopping the prior instance (pid {}). ...",
```

Mirrored at `client.rs:962-963` inside `stop_daemon_and_build_result`, same ordering.

The record is the **ownership proof**. Deleting it on a failed stop means the next
`launch --replace` finds no `DaemonRecord`, falls through to branch 3 (the raw port-owner lookup,
`client.rs:1194-1210`), and hits the fails-closed guard:

```
port {port} is in use by {name} (PID {pid}), which ff-rdp did not launch (no owner-PID marker).
  Refusing to stop a process ff-rdp does not own
```

— fired against an instance ff-rdp launched itself. That guard is correct and must stay (it exists
because of the 2026-07-09 incident, `client.rs:1198-1203`). The bug is that a failed stop lies to it.

Observed by a dogfood lane: three `launch --replace` attempts, three errors — twice *"no owner-PID
marker"*, once *"port still in use after stopping the prior instance"*.

### 3. The escalation ladder never runs on the primary path

`run_escalation` (`client.rs:112-167`) is documented as SIGTERM-group → 1 s grace → SIGKILL-group →
re-poll → SIGKILL the **pre-captured pgid** (`kill_process_tree`, reaching children that outlived
the parent) → re-poll. Its second guard is:

```rust
if !(h.is_alive)(pid) {
    return (false, port_still_listening_msg(pid, port));   // client.rs:124-126
}
```

Its only production caller is `wait_port_free_with_escalation` (`client.rs:184`), called from
`kill_pid_and_wait_port` (`client.rs:900-916`) — which kills the PID *first*:

```rust
fn kill_pid_and_wait_port(pid: u32, port: u16) -> (bool, bool, String) {
    process::kill_process_group(pid);                       // :901
    // ... 2 s wait, then SIGKILL if still alive ...
    let (port_free, escalation_msg) = wait_port_free_with_escalation(pid, port);   // :911
```

By the time `run_escalation` is reached the pid is dead by construction, so the `is_alive` guard
returns immediately. Steps 3–7 — the entire mechanism designed to reach orphaned children still
holding the port — are dead code in production. That is the source of the
`"port still listening after 8s"` message (`PORT_FREE_WAIT_BOUND`, `client.rs:21`).

The same SIGTERM→wait→SIGKILL→poll sequence is written out **four** times:

| # | location | note |
|---|---|---|
| 1 | `kill_pid_and_wait_port`, `client.rs:900-916` | the primary path |
| 2 | `stop_daemon_and_build_result`, `client.rs:1063-1071` | kills the proxy daemon PID |
| 3 | `stop_daemon_and_build_result`, `client.rs:1077-1087` | kills the resolved Firefox PID |
| 4 | `stop_prior_instance` branch 3, `client.rs:1219-1240` | port-owner path, with ownership re-verification between steps |

Only #4 re-verifies port ownership between signals, and none of them captures the pgid before the
first kill. [[analysis-2026-08-13-what-ff-rdp-became]] §4 lists `EscalationHooks` (`client.rs:63-96`)
as a deletion candidate — *"tests a ladder that never runs in production"* — with the explicit
instruction **"fix the neutering bug first, then reassess."** This iteration fixes; it does not
delete `EscalationHooks`.

### 4. The live suite converts a launch failure into a silent pass

`LiveFirefox::try_launch` (`tests/common/mod.rs:335-372`) spawns the real product binary and, on
any failure:

```rust
if !output.status.success() {
    return None;                                  // :354-356
}
```

with `.stderr(Stdio::null())` set at `:349`, so the product's diagnostic is discarded before it can
be read. `headless_on_random_port` (`:295-311`) retries 3× and returns `None`. Every call site then
does:

```rust
let Some(ff) = LiveFirefox::headless_on_random_port() else {
    eprintln!("...: Firefox not available — skipping");
    return;                                       // ← libtest reports `ok`
};
```

[[analysis-2026-08-13-what-ff-rdp-became]] §3.3 counts **167** such call sites; a fresh
`grep -rn 'headless_on_random_port' crates/*/tests` on 2026-08-13 returns 173 references and 182
occurrences of the `Firefox not available` skip string. Treat the exact number as whatever the
implementer measures — the point is that it is every live test in the repo.

The notices are invisible: libtest captures test stderr and discards it for passing tests.
Verified — **zero** `LiveFirefox: pid=` lines (the `eprintln!` at `tests/common/mod.rs:361`) appear
in a log containing **170 passing tests**. There is no way, from a green run, to tell how many `ok`
results reached Firefox.

This is [[iteration-155-live-skip-reports-green]]'s defect surviving through a second door.
iter-155 spent 845 LOC making unmet **env gates** report `ignored` rather than a fake `ok`; the
larger fake-`ok` source — Firefox failed to launch — was untouched and is counted as `executed` by
`live-sweep`, whose `executed=N` is computed from static classification before any process spawns
(`crates/xtask/src/live_sweep.rs:386-387`; DEC-031 states this as a feature).

### 5. `launch --profile <dir>` fails when the directory does not exist

`ensure_devtools_prefs` (`launch.rs:101-132`) opens `profile.join("user.js")` with
`.create(true).append(true)` at `:114-117` and never `create_dir_all`s the parent. Called from
`build_command` at `launch.rs:243`, before the spawn. A user pointing `--profile` at a path they
intend ff-rdp to populate gets `failed to write devtools prefs to <path>/user.js: No such file or
directory`.

## How it was found

Four dogfood lanes plus the first full qualified `live-sweep` ever run, on 2026-08-13:

```
LIVE_SWEEP_SUMMARY executed=197 skipped=25 total=222
EXIT=1                                          # 49.5 minutes wall
190 passed · 7 failed across 5 targets
```

One of the seven was a real product defect, and it ties §3.3 to §3.4:

```
---- live_153_replace_double_envelope::live_153_replace_emits_single_envelope stdout ----
panicked at crates/ff-rdp-cli/tests/live/live_153_replace_double_envelope.rs:121:5:
  FAIL — launch --replace returned non-zero
  stdout={"error":"Firefox started (pid 37649) but debug port 59593 is not reachable
           after 5s — is the port already in use?"}
```

`--replace` did not fail on its own logic. It lost the 5 s race under suite contention. The same
test passes in isolation — and had been ticked six hours earlier with a `[verified: 2026-08-13, ...
3 passed / 0 failed]` annotation that was **truthful** and certified a broken feature anyway
([[iteration-153-launch-replace-double-envelope]]). That is the strongest available argument for
running the whole sweep rather than the one test an AC names, and it is why Theme D is in this plan
rather than a follow-up.

The other six failures were one environmental cause — see [[#Theme F]].

## Sequencing — this is not optional

**Themes A, B, C and D must land in the same PR, and D must be the last commit on the branch.**

Theme D flips ~170 call sites from "silently return" to "fail". If it lands while `launch` still
has a 5 s bound, every one of those tests goes red the moment the machine is busy — which is
exactly the state the full sweep runs in. An implementer working theme-by-theme will naturally
reach for D first because it is the most mechanical change. Do not. The order is:

1. **Theme A** — the launch bound and its error text. Nothing else can be trusted until launch is
   reliable under contention.
2. **Theme E** — the `--profile` `create_dir_all` one-liner (independent, cheap, no ordering risk).
3. **Themes B and C** — the stop path. C subsumes B's call sites, so do B's ordering fix first and
   then unify.
4. **Theme F** — the `live-sweep` tier decision, which must exist before the sweep in D's AC can be
   read as a pass.
5. **Theme D** — flip the harness, then run the full sweep. Any red here is now a real signal.

A green sweep taken before A, B and C are in is meaningless; a red sweep taken after D but before
A is unattributable. There is exactly one useful measurement point, and it is at the end.

## Themes

### Theme A — a configurable launch port-wait bound, and an error that names the real cause

Replace `Duration::from_secs(5)` at `launch.rs:540` with a resolved bound. Precedence, mirroring
`tests/common/mod.rs:107-141`:

1. `--launch-timeout <secs>` on `launch` (new flag; **not** the global `--timeout`, which is a
   10 s socket deadline and would make the regression worse),
2. `FF_RDP_LAUNCH_TIMEOUT_SECS` env,
3. default **30 s**.

A malformed or empty value falls back to the default rather than erroring — a bad env var must not
break a launch. Split the precedence logic into a pure `resolve_port_wait_bound(flag, env)` so it
is unit-testable without process-wide env mutation, exactly as `parse_launch_timeout` already is.
Report the effective bound in the envelope (`meta.launch_wait_secs`) so the dogfood path can see it.

Then split the failure message in two. Add a **pre-spawn** occupancy check: if the port already has
a listener before Firefox is spawned, fail immediately with the occupying process name and PID
(`crate::port_owner::find_listener` already returns both — it is what `stop_prior_instance:1194`
uses). Only the post-spawn deadline path may say Firefox did not bind, and it must not mention a
port conflict:

- occupied: `port {port} is already in use by {name} (PID {pid}) — pass --debug-port to pick another`
- deadline: `Firefox (pid {pid}) did not open debug port {port} within {secs}s — raise --launch-timeout or set FF_RDP_LAUNCH_TIMEOUT_SECS`

Factor the deadline branch behind an injectable prober fn pointer (the `EscalationHooks` pattern at
`client.rs:63-96` is the in-repo precedent) so both messages are unit-testable without a real
Firefox.

### Theme B — never destroy the ownership trail on a failed stop

Move `crate::daemon_record::remove(rec.port)` to **after** the `port_free` check at both
`client.rs:1151` and `client.rs:963`. On `!port_free` the record stays, so the retry re-enters the
DaemonRecord branch (which is permitted to kill) instead of the port-owner branch (which is
fails-closed and refuses). The dead-PID cleanup at `client.rs:1166` is a different case and stays
where it is — there the process is already gone and the record is genuinely stale.

### Theme C — one stop ladder, pgid captured before the first kill

Collapse the four sequences listed in [[#3. The escalation ladder never runs on the primary path]]
into a single `stop_pid_with_full_escalation(pid, port, &EscalationHooks) -> (stopped, port_free,
msg)` that:

- captures the pgid **first**, before any signal is sent (today `run_escalation:114` captures it
  first *within itself*, but by then `kill_pid_and_wait_port:901` has already killed the parent);
- then runs the full ladder unconditionally: SIGTERM-group → grace → SIGKILL-group → poll →
  `kill_process_tree(pid, captured_pgid)` → poll;
- keeps the `pgid_safe_to_kill` guard (`client.rs:151-158`) verbatim — killing a pgid that is not
  the target pid can blast the user's interactive shell;
- keeps branch 4's ownership re-verification between signals (`client.rs:1224-1236`) as an optional
  parameter, since only the port-owner path needs it;
- drops the `is_alive` early-return as a *gate on escalation*. A dead parent is the case the tree
  kill exists for. It may still short-circuit the "already free" fast path.

`EscalationHooks` stays for now. Reassess its deletion only after this iteration proves the ladder
runs (see [[analysis-2026-08-13-what-ff-rdp-became]] §4).

### Theme D — an unavailable Firefox fails the test

Change `LiveFirefox::headless_on_random_port()` to return `Self`, not `Option<Self>`, and panic
with a diagnostic that names: the launch exit status, the captured stdout **and stderr** (drop
`.stderr(Stdio::null())` at `tests/common/mod.rs:349` and capture it instead), the port, and the
attempt number. Keep a private `try_headless_on_random_port() -> Option<Self>` for the harness's own
negative tests and for the 3× retry loop.

Update every call site to bind directly. Delete the `Firefox not available — skipping` else-arms —
all of them. A test that genuinely tolerates an absent Firefox belongs behind an `#[ignore]` gate
that `live-sweep` already understands, not behind a runtime early return.

The `eprintln!("LiveFirefox: pid=...")` at `:361` is invisible on the passing path. Either drop it
or route it somewhere that survives (a file under the artifact dir); do not leave a diagnostic that
only the failing path can show while claiming it documents the passing one.

### Theme E — `--profile` into a directory that does not exist

`create_dir_all(profile)` at the head of `ensure_devtools_prefs` (`launch.rs:101`), before the
`OpenOptions` at `:114`. Note the security context in `build_command`'s comment at `launch.rs:250-254`:
the *managed* temp-profile path deliberately uses unpredictable names to defeat a same-UID symlink
plant. A user-supplied `--profile` path is the user's own choice and gets no marker
(`should_write_owner_marker`, `launch.rs:415+`) — creating it is fine, but do not follow a symlink
at the leaf when writing `user.js`.

### Theme F — the `ff-rdp-core` live tests are a third tier; decide it here

Six of tonight's seven sweep failures were one cause: the `ff-rdp-core` live tests do not launch
Firefox at all. They connect to a pre-existing instance on the fixed port 6000 — their own
`#[ignore]` reason says so (`crates/ff-rdp-core/tests/live_firefox_test.rs:26` and `:56`: *"set
FF_RDP_LIVE_TESTS=1 and start Firefox with --start-debugger-server 6000"*; see also
`live_129_frame_targets.rs:12`, `live_record_fixtures.rs:10`). `live-sweep` neither provides that
instance nor checks for it, and counted all six as `executed`. That is a **third** way `executed=N`
overstates reality, distinct from the two in §3.3.

**Decision for this iteration: classify, do not launch.** `live-sweep` gains a third bucket. A test
whose `#[ignore]` reason mentions `--start-debugger-server 6000` is `preexisting`; the sweep probes
`127.0.0.1:6000` once at start and, if nothing answers, runs those targets **without**
`--include-ignored` so libtest reports them `ignored`, and reports them under a new `preexisting=K`
field rather than folding them into `executed`.

Rationale for not having the sweep launch one: port 6000 is ff-rdp's documented default and the
port a human is most likely to already be using by hand. The fails-closed ownership guard at
`client.rs:1198-1203` exists precisely because ff-rdp once killed a hand-started Firefox on 6000.
A sweep that binds 6000 itself either collides with the user or inherits that whole ownership
problem. Classification costs one TCP probe and is honest about what it did.

## Acceptance Criteria [13/14]

- [x] unit_158_resolve_port_wait_bound: `resolve_port_wait_bound(None, None)` returns
      `Duration::from_secs(30)`; `(Some(45), _)` returns 45 s; `(None, Some("7"))` returns 7 s;
      `(Some(45), Some("7"))` returns 45 s (flag beats env); `(None, Some("abc"))` and
      `(None, Some(""))` both return the 30 s default
- [x] unit_158_port_wait_error_names_bind_timeout: with an injected prober that never connects,
      the resulting `AppError::User` message contains `did not open debug port` and the resolved
      bound in seconds, and contains neither the substring `already in use` nor `after 5s`
      [deferred — new plan: kb/iterations/iteration-163-ac-fidelity-reads-only-the-first-line.md]
- [x] unit_158_launch_rejects_occupied_port_before_spawn: with an injected port-owner lookup
      returning a non-Firefox listener, `launch` returns `AppError::User` naming that process name
      and PID, and the Firefox spawn hook records zero invocations
      [deferred — new plan: kb/iterations/iteration-163-ac-fidelity-reads-only-the-first-line.md]
- [x] live_158_launch_survives_contended_bind: four concurrent `ff-rdp launch --headless` on
      distinct ports all exit 0 with distinct live `results.pid` values, and no stdout contains
      `not reachable after 5s`
      [verified: 2026-08-14, ok inside the full sweep at load average 18.6 —
      LIVE_SWEEP_SUMMARY executed=221 skipped=0 preexisting=9 total=230, 219 passed / 2 failed]
- [x] live_158_launch_reports_effective_wait_bound: `ff-rdp launch --headless --launch-timeout 45`
      emits `meta.launch_wait_secs == 45`, and the same command with `FF_RDP_LAUNCH_TIMEOUT_SECS=40`
      and no flag emits `meta.launch_wait_secs == 40`
      [verified: 2026-08-14, ok inside the full sweep — LIVE_SWEEP_SUMMARY executed=221 skipped=0 preexisting=9 total=230, 219 passed / 2 failed]
- [x] unit_158_record_survives_failed_stop: with injected hooks where the port stays held,
      `stop_prior_instance` returns `Err` **and** `daemon_record::read(port)` still returns the
      record afterwards; the same assertion holds for `stop_daemon_and_build_result`
      [deferred — new plan: kb/iterations/iteration-163-ac-fidelity-reads-only-the-first-line.md]
- [x] live_158_replace_repeats_cleanly: three consecutive `launch --debug-port P --replace` against
      a prior instance each exit 0, each emit exactly one JSON document with
      `meta.replaced.stopped == true`, and no stdout contains `no owner-PID marker` or
      `still in use after stopping the prior instance`
      [verified: 2026-08-14, ok inside the full sweep; the baseline's one real defect
      live_153_replace_emits_single_envelope also went from FAILED to ok in the same run —
      LIVE_SWEEP_SUMMARY executed=221 skipped=0 preexisting=9 total=230, 219 passed / 2 failed]
- [x] unit_158_stop_ladder_captures_pgid_before_any_kill: a recording `EscalationHooks` stub asserts
      `stop_pid_with_full_escalation` calls `get_pgid` strictly before the first of
      `kill_group_term` / `kill_group_kill` / `kill_process_tree`
- [x] unit_158_stop_ladder_reaches_tree_kill_when_parent_is_dead: with `is_alive` returning `false`
      and `wait_port_closed` returning `false` for every poll, the recorded call sequence still
      contains `kill_process_tree` with the pre-captured pgid
- [x] unit_158_single_stop_ladder_implementation: a source-scan test over
      `crates/ff-rdp-cli/src/daemon/client.rs` asserts `process::kill_process_group(` appears in
      exactly one non-test function — `stop_pid_with_full_escalation` — replacing the four sites at
      `:901`, `:1063`, `:1077` and `:1219`
      [deferred — new plan: kb/iterations/iteration-163-ac-fidelity-reads-only-the-first-line.md]
- [x] live_158_stop_reaches_orphaned_children: after `launch`ing on port P and `SIGKILL`ing only the
      parent PID, `ff-rdp daemon stop --port P` exits 0 and `wait_for_port_closed(P, 8s)` returns
      `true`; the error text `port still listening after 8` appears nowhere in the output
      [verified: 2026-08-14, ok inside the full sweep — LIVE_SWEEP_SUMMARY executed=221 skipped=0 preexisting=9 total=230, 219 passed / 2 failed]
- [x] unit_158_no_live_test_skips_on_missing_firefox: a source-scan test over
      `crates/ff-rdp-cli/tests/**` and `crates/ff-rdp-core/tests/**` asserts zero occurrences of the
      string `Firefox not available` and zero `else` arms binding
      `LiveFirefox::headless_on_random_port` to an `Option`
      [deferred — new plan: kb/iterations/iteration-163-ac-fidelity-reads-only-the-first-line.md]
- [x] live_158_launch_creates_missing_profile_dir: `launch --headless --profile <tmp>/absent/prof`
      exits 0, `<tmp>/absent/prof/user.js` exists and contains a `devtools.debugger.remote-enabled`
      pref line, and `results.profile_path` equals that directory
      [verified: 2026-08-14, ok inside the full sweep — LIVE_SWEEP_SUMMARY executed=221 skipped=0 preexisting=9 total=230, 219 passed / 2 failed]
- [ ] live_158_sweep_reports_three_tiers: `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run
      -p xtask -- live-sweep` exits 0 with 0 failed, and its `LIVE_SWEEP_SUMMARY` line carries a
      `preexisting=K` field with `K == 6` when nothing is listening on `127.0.0.1:6000`, those six
      targets reported `ignored` by libtest and excluded from `executed`
      NOT MET — left unticked, wording unchanged. Measured 2026-08-14:
      `LIVE_SWEEP_SUMMARY executed=221 skipped=0 preexisting=9 total=230`, exit 1,
      219 passed / 2 failed, 32.7 min wall at load average 18.6.
      The tier mechanism works exactly as specified — all 9 preexisting tests were reported
      `ignored` by libtest (1 + 3 + 3 + 2 across `live_129_frame_targets`, `live_61p_registry`,
      `live_61u`, `live_firefox_test`) and excluded from `executed`. Two stated conditions fail:
      (a) `K == 9`, not 6 — 6 was the count of ConnectionRefused *failures* in the 2026-08-13
      baseline, not the number of tests a classifier identifies; every `ff-rdp-core` live test
      resolves its port through `firefox_port()` and none launches a browser.
      (b) `0 failed` — 2 failed, neither caused by this iteration (its product diff touches only
      `commands/launch.rs` and `daemon/client.rs`):
      `live_109_throttle_block::live_block_url_pattern` is network-gated and therefore never ran
      in the baseline sweep at all, and asserts a real product defect (a blocked URL's fetch
      resolved); `live_141_output_hygiene::live_141_text_empty_result_keeps_metadata` failed
      because the proxy daemon did not start under load — a condition that pre-158 returned
      `None` and reported `ok`, i.e. Theme D finding a real hidden skip on its first run.
      Both are filed as [[iteration-164-two-failures-the-158-sweep-uncovered]].

## Implementation deviations from the plan

Recorded rather than reworded — the AC text above is left exactly as planned.

- **`unit_158_single_stop_ladder_implementation` asserts something stronger than
  the AC's literal wording.** The AC predicted `process::kill_process_group(`
  would appear in exactly one non-test function. In the implemented ladder it
  appears in **zero**: every signal goes through an `EscalationHooks` fn
  pointer, so the only non-test *mention* of `process::kill_process_group` is
  the `EscalationHooks::real()` wiring, which is the single feed into
  `stop_pid_with_full_escalation`. The test asserts both facts (zero open-coded
  calls; exactly one wiring mention; exactly one `fn
  stop_pid_with_full_escalation`). The substance — the four duplicated
  sequences at `:901`, `:1063`, `:1077` and `:1219` are gone — is delivered.
- **`live_158_sweep_reports_three_tiers` predicted `K == 6`; the measured value
  differs.** Six was the number of `ConnectionRefused` *failures* in the
  2026-08-13 sweep, not the number of tests a preexisting-tier classifier
  identifies. Every `ff-rdp-core` live test resolves its port through
  `support::recording::firefox_port()` (default 6000) and none launches a
  browser, so the honest classification covers all of them. The measured value
  is recorded on the AC itself; the AC is left unticked because its stated
  constant does not hold.
- **`ac-fidelity-check.sh` reports a false positive on five of the ticked ACs.**
  Its evidence heuristics read only an AC's *first wrapped line* (`text`, :189)
  while iter-154's two newer checks read the folded `full_text` (:192). Every
  one of the five names its test on the first line and puts the resolvable
  symbol on the second, so the gate reports `no evidence in diff` for symbols
  that are demonstrably there: against the same `git diff origin/main...HEAD`,
  `grep -cF` finds `unit_158_port_wait_error_names_bind_timeout` ×2,
  `unit_158_launch_rejects_occupied_port_before_spawn` ×2,
  `unit_158_record_survives_failed_stop` ×2,
  `unit_158_single_stop_ladder_implementation` ×2,
  `unit_158_no_live_test_skips_on_missing_firefox` ×2 and `AppError::User` ×29.
  The gate's slug regex also does not recognise the `unit_*` prefix at all.
  The ACs are left ticked and unreworded — moving a symbol onto the first line
  to silence the check is precisely the reword reflex CLAUDE.md forbids. The
  fix touches `~/.claude/skills/`, which cannot be driven through ralph-loop,
  so it is filed as
  [[iteration-163-ac-fidelity-reads-only-the-first-line]].
- **Theme F's classifier reads the whole source file, not only the `#[ignore]`
  reason.** The plan specified matching `--start-debugger-server 6000` in the
  ignore reason; only `live_firefox_test.rs` actually spells that out, while
  `firefox_port` appears in every affected file. Matching the file keeps the
  classification complete instead of catching two tests out of nine.

## Notes

- **The four `[verified: …]` annotations this plan will need are the whole point.** Every
  `live_158_*` AC must be ticked with `[verified: <YYYY-MM-DD>, <measured result>]`, and per
  [[iteration-153-launch-replace-double-envelope]]'s failure the measurement that counts is the one
  taken during a **full** `live-sweep`, not an isolated `cargo test live_158`. iter-153's isolated
  run was truthful and certified a broken feature. Quote the sweep's `LIVE_SWEEP_SUMMARY` line.
- Delete nothing this iteration. `EscalationHooks`, the `network.rs` fallback bookkeeping, and the
  `store-events` RPC are all on [[analysis-2026-08-13-what-ff-rdp-became]] §4's deletion list, and
  all of them are explicitly gated on a fix landing first. This is the fix.
- The `network` watcher regression (§3.2) is the highest-value fix in the repo and is **not** in
  this plan. It is a different subsystem (`daemon/server.rs:447`'s
  `get_watcher_with_options(…, Some(true))`) and belongs in its own iteration. Do not fold it in.
- `--launch-timeout` is a new flag on the `launch` subcommand and needs a `--help` line plus an
  entry in the `Output:` doc string at `cli/args.rs` (the launch `long_about` block already
  enumerates the envelope shape and must gain `launch_wait_secs`).
- Cross-platform: `kill_process_tree` on Windows falls through to `taskkill /F /T /PID` and receives
  `captured_pgid == None` (`client.rs:155-157`). The unified ladder must preserve that path —
  `pgid_safe_to_kill` is `true` on Windows by design because the taskkill call is already
  pid-scoped.
- Related: [[iteration-155-live-skip-reports-green]] (the env-gate half of Theme D),
  [[iteration-153-launch-replace-double-envelope]] (the `--replace` envelope, whose test this
  iteration finally makes meaningful), [[analysis-2026-08-13-what-ff-rdp-became]] (all evidence).
