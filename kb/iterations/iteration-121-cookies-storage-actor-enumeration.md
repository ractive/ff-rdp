---
title: "Iteration 121: cookies StorageActor enumeration is dead on FF152 — httpOnly cookies + flags lost"
type: iteration
date: 2026-07-18
status: complete
branch: iter-121/cookies-storage-actor-enumeration
depends_on: []
firefox_refs: []
kb_refs:
  - kb/rdp/actors/storage.md
first_call_sites: []
dogfood_path: |
  # On a page that sets an httpOnly cookie, the StorageActor path MUST surface it
  # with real flags — not silently fall back to document.cookie (which can never
  # see httpOnly and always nulls secure/sameSite):
  ff-rdp --port <p> navigate 'https://httpbin.org/cookies/set?sess=abc'
  ff-rdp --port <p> cookies --jq '.results[] | select(.name=="sess")'
  # expected: {name:"sess", isHttpOnly:.., isSecure:.., sameSite:.., host:..} from
  #           the StorageActor (getStoreObjects), NOT source:"document.cookie"
  ff-rdp --port <p> cookies --storage-only --jq '.results | length'
  # expected: >= 1 (StorageActor enumeration is non-empty on FF152)
tags:
  - iteration
  - cookies
  - storage
  - rdp
  - firefox-152
  - dogfood-61
---

# Iteration 121: cookies StorageActor enumeration is dead on FF152

Discovered in [[dogfooding-session-61]] (ff-rdp v0.3.0 / Firefox 152), **CONFIRMED on a
clean single Firefox instance** (ruling out the multi-instance daemon-registry artifact that
first masked it). `StorageActor::list_cookies`
(`crates/ff-rdp-core/src/actors/storage.rs:79-138`) returns an **empty** vector on FF152, so
`commands::cookies::run` (`crates/ff-rdp-cli/src/commands/cookies.rs:24`) silently falls back
to `document.cookie` for every entry. Consequences:

- **httpOnly cookies are missed entirely** — `document.cookie` cannot see them. On
  httpbin.org (4 cookies, 2 httpOnly) `ff-rdp cookies` returns `[]`.
- **`isSecure` / `isHttpOnly` / `sameSite` / `domain` are always null/absent** — the
  `document.cookie` fallback only knows name+value. On comparis every entry is
  `source:"document.cookie"` with no flag fields.
- **`cookies --storage-only` returns 0** — proving the enumeration path, not just the merge,
  is broken.
- **Silent failure**: exit 0, and `--help` still claims cookies "includes httpOnly, secure,
  sameSite" — never true. This guts the command's entire reason to exist over a bare
  `eval 'document.cookie'`, and breaks the security-audit use case.

The default-merge behaviour itself is fine (iter-83 Theme D added the `document.cookie` merge
as an *enrichment*); the regression is that the authoritative StorageActor path went silent.
`list_cookies` already carries FF149+ compat shims (`storage.rs:56-78`); FF152 appears to have
shifted the `cookies` resource / `getStoreObjects` contract again.

## Themes

- **A — Repair the StorageActor cookie enumeration on FF152.** Find why
  `watch_resources(&["cookies"])` → `resources-available-array` → `getStoreObjects` yields no
  items on FF152 and fix the request/parse contract so real cookies (with flags) come back.
- **B — Never silently degrade.** When the StorageActor path yields empty but `document.cookie`
  has entries, surface a top-level `source`/`degraded` marker + warning instead of returning a
  weaker result under exit 0.

## Tasks

### A. StorageActor enumeration on FF152

- [x] Reproduce live: capture the raw RDP traffic of `list_cookies` against FF152 (a fixture
      page that sets one normal + one httpOnly cookie). Determine which step returns empty —
      the `resources-available-array` host list, or `getStoreObjects` items.
- [x] Diff the FF152 `cookies` resource / `getStoreObjects` reply shape against the FF149-era
      shape the current parser (`parse_cookie`, `storage.rs:235-287`) expects. Record findings
      in `kb/rdp/actors/storage.md`.
- [x] Fix the request params (`storage.rs:110-115`, `host`/`resourceId`/`options`) and/or
      `parse_cookie` field extraction so FF152 cookies with real `isHttpOnly`/`isSecure`/
      `sameSite` are returned. If FF152 requires a field ff-rdp does not declare in the spec,
      annotate with `// allow-spec-drift: bug TBD (<rationale>)`.
- [x] Record a real fixture via `live_record_fixtures.rs` (never hand-crafted) for the FF152
      cookies reply.

### B. No silent degradation

- [x] In `commands::cookies::run`, when the StorageActor returns empty AND the `document.cookie`
      fallback returns entries, attach a `warnings[]` entry (`type: "storage_actor_empty"`) and
      set each fallback entry's `source: "document.cookie"` (already present) — so consumers can
      tell flags are unavailable rather than false.

## Acceptance Criteria [3/3]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [x] live_cookies_httponly_enumerated: after a page sets an httpOnly cookie, `ff-rdp cookies`
      returns it with `isHttpOnly == true` and non-null `isSecure`/`sameSite`, sourced from the
      StorageActor (not `document.cookie`).
- [x] live_cookies_storage_only_nonempty: `ff-rdp cookies --storage-only` returns `>= 1` entry
      on a page that set a normal cookie (StorageActor enumeration non-empty on FF152).
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- Keep the iter-83 `document.cookie` merge as enrichment; this iteration only repairs the
  authoritative path and adds the degraded marker. Do not remove `--storage-only`.
- `CookieInfo` (`storage.rs:13-29`) already has all flag fields; no schema change expected on
  the output side — the fix is on the wire request/parse side.

## Out of scope

- `Set-Cookie` header merge (`live_cookies_set_cookie_header.rs`, iter-85) — separate path,
  stays `#[ignore]` unless the FF152 fix incidentally un-reds it.
- localStorage/sessionStorage enumeration (`storage` command) — only cookies here.

## References

- [[dogfooding-session-61]]
- [[storage]]
