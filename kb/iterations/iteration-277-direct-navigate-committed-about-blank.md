---
title: "Iteration 277: direct navigation reports HTTP 200 with committed about:blank"
type: iteration
date: 2026-09-19
status: planned
branch: iter-277/direct-navigate-committed-about-blank
# NN must be free: `ls kb/iterations/` before filing. check-iteration-plan fails when
# another plan already claims it. Letter suffixes (61b, 162a) are distinct numbers.
depends_on: []
# If this iteration introduces new pub items, list each one with its first call site.
# Leave empty ([]) if no new pub items are introduced.
# Required by cargo xtask check-iteration-plan when the body mentions pub symbols.
first_call_sites: []
# Describe how to manually exercise this iteration's output end-to-end.
# Required by cargo xtask check-iteration-plan.
dogfood_path: >-
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test
  live live_166_navigate_document_status::live_166_navigate_status_direct_parity
  -- --include-ignored --exact --nocapture --test-threads=1
tags: [iteration]
# Add `skill-edit` if this iteration modifies files under ~/.claude/skills/.
# Skill-edit iterations cannot run through ralph-loop (the cmux child workspace
# has no write access to ~/.claude/skills/). Drive them by hand in a regular
# Claude session. See iter-61z for the canonical example.
---

# Iteration 277: direct navigation reports HTTP 200 with committed about:blank

Filed from iteration271's closing dual-gate sweep on baseline
`2e604688d226f4e77303468c158f0d059a59cb44`, Firefox156.0. This is carry-over
only; no277 implementation or isolated rerun was performed.

## Measured failure

`live_166_navigate_document_status::live_166_navigate_status_direct_parity`
failed its committed-URL assertion at line283. The direct-route envelope was:

```json
{"navigated":"https://example.com?ff-rdp-cache-bust=1789824374155530000-1","status":200,"status_reason":null,"committed_url":"about:blank","ready_state":"complete","elapsed_ms":695}
```

Expected committed URL was
`https://example.com/?ff-rdp-cache-bust=1789824374155530000-1`.
The test uses a fresh cache-busting URL; this is not the historical304
assertion shape. The sweep had six CLI workers and was the only workflow
Cargo/Firefox workload; the user's desktop Firefox was preserved. There was
no watchdog expiry and no live-owned profile leak. Those facts
do not prove a contention cause or rule out a product defect. No raw RDP
event sequence or page snapshot was retained for this occurrence.

Evidence: primary checkout `.git/ralph-loop/20260919-queue/iter271/sweep.log`
and its `.meta` sidecar, `sweep-launches.log`, `actual-verdicts.tsv`.
Keep this distinct from BBC consent and daemon target-lifecycle findings
unless attributable evidence connects them.

## Tasks [0/3]

- [ ] Reproduce on an owned browser with the direct route and retain the
      requested URL, document identity/readiness, network status and navigation
      event ordering at the failed occurrence; retain passing controls.
- [ ] Identify why a successful status accompanies a stale committed URL;
      repair only the demonstrated cause without widening the assertion.
- [ ] Add a bounded regression that fails without the correction and passes
      with it, then run real-Firefox verification and the ordered gates/sweep.

## Acceptance Criteria [0/3]

- [ ] An attributable occurrence explains the `about:blank` committed URL;
      a passing isolated rerun alone does not count as an explanation.
- [ ] `live_166_navigate_status_direct_parity` still requires the canonical
      destination URL and correct HTTP status, with a regression demonstrating
      sensitivity to the repaired behavior.
- [ ] Ordered fmt/clippy/workspace-test gates and a final-source dual-gate
      sweep are recorded with all non-green outcomes explicitly dispositioned.

## Out of scope

Executing this plan as part of271; changing BBC consent, daemon handshake,
unrelated frame-target behavior, or accepting `about:blank` to turn a failure green.

## References

- [[iteration-271-bbc-consent-no-cmp-recurrence]]

## Later passing control — 2026-09-19

The required final-source sweep after271's bounded-diagnostic repair passed
`live_166_navigate_status_direct_parity`. Evidence:
`.git/ralph-loop/20260919-queue/iter271/repair1/sweep.log`. No navigation code
changed, no277 investigation or isolated rerun was performed, and the original
HTTP200/committed-`about:blank` failure and all ACs remain open. This pass is
not a causal explanation or a fix.
