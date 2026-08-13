---
branch: iter-159/daemon-watcher-regression
date: 2026-08-13
depends_on:
  - kb/iterations/iteration-158-launch-lifecycle-and-harness-honesty.md
dogfood_path: |
  # CLEAN ROOM. Nothing in this sequence touches the direct (--no-daemon) path,
  # so nothing can feed the daemon buffer via `store-events`. That matters —
  # see "Why it went unnoticed".
  ff-rdp daemon stop --port 6100 || true
  ff-rdp launch --headless --debug-port 6100 --replace
  ff-rdp daemon status --port 6100 --jq '.results.buffer_sizes'
  # → every buffer is 0 here. Nothing has fed it yet.
  ff-rdp navigate https://en.wikipedia.org/wiki/Firefox --port 6100
  # → a PLAIN navigate, deliberately without --with-network: the daemon's own
  #   resource watcher is the only thing that can capture this page's requests.
  ff-rdp daemon status --port 6100 --jq '.results.buffer_sizes'
  # → the network-event buffer must now be > 0. Today it is still 0.
  ff-rdp network --port 6100 --source watcher --detail --jq '.results | length'
  # → must be >= 1. Today: 0.
  ff-rdp network --port 6100 --source watcher --detail \
    --jq '[.results[] | select(.method != null and .status != null)] | length'
  # → must be >= 1, with method "GET" and status 200 on the document request.
  #   Today: 0, because the array above is empty.
  ff-rdp network --port 6100 --jq '.meta.source'
  # → must be "watcher". Today: "performance-api" — the auto fallback fires and
  #   every row comes back with method/status/content_type/transfer_size null.
  ff-rdp navigate https://www.theguardian.com --port 6100 --with-network --auto-consent
  # → must be accepted (today clap exits 2: "cannot be used with"), must return
  #   results.consent.cmp = "sourcepoint" AND >= 1 network entry from ONE call,
  #   and must return as soon as the network stream goes idle rather than
  #   burning the whole --timeout.
  ff-rdp daemon stop --port 6100
first_call_sites: []
firefox_refs:
  - path: devtools/shared/specs/descriptors/tab.js
    lines: "24-35"
    why: >-
      Declares getWatcher's isServerTargetSwitchingEnabled as Option(0, "boolean").
      This is the flag establish_watcher passes as Some(true).
  - path: devtools/server/actors/watcher/session-context.js
    lines: "60-125"
    why: >-
      Where the flag lands in the session context (default false at :70, threaded
      from the descriptor config at :94 and :119). Establishes what the server
      actually branches on.
  - path: devtools/server/actors/watcher.js
    lines: "500-560"
    why: >-
      notifyResources + emitResources — the throttled queue that produces
      resources-available-array. Shows which actor `this.emit` fires from.
  - path: devtools/server/actors/watcher.js
    lines: "610-660"
    why: >-
      watchResources. Its own doc comment says existing resources are notified
      "via resources-available-array event on related target actors", not on the
      watcher — the sentence that makes the `from`-routing hypothesis plausible.
  - path: devtools/server/actors/resources/index.js
    lines: "330-395"
    why: >-
      The onAvailable/onUpdated/onDestroyed callbacks bind
      `rootOrWatcherOrTargetActor.notifyResources` — a three-way emitter choice.
      Which of the three is selected is the crux of Theme A.
  - path: devtools/shared/specs/targets/window-global.js
    lines: "140-155"
    why: >-
      The window-global TARGET actor also declares resources-available-array /
      resources-destroyed-array. A resource event can therefore legitimately
      carry `from: <target actor>`, which is_watcher_event rejects.
  - path: devtools/shared/specs/watcher.js
    lines: "100-123"
    why: >-
      The watcher's own declaration of the same three events, for the side-by-side
      comparison with the target-actor declaration above.
status: planned
title: "Iteration 159: the daemon's network watcher has delivered nothing since iter-137, and a workaround masks it"
type: iteration
tags:
  - iteration
---

# Iteration 159: the daemon's network watcher has delivered nothing since iter-137, and a workaround masks it

From [[analysis-2026-08-13-what-ff-rdp-became]] §3.2, which names this **the single
highest-value fix in the repo**. Measured on the wire, not inferred.

Not yet released: the last tag is `v0.3.0` (2026-07-11), cut before iter-137. This must land
before the next release.

## The defect

Clean-room A/B on **one** Firefox instance, same page, same instant:

| path | result |
|---|---|
| daemon, plain `navigate` | daemon buffer empty; `network --source watcher` → **0 entries** |
| `--no-daemon`, same page | **10 entries**, `method: GET`, `status: 200`, `content_type` set, `source: "watcher"` |

Firefox 153 supplies full HTTP metadata over RDP. The daemon never receives it.

The consequence for every daemon-mode user — which is every invocation that does not pass
`--no-daemon` — is that `method`, `status`, `content_type` and `transfer_size` are **null on
every request, always**, because `network` silently falls back to the Performance API
(`network.rs:236`, the `NetworkSource::Auto if watcher_was_empty` arm).

**Root cause.** `establish_watcher` at `crates/ff-rdp-cli/src/daemon/server.rs:447` calls:

```rust
let watcher_actor = TabActor::get_watcher_with_options(transport, &tab_actor, Some(true))
```

`Some(true)` is `isServerTargetSwitchingEnabled: true`. The working direct path calls plain
`TabActor::get_watcher()`. The flag's own doc comment, at
`crates/ff-rdp-core/src/actors/tab.rs:164-167`, reads:

> CAUTION: enabling this flag also changes *where* the top-level target is delivered (via the
> watcher, not the descriptor's `getTarget`) — do not flip this on the default
> target-acquisition path; use it only for frame-aware callers (`enumerate_frame_targets`).

The daemon's core resource watcher **is** the default target-acquisition path. Firefox caches
one `WatcherActor` per tab descriptor per RDP connection and honours these options only at
creation time, so the daemon's startup call fixes the session context for every command that
is later proxied through it.

Git-blamed to commit `1612e509`, **iter-137**, 2026-08-10
([[iteration-137-daemon-mode-parity]]), whose own plan records the pre-change state as
*"daemon → 77 rows, source: watcher."* It worked. iter-137 broke it while legitimately fixing
frame-target enumeration.

**Target events still flow.** `daemon status`'s `target_count` climbs normally, so the
connection is healthy and `watchTargets("frame")` is delivering. What never arrives is
`resources-available-array` for `network-event` / `console-message` / `error-message` reaching
the buffering branch in `dispatch_firefox_message`
(`daemon/server.rs:1233-1267`) behind `is_watcher_event` (`daemon/server.rs:1347-1357`), whose
whole test is:

```rust
msg.get("from").and_then(Value::as_str) == Some(daemon_watcher_actor)
```

## How it was found

Not by a test. By a dogfooding lane running the two connection modes back to back against one
browser and noticing that the numbers could not both be right. The A/B above is the entire
evidence; reproduce it before changing a line.

## Why it went unnoticed

Two independent mechanisms, and both must be dismantled by this iteration.

**1. A workaround feeds the broken path from the working one.** `navigate --with-network`
runs its own direct capture and pushes the result back into the daemon's buffer via the
`store-events` RPC (`daemon/server.rs:2724-2761`), serialised by
`serialize_network_resources_for_buffer` (`commands/network_events.rs:220-286`).
Demonstrated:

```
1. fresh daemon buffer:       (empty)
2. DIRECT --with-network:     5 requests captured
3. daemon network --watcher:  7 entries      ← the daemon never captured these
```

Any workflow that touches the direct path even once leaves data behind that makes the broken
daemon path look healthy. A reviewer mid-session hit exactly this and briefly concluded the
finding was wrong. **A workaround built to paper over unreliable buffering now masks the
outage it was built for** — it is actively harmful to diagnosis, and it is why every
verification in this plan must start from a buffer proven empty.

**2. The commit that broke it made its own gate green.** iter-137's daemon-parity AC reads:

> `live_137_network_source_parity`: PASSED with `--source performance-api`, 3 rows in both
> modes.

`--source` was introduced by that same commit. Pinning the parity test to
`performance-api` let it pass without ever consulting the watcher source. The test that would
have caught this — `live_128_network_detail_uses_watcher`
(`crates/ff-rdp-cli/tests/live/live_128_network_output_fidelity.rs`), which asserts
`source: "watcher"` on every entry plus a non-null `method` and `content_type` — is
`#[ignore]`-gated behind `FF_RDP_LIVE_NETWORK_TESTS` and last executed on 2026-08-10.

## Themes

### Theme A — spec research first. Do not touch the flag yet.

**This is not a one-liner, and reverting `Some(true)` is not the fix.** iter-137 set it to
repair frame-target enumeration through the daemon proxy — a real bug with its own passing
live tests (`live_137_frame_targets_via_daemon`, `live_137_consent_accept_via_daemon`,
`live_137_click_cross_origin_via_daemon`). A bare revert trades one regression for another.

Read the server before writing code. In the local Firefox checkout
(`FF_RDP_FIREFOX_PATH`, defaulting to `~/devel/firefox`), establish **how resource delivery is
scoped and addressed when `isServerTargetSwitchingEnabled` is true**. The `firefox_refs:`
entries in this plan's frontmatter are the starting set; extend them with whatever you
actually consult and keep the ranges valid (`cargo run -p xtask -- check-firefox-refs`).

**Version skew — check this first, it can invalidate the whole read.** Verified 2026-08-13:
`FF_RDP_FIREFOX_PATH` is **unset**, so the `~/devel/firefox` fallback applies. That checkout is
**Firefox 154.0** (`config/milestone.txt`, last commit `0088392ab4cc`, 2026-07-08), while the
regression was measured against the **installed 153.0.4**. `devtools/server/actors/watcher.js`
is present and readable. One version of skew is usually harmless, but resource-delivery scoping
under target switching is precisely the area in question, so before trusting any line you cite:
diff the relevant `watcher.js` / `resources/index.js` regions between the 153 release tag and
the checked-out revision, and record in Theme A's write-up whether they differ. If they do,
check out `FIREFOX_153_0_4_RELEASE` (or set `FF_RDP_FIREFOX_PATH` at a 153 worktree) and cite
that instead — a `firefox_refs:` line range that resolves in 154 but describes different
behaviour in 153 is exactly the class of false spec citation `check-firefox-refs` exists to
catch, and it would send the fix in the wrong direction the same way iter-137 went.

The specific question to answer: `devtools/server/actors/resources/index.js:380-390` binds the
resource callbacks to `rootOrWatcherOrTargetActor.notifyResources` — a three-way choice of
emitter — and `watcher.js:614` documents existing resources as arriving *"on related target
actors"*. `devtools/shared/specs/targets/window-global.js:143-150` confirms the target actor
declares `resources-available-array` in its own right. If enabling server-side target
switching moves the top-level target server-side and thereby moves resource emission onto the
**target** actor, then every network event is arriving with a `from` that
`is_watcher_event` rejects, and the daemon is discarding data it successfully asked for.

Use the **`rdp-spec-reviewer`** agent for this read.

Then choose between:

- **(a) Split the connection.** Give frame-target enumeration its own RDP
  connection/subscription with `isServerTargetSwitchingEnabled: true`, and let the daemon's
  core resource watcher go back to the default acquisition path (`get_watcher()`). Respects
  the `tab.rs:164-167` caution literally. Costs a second connection and a second watcher
  lifecycle to manage.
- **(b) Follow the routing.** Keep the flag and update `is_watcher_event` — and the
  buffer-insert path in `dispatch_firefox_message` — to accept resource events from the
  daemon's known target actors as well as its watcher actor. Cheaper, but `is_watcher_event`
  exists precisely to avoid stealing events belonging to a proxied command's own watcher
  (`daemon/server.rs:1347-1352`), so the widened predicate must stay narrow enough not to
  break the `watchResources` handshake forwarding.

**Do not pre-commit to either before the spec read.** Record the option chosen, the wire
evidence for it, and the `watcher.js` / `resources/index.js` lines that justify it, in
[[decision-log]] and in [[rdp/actors/watcher|kb/rdp/actors/watcher.md]]. Whichever is chosen,
iter-137's frame-target guarantee must survive — that is an AC, not a hope.

### Theme B — fix it, and prove both properties at once

Implement the chosen option. The bar is both of:

1. daemon-mode `network --source watcher` returns watcher-sourced entries with non-null
   `method` and `status` after a **plain** `navigate`;
2. `enumerate_frame_targets` through the daemon still returns the same non-zero frame count
   as `--no-daemon` on a multi-frame page.

Neither alone is sufficient. iter-137 had (2) and lost (1); the state before it had (1) and
lacked (2).

### Theme C — proof that cannot be faked, and a gate that cannot go quiet again

Every live assertion in this iteration must begin from a daemon buffer proven empty
(`daemon status --jq '.results.buffer_sizes'`) and must reach the page through the daemon
only. A test that permits a preceding direct `--with-network` call is measuring the
`store-events` workaround, not the watcher.

Un-`#[ignore]` `live_128_network_detail_uses_watcher`, or make `xtask live-sweep` classify and
execute it, so the network-fidelity tier stops being invisible. As-written the test navigates
with `--with-network` first, which makes it vulnerable to exactly the masking described above
— strengthen it to assert on a plain navigate, or add the plain-navigate assertion alongside.

### Theme D — cleanup, strictly AFTER Theme C is green

Order matters. Deleting the workaround before the watcher is verified removes the only thing
currently producing daemon-mode network data.

Once the ACs in Themes B and C are ticked with measured evidence:

- Delete `serialize_network_resources_for_buffer` (`commands/network_events.rs:220-286`) and
  the `store-events` RPC arm (`daemon/server.rs:2724-2761`).
- Delete the `network.rs` auto-fallback bookkeeping (~150-200 lines across
  `commands/network.rs:214-295` and `commands/network.rs:380-460`): the `Auto` variant's
  silent substitution, `used_perf_fallback` and its downstream branches, and the
  `source_reason` strings that exist only to explain a divergence that will no longer occur.
  **Keep `--source` as an explicit opt-out** — `--source performance-api` stays a supported
  request. What goes is the implicit switch.
- Also in Theme D, independent of the watcher but in the same command family:
  - `drain_network_events_timed` (`commands/network_events.rs:61-109`) is wall-clock: it
    loops until `start.elapsed() >= total_timeout` and never exits early, so
    `navigate --with-network` burns its full `--timeout` even when the page finished
    loading. Make the cutoff idle-based (stop after a quiet interval with no
    `resources-available-array` / `resources-updated-array`), keeping `total_timeout` as the
    hard ceiling and preserving the `timeout_reached` third return value's meaning.
  - `--with-network` and `--auto-consent` are mutually exclusive at the clap level
    (`cli/args.rs:1400`, `#[arg(long, conflicts_with = "with_network")]`). On any
    consent-walled site you therefore cannot dismiss the banner and capture the network in
    one call — the two things a real page needs together. Remove the conflict and make the
    consent step run inside the capture window.

## Acceptance Criteria [0/11]

- [ ] unit_159_daemon_resource_routing_pinned: a unit test asserts the daemon's resource-event
      acceptance rule against a `resources-available-array` fixture recorded from a live
      daemon session with `isServerTargetSwitchingEnabled: true` — the fixture's `from` field
      is asserted verbatim, and the test fails if the predicate drifts back to
      watcher-actor-only when the recorded `from` is a target actor. `kb/decision-log.md` and
      `kb/rdp/actors/watcher.md` record which of option (a) or (b) was chosen, with the
      `watcher.js` / `resources/index.js` line numbers that justify it.
- [ ] unit_159_establish_watcher_acquisition_path: a unit test pins `establish_watcher`'s
      `getWatcher` arguments to the option chosen in Theme A — under (a) the daemon's core
      watcher is created with no `isServerTargetSwitchingEnabled` argument and the
      frame-target subscription carries it on a separate connection; under (b) the core
      watcher keeps `Some(true)` and the test asserts the widened acceptance predicate is the
      companion change.
- [ ] live_159_daemon_watcher_captures_plain_navigate: on a daemon whose
      `daemon status --jq '.results.buffer_sizes'` reports 0 network events beforehand, a
      plain `navigate` (no `--with-network`) followed by
      `network --source watcher --detail` returns >= 1 entry, of which at least one has
      non-null `method` and non-null `status`. Measured baseline before the fix: 0 entries.
- [ ] live_159_daemon_direct_watcher_parity: the same page on the same Firefox instance
      yields `meta.source == "watcher"` and a non-zero entry count in **both** daemon mode
      and `--no-daemon` mode, with the daemon count within 20% of the direct count. Measured
      baseline before the fix: 0 daemon entries vs 10 direct entries.
- [ ] live_159_frame_targets_survive_the_fix: `click 'body' --frame <name>` through the daemon
      on a multi-frame page reports the same non-zero frame count as the same command with
      `--no-daemon`, and `daemon status --jq '.results.live_target_count'` is > 0 — iter-137's
      guarantee (`live_137_frame_targets_via_daemon`) holds under whichever option Theme A
      selected.
- [ ] live_128_network_detail_uses_watcher: passes with every entry reporting
      `source: "watcher"` and at least one entry carrying non-null `method` and non-null
      `content_type`, and it is executed by `cargo run -p xtask -- live-sweep` under
      `FF_RDP_LIVE_NETWORK_TESTS=1` — its name appears in the sweep's executed set and is
      counted in `LIVE_SWEEP_SUMMARY executed=N`.
- [ ] live_159_watcher_result_is_uncontaminated: the daemon-path assertion is made on a daemon
      buffer proven empty at the start of the test, with no `--no-daemon --with-network` call
      anywhere in the test body, so a pass cannot originate from `store-events` cross-path
      contamination. The test asserts `buffer_sizes` network count == 0 before navigate and
      > 0 after.
- [ ] unit_159_store_events_workaround_deleted: a source-audit unit test asserts that
      `daemon/server.rs` contains no `"store-events"` RPC arm and that
      `commands/network_events.rs` contains no `serialize_network_resources_for_buffer`
      symbol; the existing daemon RPC unit tests still pass with that arm gone.
- [ ] live_159_network_default_source_is_watcher: with the auto-fallback bookkeeping deleted,
      daemon-mode `network` with no `--source` flag reports `meta.source == "watcher"` and
      returns entries with non-null `method`, while `network --source performance-api` still
      returns Performance-API rows — the explicit opt-out survives the deletion.
- [ ] live_159_with_network_returns_on_idle: `navigate <quiet-page> --with-network
      --timeout 30000` returns in under 60% of the stated timeout once the resource stream
      goes idle, and still returns >= 1 network entry — `drain_network_events_timed` stops on
      idle rather than on wall clock.
- [ ] live_159_with_network_and_auto_consent_together: `navigate <consent-walled-url>
      --with-network --auto-consent` is accepted by the argument parser (exit code is not
      clap's 2) and a single invocation returns both a non-null `results.consent.cmp` and
      >= 1 network entry.

## Notes

- **Sequencing is load-bearing.** Theme D's deletions are gated on Themes B and C being
  ticked with measured evidence. If Theme A's spec read lands but Theme B slips, file the
  cleanup as a follow-up plan before this PR merges rather than deleting the workaround on a
  still-broken watcher.
- **`network`'s `--jq` shape divergence belongs to iteration 160, not here.**
  `commands/network.rs:326` folds `cli.jq.is_some()` into the `use_detail` flag, so passing
  `--jq` silently switches `results` from a summary object to an entries array — while the
  help text claims `--jq` operates on the full envelope. Verified network-only; `console`,
  `a11y`, `perf`, `sources` and `cookies` are single-shape. Cross-referenced here so it is
  not lost; do not fix it in this iteration, because changing the `--jq` result shape while
  also changing where the data comes from would make both changes unreviewable.
- The `live_128_*` test file also holds `live_128_network_text_width` and `live_128_meta_route`.
  Only the first of the three is in this iteration's scope; leave the other two alone.
- This is the same honesty family as [[iteration-153-launch-replace-double-envelope]] and
  [[iteration-149-a11y-restore-honesty]]: the command produced an answer, the answer was
  lower-fidelity than the one it claimed, and a compensating mechanism kept the discrepancy
  off-screen. The fix is not complete until the compensating mechanism is gone.
- Across iterations 135-151 the stated root cause diverged from reality at least eight times.
  The A/B table and the masking demonstration in this plan were captured from real runs.
  Reproduce both before changing anything, and re-measure after.
