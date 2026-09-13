---
title: "Iteration 257: Firefox 155 changed drawSnapshot's 4th argument to a dictionary and every --full-page screenshot is broken"
type: iteration
date: 2026-09-07
status: done
branch: iter-257/firefox-155-drawsnapshot-dictionary-arg
firefox_refs:
  - path: dom/chrome-webidl/WindowGlobalActors.webidl
    lines: "90-102"
    why: Firefox 155.0.1 release snapshot dictionary members, not the stale July 8 checkout
  - path: dom/chrome-webidl/WindowGlobalActors.webidl
    lines: "227-230"
    why: Firefox 155.0.1 release snapshot drawSnapshot signature
  - path: devtools/server/actors/utils/capture-screenshot.js
    lines: "115-120"
    why: Firefox 155.0.1 primary helper already passes the dictionary
depends_on:
  - iteration-92-full-page-and-navigate-parity
first_call_sites: []
dogfood_path: |-
  # Run the checked-in current-tree dogfood script, retaining its sentinel proof:
  FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- check-dogfood-script kb/iterations/iteration-257-firefox-155-drawsnapshot-dictionary-arg.md
  # For the minimum-supported120 check, explicitly set FF_RDP_257_FIREFOX to the
  # verified absolute Firefox120 executable and FF_RDP_257_EVIDENCE_DIR to a
  # fresh evidence directory. The script uses a fresh unmanaged profile/port.
  # FF_RDP_FIREFOX_PATH selects source for reference checks, NOT the runtime;
  # changing PATH does not override /Applications/Firefox.app on macOS.
  # Expected: decoded viewport1366x683/fullpage1366x4000 at the measured1x viewport,
  # with scrollY500, viewport dimensions and fixed-header style unchanged.
  # Required per-iteration closing sweep, preserving all five tiers and names:
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
tags: [iteration, screenshot, firefox-155, spec-drift, regression, ff-rdp-core, carry-over]
takeover_reconciliation_252: "2026-09-09: correct source path is dom/chrome-webidl/WindowGlobalActors.webidl. Release155.0 introduced DrawSnapshotOptions{resetScrollPosition=false,drawView=false}, bug2058388 final commit78f289876b9c2022059c951097c71713142c67a0;154.0.1 and120 use boolean. Local Firefox source0088392 predates this change: use recorded release tags. ThemeB premise needs runtime investigation: CLI screenshot.rs607 unconditionally routes fullpage to process fallback, so sweep failures do NOT establish primary actor failure on155. The155 primary helper already uses the dictionary. Dictionary-first try/catch cannot detect old Firefox because objects coerce to boolean true. Original ACs remain intact; oldest-supported120 runtime proof still required. Official120 DMG downloaded and both SHA256/SHA512 verified, not installed/launched. Evidence saved in takeover firefox257-prep; https://bugzilla.mozilla.org/show_bug.cgi?id=2058388."
dogfood_script: iteration-257-firefox-155-drawsnapshot-dictionary-arg.dogfood.sh
---

# Iteration 257: Firefox 155 changed `drawSnapshot`'s 4th argument to a dictionary and every `--full-page` screenshot is broken

> Filed 2026-09-07 as carry-over from [[iteration-235-live-bulk-cap-shrinks-a-process-global]]'s
> closing live sweep. Seven of that sweep's eleven failures are this single TypeError — every
> `--full-page` capture in the suite and nothing else — and it is a
> **product** defect, not a test-harness one: the CLI returns `error_type: "User"` and no image.

## The defect

`crates/ff-rdp-core/src/actors/screenshot.rs` builds parent-process JS that calls

```js
const snapshot = await wg.drawSnapshot(rect, 1, "rgb(255,255,255)", true);
```

The comment above it records the contract iteration 92 measured:

> The 4th arg to `drawSnapshot(rect, scale, color, resetScrollPosition)` is `resetScrollPosition`
> (see `dom/webidl/WindowGlobalActors.webidl`), NOT a fullpage flag.

On Firefox 155.0.1 that is no longer true. The call throws
`TypeError: WindowGlobalParent.drawSnapshot: Argument 4 can't be converted to a dictionary.`
before rendering anything, so the fallback path returns an error string and the CLI reports a
failed capture.

This is exactly the class of change `// allow-spec-drift` exists to track, and the call site
already carries `// allow-spec-drift: bug TBD (SD-2: BrowsingContext.drawSnapshot used via
…)` — the drift annotation was there, the *verification against a current Firefox* was not.

## Themes

- **A — Establish the new signature from Firefox source, not by guessing.** Read
  `dom/webidl/WindowGlobalActors.webidl` in the 155 tree (`FF_RDP_FIREFOX_PATH`) and record
  the dictionary's actual member names in `kb/rdp/flows/take-screenshot.md`. Do not ship a
  speculative `{resetScrollPosition: true}` without having read the IDL — a dictionary silently
  ignores unknown members, so a wrong member name produces a screenshot that is quietly wrong
  rather than an error, which is worse than today's loud failure.
- **B — Why is the fallback reached at all?** All seven failures go through
  `screenshot_via_process_drawsnapshot`, i.e. the *primary* screenshot-actor path had already
  failed and the CLI fell back. Non-`--full-page` captures pass, so the primary path works for
  them. Establish why `--full-page` always lands in the fallback on FF 155 before assuming the
  fallback is the whole story — a primary path that handled full-page would make the drift moot.
- **C — Version compatibility.** ff-rdp advertises a minimum supported Firefox of 120. If the
  dictionary form is 155-only, the call needs to work on both; decide between feature-detection
  in the JS (`try` the dictionary, fall back to the boolean) and a version gate, and say which
  and why.
- **D — The misleading hint.** On a TypeError the CLI currently advises "Very tall pages can
  exceed the renderer's limits — retry without `--full-page`". That sentence is true of a real
  renderer limit and false of a signature change, and it sent this sweep's first reading down the
  wrong path. The hint must be conditional on the failure actually looking like a size limit.

## Tasks

### A. Root cause [2/3]
- [x] Read `dom/webidl/WindowGlobalActors.webidl` at the Firefox 155 tag and record the current
      `drawSnapshot` signature and dictionary members in `kb/rdp/flows/take-screenshot.md`
- [x] Determine which Firefox release changed it, and note it against the minimum supported 120
- [ ] Establish why the primary screenshot-actor path also failed on FF 155 (Theme B)

### B. The fix [2/2]
- [x] Pass argument 4 in the form the running Firefox accepts, working on 155 and on the oldest
      supported build
- [x] Stop the "very tall pages" hint from firing on a `TypeError` (Theme D)

### C. Proof [2/2]
- [x] The seven failing live tests green on FF 155
- [x] A live test that fails loudly if `drawSnapshot` ever again throws rather than rendering —
      per CLAUDE.md, a spec-method change needs a live Firefox test, not a unit test

## Acceptance Criteria [4/4]

- [x] `ff-rdp screenshot --full-page` produces a PNG taller than the viewport on Firefox 155.0.1
- [x] All seven pass in one sweep: `live_92_screenshot_full_page` (both tests),
      `live_61l::live_screenshot_full_page`, `live_61r_screenshot::live_screenshot_full_page`,
      `live_135_screenshot_full_page_taller`, `live_144_full_page_no_duplicate_header` and
      `live_screenshot_shim::live_screenshot_unchanged_after_shim`
- [x] The `// allow-spec-drift` comment at the call site names a real Bugzilla issue, or states
      the measured 155 signature — `bug TBD` is no longer honest for a drift we have now hit
- [x] No error path claims a page-size limit for a failure that was a `TypeError`

## Out of scope

- The other four failures in iteration 235's sweep. They are recorded with their dispositions in
  that iteration's PR body; two are the load-sensitive pair iteration 210 already tracks.

## Implementation evidence, 2026-09-14

The original tasks and AC wording above are retained. Task A's path is historical:
the verified source is **`dom/chrome-webidl/WindowGlobalActors.webidl`**, at the
Firefox 120.0, 154.0.1, 155.0 and 155.0.1 release tags. The 155 snapshots declare
`DrawSnapshotOptions { resetScrollPosition = false, drawView = false }`; 120 and
154 retain the boolean. First stable change: 155.0, Mozilla bug 2058388, final
commit `78f289876b9c2022059c951097c71713142c67a0`. [[take-screenshot]] carries
the official source links and detailed semantics. The Firefox-ref gate uses an
exact saved 155.0.1 release-file snapshot, **not** the local July 8 checkout.
Hyalo does not support writing nested mapping values; `firefox_refs` was edited
directly after verifying that limitation, while supported status/task changes
use Hyalo.

The fallback tries the legacy boolean and retries only the exact argument-4
dictionary TypeError, once, with the same reset value and `drawView: false`.
Other failures are returned unchanged. Dictionary-first is not feature detection:
old WebIDL would silently coerce any object to true. The CLI now adds tall-page
advice only for an explicit dimension-limit diagnostic and never for TypeError.
Viewport/null-rect and full-page/document-rect behavior, scale, direct connection,
PNG output, and fixed/sticky restoration remain intact.

Actual runnable dogfood results: both **Firefox 120.0** (absolute extracted
official runtime, PID 23116, raw debugger 31745, fresh unmanaged profile) and
**Firefox 155.0.1** produced viewport **1366×683** and full-page **1366×4000** PNGs.
PNG IHDR bytes agree with the JSON dimensions. `scrollY=500`, viewport dimensions,
and the fixed header's original style were identical before and after capture.
The 120 trace explicitly enters the process-drawsnapshot path. Its browser was
terminated and waited for; the desktop browser was preserved. Runtime identity,
commands, JSON, PNGs and traces are retained in
`.git/ralph-loop/20260912-validation-efficiency/iter257-implementation/`.

Four core unit tests check legacy true/false arguments, dictionary retry values,
unrelated-error propagation and bounded retry. Three focused CLI capture tests
pass, including TypeError exclusion and explicit size diagnostics. The new
`live_257_drawsnapshot_renders_both_argument_modes_without_changing_scroll`
directly calls the core fallback for viewport and full-page captures and passes
on 155. Deliberately disabling the retry makes both its non-live retry test and
the live guard fail with the dictionary TypeError; the source was then restored
byte-for-byte. The first guard attempt had a test JSON-parser mistake before
capture; that was corrected, without changing the product contract.

The first two 155 dogfood attempts reached the debugger before a first tab was
available; the retained second error is `no tabs available`. A bounded first-tab
readiness check fixes this script setup race, matching the shared live harness.
The third attempt passed. Those failures do not establish a screenshot defect.

Task A3 remains unticked because its premise is false in the existing CLI:
`full_page` unconditionally bypasses `ScreenshotActor::capture` to retain the
Firefox 151 viewport-clamp workaround. The observed full-page trace names that
bypass. The 155 upstream primary helper already supplies the dictionary; the
runtime primary-path probe directly called `prepareCapture(fullpage=true)` and
`ScreenshotActor::capture`: it returned a decoded **1366×4000** PNG. The primary
actor therefore did not fail in this measured 155.0.1 run. The bypass remains to
preserve the historical older-build workaround and full-page cleanup contract.

The own closing dual-gate sweep ran on the frozen implementation tree
`5e4d8f71e30dbe0ecb4abdd63d374386f9249bd3`; subsequent changes are closing
documentation only. All five compiled ignored-test inventories exactly match
the actual verdicts, with no missing, duplicate or extra names:

| Tier | Passed | Failed | Total |
|---|---:|---:|---:|
| CLI `live` | 333 | 2 | 335 |
| Core `live_129_frame_targets` | 1 | 0 | 1 |
| Core `live_61p_registry` | 3 | 0 | 3 |
| Core `live_61u` | 3 | 0 | 3 |
| Core `live_firefox_test` | 2 | 0 | 2 |
| **All tiers** | **342** | **2** | **344** |

```text
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
LIVE_SWEEP_SUMMARY executed=344 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=344
LIVE_SWEEP_PROFILES leaked=0 unattributed=0 root=/Users/james/Library/Application Support/ff-rdp/profiles
```

The runner emitted one final profile summary above, plus five clean per-tier
profile-scan notes; all six lines are retained in `all-profile-accounting.txt`.
Raw Firefox155 PID47188 used a fresh unmanaged profile on port6000 with the
three debugger preferences. Ten-second PID/listener/load samples span the sweep.
The controller stopped and waited for it. A later supervisor check at
2026-09-14 01:00:43 CEST found no listener on port6000 and confirmed the
pre-existing desktop PID37270 (started September8) was still alive. Its exact
commands, exit codes and output are retained in
`.git/ralph-loop/20260912-validation-efficiency/iter257-supervisor-verification/post-cleanup-processes.json`;
this is a later observation, not a claim of continuous availability. Default jobs6 and watchdog300/900
were unchanged. The separate owed242 negative below remains separate evidence.

**All seven original regressions passed in that one sweep**: both
`live_92_screenshot_full_page` cases, `live_61l::live_screenshot_full_page`,
`live_61r_screenshot::live_screenshot_full_page`,
`live_135_screenshot_ff153::live_135_screenshot_full_page_taller`,
`live_144_session_hygiene_followup::live_144_full_page_no_duplicate_header`, and
`live_screenshot_shim::live_screenshot_unchanged_after_shim`. The new257 direct
fallback guard and the separate61r DPR2 guard also passed.

Ordered Rust gates passed after restoring the mutation: stable1.98.1;
`cargo fmt`; strict workspace Clippy; `cargo test --workspace -q` with
2471passed/0failed/416ignored across36 summaries. Independent review and final
publication are supervisor-owned; no review verdict is claimed by this worker.

All nine actual xtask `check-*` commands passed: iteration-plan directory sweep,
source invariants, release155 Firefox references, actor/KB sync against
`1c3e9e66e91bdde5f94825609f96620618f94d8b`, live-test layout, executed dogfood,
skill drift, help idioms, and vendored JavaScript. Hyalo HYALO005 also passed.

## Carry-over

| Observation or unmet item | Disposition and evidence |
|---|---|
| Guardian137 sweep `consent_no_cmp` after target readiness106ms/1poll; exact isolation passes5.09s, readiness18ms, Sourcepoint accepted | **Fold:** [[iteration-262-daemon-live-target-never-promoted]]. New distinct detection outcome; no promotion failure, detected-but-not-actioned result, banner DOM or common cause is claimed. Original three-green-sweep requirement remains unmet. |
| BBC144 sweep `consent_no_cmp`; exact isolation fails4.77s with the same envelope | **Fold:** [[iteration-271-bbc-consent-no-cmp-recurrence]]. Preserve the separately owed242 failures and this new recurrence; failed-page/banner evidence remains missing. No consent change or weaker assertion here. |
| A3 asks why the primary actor failed | **No plan:** false premise. The CLI bypassed it, and the direct155 probe rendered1366×4000 successfully. Original task remains honestly unticked. A separately observed primary failure would require its own investigation. |
| First new live-guard attempt read eval's obsolete nested `result` shape and failed before capture | **Closed in this PR:** test reads the actual direct result string; positive live run and expected-failing compatibility mutation verify the corrected instrument. |
| Two initial155 dogfood attempts failed before a first tab was available; first lacked retained diagnostics, second retained `no tabs available` | **Closed in this PR:** bounded first-tab setup wait and evidence-on-exit capture; third attempt and the final xtask dogfood passed. The first failure has no independently retained envelope, so its attribution rests on the exact retry, not invented output. |
| Initial focused CLI test invocation used nonexistent `--lib` target | **No plan:** command corrected to `--bin ff-rdp`; three capture tests passed. No product failure occurred. |
| The earlier owed242 sweep failed seven screenshots and five other cases | **Preserved separately:**242's negative evidence and original four unmet requirements remain unchanged. Only the screenshot mechanism is repaired here; owners203/260/262/267/271 retain their other obligations. |

The reconciliation inventory contains sixteen upcoming planned iterations plus
the active257 plan (seventeen in the initial pending set). Full frontmatter and
bodies, including metadata-only265–267, were inspected. Dated version hashes and
individual dispositions are retained in the implementation evidence directory.
Only invalidated validation guidance and new measured carry-over evidence were
updated; none of the broader backlog was implemented.

## References

- `crates/ff-rdp-core/src/actors/screenshot.rs` — the `drawSnapshot` JS and its `allow-spec-drift`
- `crates/ff-rdp-cli/src/commands/screenshot.rs` — the fallback and the misleading hint
- `kb/rdp/flows/take-screenshot.md`, `kb/research/screenshot-protocol-ff149.md`
- [[iteration-92-full-page-and-navigate-parity]] — where the `resetScrollPosition` contract was measured
- [[iteration-235-live-bulk-cap-shrinks-a-process-global]] — the sweep that found this


## Separately owed242 sweep recurrence, 2026-09-14

At mergedmain `19f4e236a70399c2984e6d46eb15bdac37a55181`, all seven original full-page tests failed
with the same dictionary TypeError and misleading page-size hint, then all seven
failed the exact serial dual-gate isolation with that same signature. Full run:
343=331pass+12fail, all five tiers and names reconciled, no skips/reclassifications
or profile leaks. This is new negative evidence for257, not its closing sweep.
Original tasks, ACs and mandatory oldest-supported120 runtime proof stay unchanged.
The five non-screenshot failures are individually dispositioned in242's evidence
section; none is claimed caused by screenshot code. Evidence: `.git/ralph-loop/20260912-validation-efficiency/iter242-owed-sweep/`.

## Final local review, 2026-09-14

Independent source review of tree`5e4d8f71e30dbe0ecb4abdd63d374386f9249bd3`
returned no findings. The closing documentation review identified one missing
retained cleanup observation; the two affected references now identify the later
timestamped supervisor check. Fresh scoped review of that correction returned
no findings (session`01a09d01-c316-7f30-82de-32fbfe8e65a4`). One review repair
batch was used. Source, tests and dogfood script are unchanged from their passing
ordered gates and closing sweep; the affected plan and Hyalo checks passed.
The original primary-failure task remains unticked for the reason above.
