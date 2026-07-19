---
branch: iter-128/network-hint-always-present
date: 2026-07-19
depends_on:
  - kb/iterations/iteration-126-network-json-shape-consistency.md
dogfood_path: |
  ff-rdp launch --headless --auto-consent
  ff-rdp navigate https://example.com --with-network --jq '.results.network | has("hint")'
  # → true on a quiet page (value null), same key set as a busy page
  ff-rdp navigate https://en.wikipedia.org/wiki/Firefox --with-network --jq '.results.network.hint'
  # → "showing 20 of N, use --all for complete list"
first_call_sites: []
status: planned
---

# Iteration 128: canonical network object leaks a conditional `hint` key (iter-126 escape)

Found by the 2026-07-19 post-batch live sweep, deterministic (failed in the full
run and again serialized): `live_126_network_shape::live_navigate_with_network_shape_quiet_and_busy`
asserts the quiet (example.com) and busy (wikipedia) `.results.network` key sets are
identical, but the busy/truncated path carries an extra `hint` key:

```
left:  ["by_cause_type", "entries", "shown", "slowest", "timeout_reached", "total", "total_requests", "total_transfer_bytes", "truncated"]
right: [..., "hint", ...]
```

Root cause: iter-126's `build_canonical_network`
(`crates/ff-rdp-cli/src/commands/network.rs:770-797`) inserts `hint` only when
`truncated`, and its doc comment (`network.rs:762`) even documents the key as
`// only when truncated or timeout_reached` — directly contradicting the
iteration's own key-set-equality AC test. Two more producers make the key
conditional the same way: the summary timeout hint preserved by
`merge_summary_fields` (`network.rs:629`, `710`) and `run_network`'s
empty-capture hint (`network.rs:401-404`, `444`). The iter-126 unit tests
pinned the bug by asserting `hint` is *absent* on non-truncated paths
(`network.rs:1114`, `1174`, `1199`). The escape survived the PR because the
full live suite runs once per batch, not per iteration — exactly the sweep's
job to catch.

## Themes

- **A — `hint` becomes an always-present nullable member of the canonical
  object.** `null` when there is nothing to hint, a string otherwise. All
  producer paths (truncation hint, timeout hint, empty-capture hint, standalone
  `network` detail envelope) write into the same always-present key, so the
  canonical key set is genuinely fixed on every path.

## Tasks

- [ ] `build_canonical_network`: seed `hint: null` unconditionally in the base
      object; keep the truncation/timeout overwrites; fix the doc comment at
      `network.rs:762`.
- [ ] `run_network` detail envelope + `merge_summary_fields` (standalone
      `network` command): same always-present nullable `hint`.
- [ ] Flip the unit tests asserting absence (`network.rs:1114`, `1174`,
      `1199`) to assert `hint == null`; add
      `unit_canonical_network_hint_null_when_quiet` covering the quiet path on
      both the navigate and standalone builders.
- [ ] Help text (`args.rs` network/`--with-network` shape docs): document
      `hint` as always present, `null` when nothing to report.

## Acceptance Criteria [0/3]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [ ] live_navigate_with_network_shape_quiet_and_busy: passes UNMODIFIED —
      quiet and busy key sets identical (this is the iter-126 AC test that
      flagged the escape; the fix must satisfy it without touching it).
- [ ] unit_canonical_network_hint_null_when_quiet: `hint` is JSON `null` when
      `!truncated && !timeout_reached` and the capture is non-empty.
- [ ] unit_canonical_network_truncation_hint (existing assertion at
      `network.rs:1183`): truncated path still yields the
      "showing N of M, use --all for complete list" string.

## Notes

Filed before any fix lands, per carry-over discipline. Sibling sweep findings
(flaky `live_109_throttle` timing, load-sensitive `live_61r_eval`,
environment-dependent `live_96_profile_cleanup`, legacy port-6000 core tests)
are inventoried in the 2026-07-19 sweep report, not in this plan — this plan
covers only the deterministic iter-126 escape.
