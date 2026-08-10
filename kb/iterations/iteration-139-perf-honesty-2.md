---
branch: iter-139/perf-honesty-2
date: 2026-08-09
depends_on:
  - kb/iterations/iteration-131-measurement-honesty.md
dogfood_path: |
  ff-rdp launch --headless --port 6100
  ff-rdp navigate https://www.bbc.com/news --port 6100
  ff-rdp perf vitals --port 6100
  # → cls/tbt must NOT report a fabricated "good"; Firefox cannot measure them
  ff-rdp perf audit --port 6100
  # → resource_by_type document bytes must not contradict navigation.transfer_size
  # → third_party_summary.count must exclude first-party
  ff-rdp perf summary --format text --port 6100 | awk '{print length}' | sort -rn | head -1
  # → longest line must be bounded (~120), not 6709
first_call_sites: []
status: planned
---

# Iteration 139: perf honesty II — unmeasurable vitals, byte attribution, page identity

Follow-up to [[iteration-131-measurement-honesty]] and [[iteration-125-perf-audit-lcp-unavailable]],
from [[dogfooding-session-63]]. Same false-good class the project already fixed once for LCP.

## Themes

### Theme A — CLS and TBT are fabricated

```
$ ff-rdp perf vitals
  "cls": 0.0, "cls_rating": "good", "tbt_ms": 0.0, "tbt_rating": "good",
$ ff-rdp eval 'JSON.stringify(PerformanceObserver.supportedEntryTypes)'
["event","first-input","largest-contentful-paint","mark","measure","navigation","paint","resource"]
```

No `layout-shift`, no `longtask` — Firefox **structurally cannot measure these**, and won't
for the foreseeable future. Reporting `0.0` with a `"good"` rating on an ad-heavy BBC News
page is a confident lie, and it is exactly the failure mode iter-125 fixed for LCP.

LCP already carries an honest `lcp_note`. Give CLS and TBT the same treatment: `null` plus a
note naming the missing entry type, or an explicit `"unavailable"` rating. A rating of "good"
must never be emitted for a metric that was never observed.

Check the whole vitals surface for siblings — anything else derived from an unsupported entry
type has the same problem.

### Theme B — `perf audit` byte attribution is self-contradictory

```
"navigation": {"transfer_size": 64035},
"resource_by_type": [{"type":"font","count":6,"transfer_size":400104},
                     {"type":"image","count":18,"transfer_size":2700},
                     {"type":"document","count":6,"transfer_size":300}]
"third_party_summary": {"count":140,"transfer_size":515702}   # total count is also 140
```

Three defects in one output:
1. The main document is 64035 B per `navigation` but `resource_by_type.document` totals 300 B.
2. Fonts appear to be 78 % of the page only because they are same-origin while images are
   opaque — `300` is the opaque placeholder. iter-131 flags `transfer_size_opaque` at summary
   level but **not** on the per-type and per-domain breakdowns an agent actually reads.
3. `third_party_summary.count` equals the total count — the first-party document and
   `static.files.bbci.co.uk` are counted as third-party. "100 % of your bytes are third-party"
   is wrong.

Propagate iter-131's opaque handling into every breakdown, and fix first-party detection.

### Theme C — `perf vitals` has no page identity

After an `emulate --offline on` + failed navigation, vitals returned `fcp_ms 18125.0 / poor`,
`ttfb_ms 18085.0 / poor` for gov.uk; a clean re-navigation gave `fcp 173 / ttfb 108`. The
output contains **no URL and no timestamp** — nothing says which navigation it measured.

Add page identity (URL + when the measurement was taken, or the navigation id) so stale data
is detectable. Relates to iter-131's `resources_pending`, which closed the zero case but not
the stale case.

### Theme D — `perf summary --format text` untruncated URLs

Session-62 issue 2, still present: "Top 5 Slowest Resources" lines of 6709 and 7378 chars.
iter-128's `middle_ellipsis` was wired into `network` and `sources` but not here. Apply the
existing helper; do not write a second one.

## Acceptance Criteria

- [ ] live_139_cls_unavailable: on a real page, `cls` is null/`"unavailable"` with a note —
      never `0.0` rated `"good"`
- [ ] live_139_tbt_unavailable: same for TBT
- [ ] live_139_audit_document_bytes_agree: `resource_by_type.document` does not contradict
      `navigation.transfer_size`
- [ ] live_139_audit_opaque_flagged_per_type: per-type and per-domain breakdowns carry the
      opaque marker when they contain opaque resources
- [ ] live_139_third_party_excludes_first_party: `third_party_summary.count` < total on a page
      with same-origin resources
- [ ] live_139_vitals_page_identity: vitals output names the URL it measured
- [ ] live_139_perf_summary_text_bounded: longest `perf summary --format text` line bounded on
      an ad-heavy page
- [ ] unit_vitals_unsupported_entry_types: unit coverage that an unsupported entry type yields
      unavailable, not zero

## Notes

- The governing rule, third time in this project: **a metric that cannot be measured must not
  be reported as a good score.** See [[iteration-125-perf-audit-lcp-unavailable]].

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
