---
branch: iter-128/network-output-fidelity
date: 2026-07-19
depends_on:
  - kb/iterations/iteration-126-network-json-shape-consistency.md
dogfood_path: |
  ff-rdp launch --headless --auto-consent
  ff-rdp navigate https://example.com --with-network --jq '.results.network | has("hint")'
  # → true on a quiet page (value null), same key set as a busy page
  ff-rdp navigate https://www.theguardian.com --with-network >/dev/null
  ff-rdp network --detail --jq '[.results.entries[].source] | unique'
  # → ["watcher"] (not performance-api) while the daemon buffer has events
  ff-rdp network --format text | awk '{ if (length($0) > 120) exit 1 }'
  # → table lines stay terminal-readable even with 900-char CMP URLs
first_call_sites: []
status: planned
---

# Iteration 128: network output fidelity — hint key, watcher parity, text readability

Scope grew from the original hint-only plan per James's 2026-07-19 direction: per-PR
overhead (create-pr / review-pr / gates) is high, so the deterministic sweep escape is
bundled with the two network-fidelity findings from [[dogfooding-session-62]] into one
network-focused iteration.

## Findings driving this iteration

1. **Sweep escape (deterministic):** `live_126_network_shape::live_navigate_with_network_shape_quiet_and_busy`
   fails on main — `build_canonical_network` (`crates/ff-rdp-cli/src/commands/network.rs:770-797`)
   inserts `hint` only when `truncated`; doc comment at `network.rs:762` even documents
   `// only when truncated or timeout_reached`, contradicting iter-126's key-set-equality
   AC test. Same conditionality: `merge_summary_fields`' timeout hint (`network.rs:629`,
   `710`) and `run_network`'s empty-capture hint (`network.rs:401-404`, `444`). The
   iter-126 unit tests pinned the bug by asserting absence (`network.rs:1114`, `1174`,
   `1199`). Dogfood-62 confirmed the key-set diff is exactly `["hint"]`, nothing else.
2. **`network` fidelity depends on output mode** (dogfood-62 #4, MODERATE): text/summary
   path renders 18 rows `source:watcher` with methods populated, while `--detail`/`--jq`
   returns 93 rows `source:performance-api` with method/status/content_type null — even
   though the daemon held 840+ buffered watcher events. Agents piping `--jq` silently get
   the worst-fidelity source. `content_type` is null even on watcher rows despite the
   `--help` promise.
3. **`--format text` tables unreadable with long URLs** (dogfood-62 #2, MODERATE):
   ~900-char Sourcepoint URLs expand the url column to thousands of columns. `sources
   --format text` has the same problem.
4. **Routed commands don't self-identify** (dogfood-62 #10, INFO): `meta` is empty on
   daemon-routed commands; routing is only observable via `daemon status` + registry file.

## Themes

- **A — `hint` is an always-present nullable member of the canonical object.** `null`
  when there is nothing to hint; all producers (truncation, timeout, empty-capture,
  standalone `network` detail envelope) write the same key.
- **B — one capture source, every output mode.** `--detail`/`--jq` consume the same
  watcher-buffer entries as the text/summary path; fall back to performance-api only
  when the watcher genuinely has nothing, and say so via the `source` field. Populate
  `content_type` on watcher rows or fix `--help` to stop promising it.
- **C — text tables stay terminal-readable.** Middle-ellipsis URLs to a bounded column
  width (~80 chars) in `network --format text` and `sources --format text`.
- **D — routed commands self-identify.** `meta.route: "daemon" | "direct"` on every
  envelope, so an agent can tell how a command executed without consulting
  `daemon status`.

## Tasks

- [ ] A: seed `hint: null` unconditionally in `build_canonical_network`; keep the
      truncation/timeout overwrites; fix the doc comment; same treatment for the
      standalone `network` detail envelope + `merge_summary_fields`.
- [ ] A: flip the absence-asserting unit tests (`network.rs:1114`, `1174`, `1199`) to
      assert `hint == null`; add `unit_canonical_network_hint_null_when_quiet` for both
      builders.
- [ ] B: route the `--detail`/`--jq` path through the watcher buffer (daemon drain or
      direct watcher capture) before considering the performance-api fallback; keep the
      fallback for genuinely-empty buffers and label it via `source`.
- [ ] B: populate `content_type` on watcher rows (response headers are already
      captured for `network --security`); align `--help` with reality.
- [ ] C: middle-ellipsis helper for table cells; apply to url columns in `network` and
      `sources` text renderers; unit-test the ellipsis edge cases (short URLs untouched,
      multibyte safety).
- [ ] D: emit `meta.route` from the CLI dispatch layer on all commands.

## Acceptance Criteria [0/6]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [ ] live_navigate_with_network_shape_quiet_and_busy: passes UNMODIFIED — quiet and
      busy key sets identical (the iter-126 AC test that flagged the escape).
- [ ] unit_canonical_network_hint_null_when_quiet: `hint` is JSON `null` when
      `!truncated && !timeout_reached` and the capture is non-empty, on both the
      navigate and standalone builders.
- [ ] live_128_network_detail_uses_watcher: after a real navigate with the daemon
      buffering events, `network --detail --jq '[.results.entries[].source] | unique'`
      == `["watcher"]`, and ≥1 entry has non-null `method` and `content_type`.
- [ ] live_128_network_text_width: `network --format text` on a page with a >200-char
      URL emits no line wider than 120 columns; same assertion for
      `sources --format text`.
- [ ] unit_middle_ellipsis: helper preserves scheme+host prefix and path tail around
      the ellipsis; no-op below the width cap.
- [ ] live_128_meta_route: a daemon-routed command reports `meta.route == "daemon"`;
      the same command with `--no-daemon` reports `"direct"`.

## Notes

Sibling plans from the same findings batch: [[iteration-129-consent-and-cross-origin-frames]],
[[iteration-130-navigation-truthfulness]], [[iteration-131-measurement-honesty]],
[[iteration-132-cli-polish]].
