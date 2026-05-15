---
title: "Iteration 62: Page-map index (crawl, extract, emit YAML)"
type: iteration
date: 2026-05-15
status: planned
branch: iter-62/page-map-index
depends_on: [iteration-61-script-runner-recorder]
tags:
  - iteration
  - page-map
  - crawler
  - agent-context
  - aria-tree
  - forms
  - json
---

# Iteration 62: Page-map index

The "show the agent the map before it starts walking" idea. The first
4–8 turns of any agent session today are spent discovering "what's on
this page" — links, forms, field names. If we can pre-compute that into
a static file the agent reads once, those turns disappear.

**Format: JSON, not YAML.** Page-maps are consumed by LLMs and
occasionally re-emitted by them (e.g. when patching a drift). JSON's
determinism and schema-validation story matter more than YAML's token
savings; the same reasoning behind iter-61's script-format decision
applies. YAML is accepted as input for free (parser is a superset) but
the canonical artefact is JSON.

No existing standard combines route inventory + page semantics + form
schemas in one format (verified in the research recap that produced
this batch of iterations). We define our own, but lean on prior art so
LLMs can pattern-match the shape:

- **Per-page section**: Playwright's `aria-snapshot` style, rendered as
  structured JSON instead of YAML (same tree, different syntax).
- **Site index layer**: sitemap-flavoured, with `llms.txt`-style
  annotations.
- **Form-schema layer**: OpenAPI's `parameters` shape, simplified.

The crawler is intentionally pragmatic, not exhaustive. It produces a
useful map for a logged-in user; it does not try to be a security
scanner or a full link checker.

Themes:

- **A — Format.** JSON schema for page-map files, documented and
  versioned (YAML accepted as input).
- **B — Crawler.** New `ff-rdp index` subcommand that crawls from a base
  URL and emits the JSON.
- **C — Auth-aware crawling.** Support pages behind login by reusing
  cookies from the running daemon's session, plus explicit cookie-file
  and bearer-token inputs.
- **D — Integration with the runner.** Scripts (iter-61) can reference
  page-map entries by name: `click: { page_map: dashboard.user_menu.signout }`.

## Tasks

### A. Format

#### A1. Schema
- [ ] Top-level keys: `$schema:
  "https://ff-rdp.dev/schemas/page-map/v1.json"` (required, literal
  discriminator — not fetched), `version: 1`, `generated_at`,
  `base_url`, `pages`, `api_routes` (optional, each entry:
  `{name, method, path, request?, response?}` — `name` is the
  dotted-path key scripts use as `api_route: <name>`),
  `flows` (optional, named reusable iter-61 script bodies).
- [ ] Each page entry: `path`, `title`, `auth_required`, `landmarks`
  (named regions of the page with their key interactive elements),
  `forms` (per-form: `id`, `selector`, `fields[]`, `submit`), `links` (a
  curated list of important outgoing routes, not every `<a>` on the
  page).
- [ ] Each field entry: `name`, `selector`, `type`, `required`,
  `placeholder?`, `validation?`. `type` maps to HTML input types plus
  inferred semantic types (email, password, date). The dotted path
  `pages.<page>.forms.<form>.fields.<name>` is the canonical address
  used by iter-61 scripts via `field: …`.

#### A1b. `flows` reuses the iter-61 script body
- [ ] Each `flows.<name>` value is an iter-61 script object **minus**
  the top-level metadata (`$schema`, `version`, `name`, `base_url`,
  `page_map` are inherited from the containing page-map). Required
  fields: `steps`. Optional: `vars`, `metadata`.
- [ ] Validation: when the page-map is loaded, every `flows.<name>` is
  re-validated against the iter-61 script schema as if it were a
  standalone script with the inherited metadata filled in. Errors
  point at `flows.<name>.steps[i]` paths, not at an opaque "flow
  invalid" message.
- [ ] A script can call a flow via the existing `run: <ref>` verb with
  `flow: <name>` form, which resolves through the loaded page-map's
  `flows[]` map.
- [ ] Ship a real JSON Schema (`draft-2020-12`) at
  `crates/ff-rdp-cli/schemas/page-map.schema.json`. Crawler emits files
  that validate; runner (iter-61) and `--check` mode reject malformed
  maps with a useful diagnostic.
- [ ] Document the schema in `kb/reference/page-map-format.md` with a
  worked example for `admin.wardrobe-assistants.ch` (the site used
  across sessions 42 and 44).

#### A2. Versioning + schema migration policy
- [ ] `version: 1` is mandatory. `ff-rdp` rejects unknown major versions
  with a clear "regenerate the map" error. Minor-version-bump fields
  are tolerated (forward compatibility within a major).

#### A3. Accept YAML input
- [ ] `ff-rdp index --out page-map.json` is the default. Loaders for
  `page_map:` references in scripts (iter-61) also accept `.yaml`/`.yml`
  files — same `serde::Deserialize` impl, same in-memory shape.

### B. Crawler

#### B1. `ff-rdp index` subcommand
- [ ] Args: optional base URL (defaults to the current tab's origin),
  `--out <path>` (defaults to `./.ffrdp/page-map.json`), `--depth <n>`
  (default 2), `--max-pages <n>` (default 50), `--include <regex>`,
  `--exclude <regex>`, `--format json|yaml` (default `json`).
- [ ] BFS from the base URL. Same-origin only by default;
  `--cross-origin` opt-in. Respect `robots.txt` (skip disallowed
  paths), with `--ignore-robots` for closed/internal admin tools.
- [ ] Per-page: navigate, wait for settle (iter-59), capture the ARIA
  tree (iter-60), extract forms (every `<form>` plus form-shaped
  React components — heuristic: a `<div>` containing ≥2 inputs and one
  submit-y button), follow links queued for the next depth level.

#### B2. Form extraction details
- [ ] For each `<form>`: `action` → `submit.posts_to`; observed `method`
  attribute or fallback to "POST"; each input gets `name`, `type`,
  `required` (from `required` attr or `aria-required`), `placeholder`,
  `value` if pre-filled.
- [ ] For form-shaped React components without a wrapping `<form>`:
  collect inputs by visual grouping (shared parent ancestor within N
  levels), submit button by `type="submit"` or the only obvious CTA in
  the group.

#### B3. Landmarks
- [ ] Emit named landmarks for ARIA-tagged regions: `navigation`,
  `main`, `complementary`, header `banner`, `contentinfo` (footer),
  `search`. Each landmark lists its top-N interactive elements with
  refs and labels.

#### B4. Streaming progress
- [ ] Long crawls emit progress to stderr: `[crawler] visited 12/50
  pages, queue=8`. Final YAML written atomically.

### C. Auth-aware crawling

#### C1. Reuse current session cookies
- [ ] Default: if a daemon is connected to Firefox and the user has
  already logged in, reuse the live session — the crawler navigates in
  the existing tab so cookies just work.
- [ ] Detect `--auth` mismatches: if a target URL responds with a login
  redirect, mark the page `auth_required: true` and skip it (unless
  the crawler has been authorised).

#### C2. Explicit credentials
- [ ] `--cookies-from <path>`: load Netscape-format cookie jar before
  crawling (useful when running headless from CI).
- [ ] `--bearer <token>`: inject `Authorization` header on each navigate
  via the network actor's request-intercept hook (if available on this
  Firefox version; else error out clearly).
- [ ] `--login-script <path>`: run an iter-61 script first that
  performs login, then crawl. Accepts either JSON or YAML script files.
  This is the most-likely-used path for SPA admins.

### D. Runner integration

#### D1. `page_map:` reference in scripts
- [ ] In iter-61 script verbs that take a selector, accept
  `page_map: <dotted.path>` as an alternative. The runner loads
  `./.ffrdp/page-map.json` (or a path from `--page-map`) and resolves
  the dotted path to a selector.
- [ ] Validation: at parse time (or at `--dry-run` time), each
  `page_map:` reference resolves to something in the map, else error.

#### D2. `ff-rdp index --check`
- [ ] Re-crawl in "verify" mode against an existing page-map. Report
  which selectors/refs/forms have drifted. Useful in CI to catch when
  the UI has changed under existing scripts.

## Acceptance Criteria

- [ ] `ff-rdp index https://example-app.local --out map.json` produces
  a JSON file covering all reachable pages within 2 hops from the base,
  with the documented schema, in under 30 s for a 20-page fixture site.
  The file validates against the shipped JSON Schema.
- [ ] The emitted map for the admin Wardrobe Assistants login page
  (logged-out crawl) contains the email + password fields with correct
  `name`, `type`, `selector`, and a `submit.posts_to:
  /api/auth/sign-in/email` field — i.e. the same things session 44 had
  to discover at runtime.
- [ ] A logged-in crawl (using `--login-script` from iter-61) produces
  entries for `/`, `/users`, `/bookings`, `/services`, with form
  metadata where forms exist and an empty `forms: []` where they
  don't.
- [ ] A script written with `page_map:`, `field:`, and `api_route:`
  references runs against the same site, and re-runs cleanly after a
  `ff-rdp index --check` shows no drift. All three reference forms
  resolve through the loaded page-map.
- [ ] A page-map with a `flows.login` body that is an iter-61 script
  object (minus inherited metadata) loads, validates against the
  iter-61 script schema, and is callable from another script via
  `run: { flow: login }`.
- [ ] `ff-rdp index --check` against a deliberately stale map flags the
  drifted selectors and exits non-zero.
- [ ] The format spec is precise enough that a hand-written page-map
  (in either JSON or YAML) validates against the shipped JSON Schema —
  i.e. it's a real format, not a documentation suggestion.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings &&
  cargo test --workspace -q` clean.

## Design Notes

- The crawler is **pragmatic, not principled**. It does not try to
  emulate browser behaviour exhaustively (no shadow-DOM traversal in
  v1, no service-worker registration awareness, no
  hover-to-reveal menus unless a Radix-style trigger is already known
  from a prior `dom`). The point is "useful map", not "complete graph."
- Reusing the live Firefox session for crawling is what makes the
  auth story tractable. Re-implementing storage state import/export
  the way Playwright does is out of scope; the daemon already has the
  cookies.
- Refs from the page-map are *not* the same as iter-60 runtime refs.
  Page-map refs are stable selectors (e.g. CSS or generated via
  Playwright-style locator inference); runtime refs only live for the
  duration of a session. The script runner needs to translate
  `page_map: x.y.z` → CSS selector and feed it to the underlying
  command.
- Alignment with iter-61 is by design: same `$schema`-style
  discriminator, same JSON-default / YAML-input policy, same
  validation philosophy (reject unknown keys), and `flows` reuses the
  exact iter-61 script body shape. The four targeting forms
  (`selector` / `ref` / `page_map` / `field`) and the network-target
  forms (`url_contains` / `api_route`) are mutually exclusive *per
  step* — enforced by both schemas.
- Where this iteration explicitly stops: no diff-view UI for
  `--check`, no auto-PR generation for drift, no fuzz-testing of
  forms. All on the roadmap for follow-ups.

## References

- Playwright aria-snapshots:
  <https://playwright.dev/docs/aria-snapshots>
- llms.txt proposal:
  <https://llmstxt.org/>
- OpenAPI `parameters` shape:
  <https://swagger.io/docs/specification/describing-parameters/>
- [[dogfooding/dogfooding-session-44]] — the discovery turns that
  motivated this iteration (every "what's on this page" call).
- [[iteration-59-autowait-pointer-retry]] — settle conditions used
  during crawl.
- [[iteration-60-compact-responses-refs]] — ARIA-tree output used by
  the crawler to populate landmarks.
- [[iteration-61-script-runner-recorder]] — script integration; the
  recorder may emit `page_map:` references when the source map is
  present.
