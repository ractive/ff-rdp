---
name: project-serde-json-ordering
description: serde_json preserve_order feature and the text-table column-order contract in ff-rdp-cli
metadata:
  type: project
---

Root Cargo.toml's `serde_json` dependency now enables `preserve_order`
(fixed in fix/doctor-text-column-order, ~2026-07-07). Before this fix,
`serde_json::Map` was backed by a `BTreeMap`, so keys always serialized
alphabetically — silently violating the documented "first-seen insertion
order" contract in `crates/ff-rdp-cli/src/output_pipeline.rs`'s
`collect_table_columns`/`render_table` (used by `--format text`).

**Why:** `doctor --format text` was rendering columns as
detail, glyph, name, status, hint (alphabetical) instead of the intended
glyph, name, status, hint, detail — the wide free-text `detail` column
came first and pushed everything else off-screen. Root cause was the
BTreeMap-vs-insertion-order mismatch, not a bug in the renderer itself.

**How to apply:**
- Any command building a JSON object for text-table rendering should
  insert narrow/glance-able columns first (e.g. glyph, name, status) and
  wide free-text columns (detail, message, body) last — insertion order
  now genuinely controls display order project-wide, for every command's
  JSON envelope, not just `doctor`.
- `collect_table_columns()` in `output_pipeline.rs` is a pure function
  extracted specifically so column order is unit-testable without
  capturing stdout — reuse this pattern for future renderer tests.
- `preserve_order` changes JSON key order in ALL command envelopes
  (insertion order instead of alphabetical). No existing test asserted
  exact alphabetical key order at the time of this fix, so the blast
  radius was zero test-expectation updates — but be alert for this in
  future PRs that touch `serde_json`.

See also [[project_xtask_discipline_gates]] for how `check-dead-primitives`
/`check-todo-annotations` diff `origin/main...HEAD` (committed history),
not the working tree — uncommitted edits won't show up until committed.
