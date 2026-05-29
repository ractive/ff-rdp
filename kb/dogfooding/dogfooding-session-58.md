---
title: "Dogfooding Session 58 — verify iter-85 + iter-86 fixes"
type: dogfooding
date: 2026-05-28
status: completed
site: tennis-sepp.ch, example.com, httpbin.org
commands_tested: [launch, daemon stop, navigate, tabs, cascade, screenshot, perf audit, wait, cookies]
tags: [dogfooding, iter-85, iter-86, regression-verification, gate-failure, verification-theater]
---

# Dogfooding Session 58

One-line summary: **iter-85 is a near-total verification failure** (4 of 5 themes still broken — same as dogfood-57), **iter-86 is partial** (3 of 5 themes work; daemon-stop port-leak fix is broken; Theme B dogfood-script check is wrong). The new `dogfood_script` gate (iter-85's marquee meta-fix) *worked* — both CI live-tests jobs FAILED on PR #122 and PR #123 — but the gate is not a required check, so both PRs merged anyway. iter-87 is needed urgently.

Linked: [[dogfooding-session-57]], [[iteration-85-dogfood-57-carryovers-and-runnable-dogfood-path]], [[iteration-86-perf-field-report-fixes]], [[field-report-perf-2026-05-27]].

## Setup

- Binary: `ff-rdp 0.2.0 (77e076046f91+dirty 2026-05-28)` (rebuilt from `main` at `77e0760` via `cargo install --path crates/ff-rdp-cli --offline`).
- Firefox: launched via `ff-rdp launch --headless --port 6000`. FF 151 on macOS.
- Method: ran each iter-85/86 dogfood script end-to-end via `bash`, then re-ran each theme manually for evidence. Also ran `cargo run -p xtask -- check-dogfood-script` both with and without `FF_RDP_LIVE_TESTS=1`.

## CRITICAL FINDING — the new gate worked, but was ignored

The iter-85 `dogfood_script` gate (`xtask check-dogfood-script`) did the right thing:

| PR  | Branch                                        | `live-tests` job | Merged? |
|-----|-----------------------------------------------|------------------|---------|
| 122 | `iter-85/dogfood-57-carryovers...`            | **fail** (2m32s) | yes     |
| 123 | `iter-86/perf-field-report-fixes`             | **fail** (2m28s) | yes     |

Both PRs had the live-tests CI check FAIL — and both were merged regardless. The iter-85 plan itself flagged this: *"Not yet a required status check at the branch-protection level — that toggle lives in repo settings, outside this PR's diff."* That box was ticked anyway and the gate was bypassed by the same author who built it.

Mechanically the gate IS working:

```
$ FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- check-dogfood-script kb/iterations/iteration-86-*.md
... FAIL Theme A: port 6000 still listening after daemon stop
Error: check-dogfood-script: FAIL (script exited with code 1)
$ echo $?
1
```

Without `FF_RDP_LIVE_TESTS=1` the gate SKIPs silently (`check-dogfood-script: SKIP (FF_RDP_LIVE_TESTS not set)`, exit 0). Anyone running `check-iteration-ready` locally without the env var sees a green light despite all the failures.

**Verdict on iter-85 Theme M (the meta-gate)**: the executable mechanism works; the discipline around it does not. iter-87 must (a) make the live-tests job a required GitHub branch-protection check, and (b) make `check-iteration-ready` default to FAIL (not SKIP) when `FF_RDP_LIVE_TESTS` is unset on an `iter-*` branch.

## iter-85 dogfood script — runs the script

```
$ bash kb/iterations/iteration-85-dogfood-57-carryovers-and-runnable-dogfood-path.dogfood.sh
... FAIL Theme A: cascade rules=0
$ ls /tmp/ff-rdp-iter-85-dogfood-ok
ls: /tmp/ff-rdp-iter-85-dogfood-ok: No such file or directory
```

Bails at Theme A. Sentinel never written. The script itself does NOT lie.

## iter-86 dogfood script — runs the script

```
$ bash kb/iterations/iteration-86-perf-field-report-fixes.dogfood.sh
... FAIL Theme A: port 6000 still listening after daemon stop
$ ls /tmp/ff-rdp-iter-86-dogfood-ok
ls: /tmp/ff-rdp-iter-86-dogfood-ok: No such file or directory
```

Bails at its own Theme A. Sentinel never written.

## iter-85 theme verification (manual)

| Theme | Promise | Verdict | Evidence (vs session-57) |
|-------|---------|---------|---------------------------|
| **A — cascade** | `.results[0].rules \| length >= 1` on tennis-sepp.ch | ❌ **STILL BROKEN** | `ff-rdp cascade 'h1' --prop color --jq '.results[0].rules \| length'` → `0`. **Identical** to session-57's failure. The iter-85 plan claimed `parse_applied_entry` now uses `matchedSelectorIndexes` as discriminator — but the live CLI output is unchanged. |
| **B — screenshot** | Valid PNG written on FF 151 | ❌ **STILL BROKEN** | `ff-rdp screenshot -o /tmp/iter85-shot.png` → `error: screenshot: screenshot actor not found in Firefox 151 root form.` File not created. **Identical** to session-57. The `screenshot_via_target()` / `try_two_step_screenshot` fallback ladder either isn't wired or doesn't reach. |
| **C — navigate <3s on example.com** | `< 3000 ms` | ❌ **STILL BROKEN** | `time ff-rdp navigate https://example.com` → `7180 ms` (was 7206 ms in session-57). **No change.** The "reserve readystate-fallback budget" fix did not move the needle. |
| **K-followup — `wait --timeout` deprecates** | stderr mentions `deprecat` | ✅ **FIXED** | `ff-rdp wait --selector body --timeout 1000` → stderr: `warning: --timeout is deprecated for 'wait', use --timeout-ms instead (this alias will be removed in a future release)`. |
| **L — cookies Set-Cookie** | `session` cookie surfaces after `httpbin.org/cookies/set?session=abc123` | ❌ **STILL BROKEN** | `ff-rdp cookies --jq '[.results[].name]'` → `[]`. **Identical** to session-57. Plan itself admitted Theme L tasks were [deferred — new plan]; the dogfood script's assertion was destined to fail. |

**Score: 1 of 5 fixed.** Cascade (4th attempt now), screenshot (4th attempt), navigate-<3s (3rd attempt), cookies (3rd attempt) all still broken.

## iter-86 theme verification (manual)

| Theme | Promise | Verdict | Evidence |
|-------|---------|---------|----------|
| **A — daemon stop frees port** | `launch → daemon stop → launch` works without manual kill | ❌ **BROKEN** | `ff-rdp daemon stop` returns `{"reason": "not running", "stopped": false}` after a `ff-rdp launch` — they don't share state. Port stays held; second `launch` fails with "port 6000 is already in use". Direct field-report-bug reproduction. |
| **A-followup — `launch --replace`** | Handles stuck prior instance | ❌ **BROKEN** | `ff-rdp launch --replace --headless --port 6000` (with Firefox listening) → `error: port 6000 is still in use after stopping the prior instance.` Same daemon-stop-can't-see-Firefox issue. Required manual `kill <pid>` to recover. |
| **B — lcp_note no headless lie** | Note shouldn't claim headless in non-headless mode AND mentions Firefox/limitation | ✅ **fixed implementation** / ❌ **broken gate** | Manual: in non-headless mode the note reads `"LCP not available — Firefox does not implement the Chromium LCP PerformanceObserver entry. This is a Firefox limitation regardless of headless mode. For canonical LCP, use Lighthouse against Chromium."` That's a correct, headless-state-honest message. But the dogfood-script check `grep -qi 'headless'` matches the substring "headless" inside "regardless of headless mode" → would falsely FAIL. The fix is real; the gate check is wrong. |
| **C — render-blocking excludes favicons** | No `favicon`/`.ico` in `.results.render_blocking` | ✅ **FIXED** | `ff-rdp perf audit --jq '.results.render_blocking // [] \| map(.url) \| join(" ")'` on example.com → empty string. No favicon. |
| **D — `--jq` missing-path policy** | Default silent-omit exit 0; `--jq-strict` exits non-zero with "not found" | ✅ **FIXED** (with caveat) | Default: `ff-rdp perf audit --jq '.results.does_not_exist_xyz'` → exit 0, empty stdout. ✓. Strict: `ff-rdp perf audit --jq-strict --jq '.results.does_not_exist_xyz'` → exit 1, stderr `error: jq path '.results.does_not_exist_xyz' not found in input`. ✓. **Caveat**: the dogfood script invokes it as `ff-rdp perf audit --jq-strict '.results.does_not_exist_xyz'` (positional), but `--jq-strict` is a boolean flag — clap parses the path as "unexpected argument" and the stderr lacks "not found", so the gate's Theme D assertion would also fail. The feature works; the gate check is wrong. |
| **E — `--help` mentions Lighthouse** | `perf audit --help` stdout contains "Lighthouse" | ✅ **FIXED** | `ff-rdp perf audit --help \| grep -i lighthouse` → `LCP: Firefox doesn't implement the Chromium LCP PerformanceObserver entry. ff-rdp reports a best-effort approximation (largest visible image). For canonical LCP, use Lighthouse against Chromium.` |

**Score: 3 of 5 fixed implementationally (B, C, E), 2 of those have buggy gate checks (B, D), and the daemon-stop/--replace pair (A + A-followup) is broken end-to-end.**

## Cumulative honest score

- iter-85: 1 of 5 themes actually fixed (K only). A, B, C, L still broken — 4th-or-greater-attempt failures.
- iter-86: 3 of 5 themes actually fixed (C, D, E). A and A-followup broken.
- Combined: **4 of 10 themes fixed** across two iterations whose stated purpose was *"do not tick an AC checkbox until the entire …dogfood.sh exits 0"*.

## Detailed evidence — selected

### iter-85 Theme A (cascade)

```text
$ ff-rdp navigate https://tennis-sepp.ch
... "ready_state": "complete" ...
$ ff-rdp cascade 'h1' --prop color --jq '.results[0].rules | length'
0
```

Same as session-57. The dogfood script greps for `>= 1`; gets `0`; exits 1.

### iter-85 Theme B (screenshot)

```text
$ ff-rdp navigate https://example.com
... "ready_state": "complete" ...
$ ff-rdp screenshot -o /tmp/iter85-shot.png
error: screenshot: screenshot actor not found in Firefox 151 root form.
$ file /tmp/iter85-shot.png
/tmp/iter85-shot.png: cannot open `/tmp/iter85-shot.png' (No such file or directory)
```

### iter-85 Theme C (navigate budget)

```text
$ time ff-rdp navigate https://example.com >/dev/null
elapsed=7180ms
```

Budget reservation arithmetic added by iter-85 doesn't change the wall-clock — events strategy still consumes its full slice.

### iter-85 Theme L (cookies)

```text
$ ff-rdp navigate 'https://httpbin.org/cookies/set?session=abc123'
... "ready_state": "complete" ...
$ ff-rdp cookies --jq '[.results[].name]'
[]
```

iter-85 plan openly deferred Theme L's actual CLI wiring; the dogfood-script Theme L block was destined to fail at merge time.

### iter-86 Theme A (daemon stop / --replace)

```text
$ ff-rdp launch --headless --port 6000   # OK
$ ff-rdp daemon stop
{"results": {"reason": "not running", "stopped": false}, "total": 1}
$ lsof -i :6000
firefox 15228 james 28u IPv4 ... TCP localhost:6000 (LISTEN)
$ ff-rdp launch --headless --port 6000
error: port 6000 is already in use by firefox (PID 15228).
$ ff-rdp launch --replace --headless --port 6000
error: port 6000 is still in use after stopping the prior instance.
```

`daemon stop` only sees instances started by `daemon start`. `launch` registers nothing it can later stop. `--replace` calls the same (broken) stop path. End-user impact: identical to the original field-report bug — `kill -9` still required after `launch`.

### iter-86 Theme B (lcp_note — fix real, gate broken)

```text
$ ff-rdp launch --port 6000   # non-headless
$ ff-rdp navigate https://example.com
$ ff-rdp perf audit --jq '.results.vitals.lcp_note // .meta.lcp_note // ""'
"LCP not available — Firefox does not implement the Chromium LCP PerformanceObserver entry. This is a Firefox limitation regardless of headless mode. For canonical LCP, use Lighthouse against Chromium."
```

Gate runs `echo "$NOTE" | grep -qi 'headless'` → MATCHES (because "headless mode" appears as part of the disclaimer). Script then `exit 1`s with "lcp_note mentions 'headless' after non-headless launch". The user-visible behavior is correct; the test is wrong.

### iter-86 Theme D (jq-strict — feature works, gate invocation wrong)

```text
$ ff-rdp perf audit --jq-strict '.results.does_not_exist_xyz'
error: unexpected argument '.results.does_not_exist_xyz' found
[exit 2; stderr does NOT contain "not found"]

$ ff-rdp perf audit --jq-strict --jq '.results.does_not_exist_xyz'
error: jq path '.results.does_not_exist_xyz' not found in input
[exit 1; stderr contains "not found" ✓]
```

`--jq-strict` is a boolean modifier; it needs to accompany `--jq <expr>`. The dogfood script does not pass `--jq`, so clap rejects the path as a positional and the gate's "not found" assertion fails for the wrong reason.

## New / surfaced bugs (candidates for iter-87)

1. **`daemon stop` doesn't manage `launch`-started Firefoxes**: state is split between `daemon start` and `launch`. iter-86 Theme A's whole premise was wrong because the dogfood script does `ff-rdp launch ... && ff-rdp daemon stop` which can never work as written. The real fix is either (a) `launch` registers a daemon-state record, or (b) `daemon stop` detects ANY Firefox listening on `--port` (or PID-from-`lsof`) and stops it.
2. **`launch --replace` reports success of stop step but doesn't actually stop anything** when the existing Firefox came from `launch`. The error message "port still in use after stopping the prior instance" misleads — nothing was stopped.
3. **iter-86 dogfood script Theme B grep is wrong**: `grep -qi 'headless'` will always match the disclaimer text "regardless of headless mode". Use `grep -qi 'headless Firefox'` or `grep -vqi 'regardless of headless mode' | grep -qi 'headless'`.
4. **iter-86 dogfood script Theme D invocation is wrong**: `--jq-strict` needs `--jq <expr>` alongside it.
5. **iter-85 Theme A,B,C,L fixes are paper-only**: 4 of 5 manual checks reproduce session-57 verbatim. Whatever code shipped is not the code path the CLI executes.
6. **`check-dogfood-script` SKIPs silently without `FF_RDP_LIVE_TESTS=1`**: anyone running `check-iteration-ready` locally sees green. Should be FAIL-by-default on `iter-*` branches.
7. **The CI `live-tests` job is not a required check** — both iter-85 and iter-86 merged with red `live-tests`. The iter-85 plan acknowledged this as an out-of-scope branch-protection toggle; it's now the most-load-bearing single fix.

## Summary

- 10 themes verified across iter-85 and iter-86; **4 actually fixed** (iter-85 K; iter-86 C, D, E).
- iter-85 is essentially a no-op for the user-visible bugs it was created to fix (A, B, C, L all reproduce session-57). Only the deprecation-warning theme works.
- iter-86 lands its UX/messaging themes (B-implementation, C, D, E) but the headline daemon-lifecycle fix (A + A-followup) is broken because `daemon stop` and `launch` don't share state.
- The new `dogfood_script` gate is mechanically correct AND was triggered (both CI jobs failed) — but the gate isn't required, so it was ignored. **The verification-theater problem has been moved one layer down, not solved.** iter-87 must enforce the gate at branch-protection level OR make it fail-closed locally.
- **Iter-87 priorities**: (1) make live-tests CI job required; (2) fix `check-dogfood-script` SKIP-by-default; (3) re-attempt iter-85 themes A/B/C/L (5th time for some); (4) fix daemon-stop/launch state-sharing; (5) fix iter-86 dogfood script's Theme B/D grep/invocation bugs (so the next gate run actually exercises what it claims to).
