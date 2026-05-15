---
title: "Iteration 60: Compact responses and stable element refs"
type: iteration
date: 2026-05-15
status: planned
branch: iter-60/compact-responses-refs
depends_on: [iteration-59-autowait-pointer-retry]
tags:
  - iteration
  - output-format
  - token-efficiency
  - agent-speed
  - aria-tree
  - ref-ids
  - breaking-change
---

# Iteration 60: Compact responses and stable element refs

Every ff-rdp response today carries a `meta.connection` block repeating
`connected_pid`, `host`, and `port` — a per-call boilerplate that an LLM
agent re-reads on every single tool result. Worse, `dom` returns raw HTML
strings: 30 KB for a 400-element page is normal, most of which is
unparseable class soup. Both problems compound: agents pay tokens to read,
to think, and to emit responses, scaled by payload size.

This iteration shifts the default output to a **terse, agent-first shape**
inspired by Playwright's `_snapshotForAI()` / `aria-snapshot` format,
introduces **stable ref IDs** that survive DOM mutation across calls, and
makes ref-based targeting a first-class alternative to CSS selectors.

Because ff-rdp is still pre-1.0 (v0.1.0 release in progress), this is the
right window to make breaking output-shape changes — but the iteration
ships an opt-in legacy path for any consumers that need it.

Themes:

- **A — Default-trim the response envelope.** Drop `meta.connection` unless
  `--verbose`. Keep `meta` for fields that *change* per request (timing,
  warnings, settle method).
- **B — ARIA-tree output for `dom` and `snapshot`.** New default shape:
  one entry per node with `role`, `name`, `level`, and a stable `ref`. Raw
  HTML is available behind `--format html`.
- **C — Ref IDs as first-class selectors.** `--ref e23` is accepted anywhere
  a CSS selector is. Refs are minted by ff-rdp, persisted in the daemon
  per-tab, and remain valid across requests until the page navigates.
- **D — Output mode discipline.** Document the three formats (`json`,
  `text`, `html`/`legacy`) and make `--format json` the unambiguous
  default for machine consumption.

## Tasks

### A. Trim the response envelope

#### A1. Drop `meta.connection` from default output
- [ ] In `crates/ff-rdp-cli/src/output.rs` (or equivalent), only include
  `meta.connection` when `--verbose` is set. Default response shape becomes
  `{results, total, meta?}` with `meta` omitted when empty.
- [ ] Keep per-request `meta` fields that *vary*: `meta.elapsed_ms`,
  `meta.tab`, `meta.warnings`, `meta.settle_method`.

#### A2. Snapshot-test the new default shape
- [ ] Add fixture-based snapshot tests for `tabs`, `click`, `dom`,
  `console`, `network` confirming `meta.connection` is absent without
  `--verbose` and present with it.

#### A3. `--verbose` global flag
- [ ] Promote `--verbose` to a top-level flag (currently per-subcommand or
  absent). One flag, every command, restores the pre-iter-60 envelope.

### B. ARIA-tree output for `dom` and `snapshot`

#### B1. New default shape for `dom`
- [ ] Each result entry becomes:
  ```json
  {"ref":"e23","role":"button","name":"Sign out","level":3,
   "state":{"expanded":false,"disabled":false},
   "tag":"button","attrs":{"id":"radix-_R_x_","aria-haspopup":"menu"}}
  ```
- [ ] `attrs` only includes the small set ff-rdp considers actionable
  (`id`, `name`, `type`, `href`, `aria-*`, `data-state`, `role`,
  `placeholder`, `value` for inputs). Everything else lives behind
  `--format html`.

#### B2. New `snapshot` shape: ARIA tree
- [ ] Replace the current `{tag, attrs, children}` recursive DOM dump with
  a YAML-ish tree pruned to the accessibility tree:
  ```yaml
  - main
    - heading "Welcome back, James Admin Test 1" [level=1] [ref=e3]
    - link "Users" [ref=e7]
    - table
      - row "James Admin Test 1 …" [ref=e12]
  ```
- [ ] JSON form is the structured equivalent (same data, machine-readable).
  Text form is a tree printed line-by-line with two-space indent.
- [ ] Document the format in `kb/reference/page-snapshot-format.md`.

#### B3. `--format html` keeps the old behaviour
- [ ] Power users who actually want raw HTML strings (e.g. for HTML
  diffing) opt in. Document in `--help` for both commands.

### C. Ref IDs as first-class selectors

#### C1. Ref allocation
- [ ] In the daemon, maintain a monotonic counter per-tab. Each node
  returned by `dom`/`snapshot` is assigned `e<N>`. Mapping `ref → DOM
  path` (a JS expression like `document.querySelector(...)` or a
  generated unique selector) is stored in the daemon's per-tab session.
- [ ] On `pageshow`/navigation, invalidate the map. Subsequent `--ref`
  calls return a clear "ref expired (page navigated)" error.

#### C2. `--ref <id>` accepted everywhere CSS selectors are
- [ ] Audit: `click`, `type`, `scroll`, `wait`, `dom`, `geometry`,
  `styles`, `computed`, `a11y`, `responsive`.
- [ ] When `--ref` is provided, skip CSS parsing and resolve via the
  daemon's map directly. Mutual-exclusion with positional/`--selector`.

#### C3. No-daemon mode
- [ ] When running with `--no-daemon`, ref allocation is per-process and
  refs from one CLI invocation can't be used in the next. Document the
  limitation; the agent path always uses the daemon anyway.

### D. Output-format discipline

#### D1. Document the three formats end-to-end
- [ ] Add `kb/reference/output-formats.md` covering `json` (default,
  machine-readable, the contract), `text` (human-readable, lossy), and
  `html` (where applicable, raw passthrough). Note that `--jq` operates
  on the JSON form regardless of `--format`.

#### D2. Allow `--jq` together with `--format text`
- [ ] Today these error out as mutually exclusive (see session 44 issue
  #7). New behaviour: jq runs first on the JSON, the result is then
  rendered as text if `--format text` is set. Matches the "filter, then
  make terse" intuition.

## Acceptance Criteria

- [ ] Default `ff-rdp click '…'` response is ≤200 bytes for a successful
  click (measured against the existing happy-path test fixture). Current
  baseline is ~400 bytes.
- [ ] `ff-rdp snapshot` on the admin dashboard fixture produces output
  ≤2 KB in ARIA-tree form (compared to ~30 KB current DOM dump).
- [ ] An agent flow can: `snapshot` → pick a ref → `click --ref e23` → no
  intermediate "find the right selector" calls.
- [ ] `--verbose` restores the pre-iter-60 envelope shape exactly, byte
  for byte for the fields it carries (regression-tested).
- [ ] `dom --format html` reproduces the current shape (escape hatch
  works).
- [ ] `--jq '…' --format text` combination works and is documented.
- [ ] All e2e tests updated to expect the new default shape; the diff is
  predominantly "remove `meta.connection`" plus a smaller set of `dom`
  shape updates.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings &&
  cargo test --workspace -q` clean.

## Design Notes

- The Playwright `aria-snapshot` format
  (<https://playwright.dev/docs/aria-snapshots>) is the de-facto target —
  not because we want compatibility (their refs aren't portable), but
  because LLMs trained on Playwright examples already understand the
  shape. Cheap legibility win.
- Ref invalidation on navigation is the only correctness-critical part.
  Refs that silently resolve to stale nodes after SPA navigation would be
  a hard-to-diagnose footgun.
- For pages with thousands of nodes, the ARIA tree can still be large.
  Add `--max-depth` and `--filter-role` as follow-ups if needed — out of
  scope for this iteration.
- The breaking change is real. Document migration in the v0.1.0 release
  notes. Pre-1.0 latitude is the reason this is OK to do now and not in a
  year.

## References

- Playwright snapshots for AI:
  <https://playwright.dev/docs/aria-snapshots>
- [[dogfooding/dogfooding-session-44]] — issue #5 (cookies schema drift),
  issue #7 (`--jq` + `--format text` exclusivity), and the broader
  "30 KB of HTML strings" observation.
- [[iteration-59-autowait-pointer-retry]] — must land first so the
  examples used to validate the compact shape come from a stable
  interaction layer.
