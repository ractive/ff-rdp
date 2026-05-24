---
title: "Iteration 62: Page-map index (crawl, extract, emit JSON)"
type: iteration
date: 2026-05-24
status: planned
branch: iter-62/page-map-index
depends_on:
  - iteration-61-script-runner-recorder
  - iteration-61w-security-hardening-and-cleanup
  - iteration-61z-discipline-skill-integration
# Required by check-iteration-plan when the body introduces new pub items.
# Filled in once the resolver/crawler module boundaries are decided during
# implementation — initial set targets the runner stubs that already exist
# in crates/ff-rdp-cli/src/script/runner.rs.
first_call_sites:
  - primitive: "PageMap"
    site: "crates/ff-rdp-cli/src/script/runner.rs::resolve_target (replaces the iter-62-not-yet-implemented branch at runner.rs:573-585)"
  - primitive: "ff_rdp_cli::commands::index::run"
    site: "crates/ff-rdp-cli/src/main.rs (Cli::Index dispatch)"
dogfood_path: |
  # 1. Crawl the admin Wardrobe Assistants login page (logged out).
  ff-rdp index https://admin.wardrobe-assistants.ch/sign-in \
      --out /tmp/wa.page-map.json --depth 1
  jq '.pages[] | {path, forms: [.forms[].id]}' /tmp/wa.page-map.json
  # Expected: a `sign-in` page with one form whose fields[] include `email`
  # and `password`, and `submit.posts_to == "/api/auth/sign-in/email"`.

  # 2. Run an iter-61 script that uses `page_map:` to drive the same form.
  ff-rdp run kb/scripts/login.script.json --page-map /tmp/wa.page-map.json
  # Expected: the runner resolves `page_map: pages.sign-in.forms.signin.submit`
  # to a CSS selector and clicks it — no "iter-62 not-yet-implemented" error.

  # 3. Verify drift detection.
  ff-rdp index --check --page-map /tmp/wa.page-map.json \
      https://admin.wardrobe-assistants.ch/sign-in
  # Expected: exit 0 (no drift). After re-renaming a selector in the live
  # site, exit non-zero with a diff report.
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

**Status of the dependency chain.** iter-61 (script runner/recorder)
landed and is on main. While iter-62 was deferred, iter-61 grew a long
stability sub-tree (61b..61z) that rebuilt the Actor Registry,
ResourceCommand bus, Front lifecycle, navigate/screenshot completion,
security boundaries, and the iteration-discipline tooling. This plan
predates that work; it has been refreshed on 2026-05-24 to reflect what
actually shipped:

- The script runner (`crates/ff-rdp-cli/src/script/runner.rs`) already
  parses `page_map:`, `field:`, and `api_route:` keys and errors out
  with `"page_map and field target selectors require iter-62 page-map
  support (not yet implemented)"` (runner.rs:573-585). iter-62 swaps
  those branches for an actual resolver — **D1 is half-built**.
- The script schema at `crates/ff-rdp-cli/schemas/script.schema.json`
  already documents the four targeting forms as mutually exclusive.
  iter-62 ships a sibling `page-map.schema.json` and links the two via
  the same `$schema`-style discriminator policy.
- iter-59 (autowait/settle) and iter-60 (ARIA tree + refs) are both on
  main; their helpers (`autowait_element`, `settle_page`, the ARIA-tree
  template in `commands/dom.rs`) are the building blocks the crawler
  composes per-page.
- iter-61w hardened the daemon's resource boundaries (RefStore caps,
  cookie scoping). The crawler reuses the live tab's cookies through
  those existing surfaces — no new credential plumbing is needed for
  the common case.

iter-61aa (claim-miss hard gate) is the only other planned iteration; it
is independent of iter-62 and may land in either order.

**Format: JSON, not YAML.** Page-maps are consumed by LLMs and
occasionally re-emitted by them (e.g. when patching a drift). JSON's
determinism and schema-validation story matter more than YAML's token
savings; the same reasoning behind iter-61's script-format decision
applies. YAML is accepted as input for free (parser is a superset) but
the canonical artefact is JSON.

No existing standard combines route inventory + page semantics + form
schemas in one format. We define our own, but lean on prior art so LLMs
can pattern-match the shape:

- **Per-page section**: Playwright's `aria-snapshot` style, rendered as
  structured JSON.
- **Site index layer**: sitemap-flavoured, with `llms.txt`-style
  annotations.
- **Form-schema layer**: OpenAPI's `parameters` shape, simplified.

The crawler is intentionally pragmatic, not exhaustive. It produces a
useful map for a logged-in user; it does not try to be a security
scanner or a full link checker.

## Themes

- **A — Format.** JSON schema for page-map files, documented and
  versioned (YAML accepted as input).
- **B — Crawler.** New `ff-rdp index` subcommand that crawls from a base
  URL and emits the JSON, composing iter-59 (settle) + iter-60 (ARIA
  tree).
- **C — Auth-aware crawling.** Reuse the live daemon tab's cookies; add
  explicit cookie-file and bearer-token inputs for headless CI; allow a
  `--login-script` (iter-61 script) to run before the crawl.
- **D — Runner integration.** Wire the existing iter-62-deferred
  branches in `script/runner.rs` to an actual page-map resolver. Add
  `ff-rdp index --check` for drift detection.

## Tasks

### A. Format

#### A1. Schema [0/3]
- [ ] Top-level keys: `$schema:
  "https://ff-rdp.dev/schemas/page-map/v1.json"` (required, literal
  discriminator — not fetched), `version: 1`, `generated_at`,
  `base_url`, `pages`, `api_routes` (optional, each entry:
  `{name, method, path, request?, response?}` — `name` is the
  dotted-path key scripts use as `api_route: <name>`),
  `flows` (optional, named reusable iter-61 script bodies).
- [ ] Each page entry: `path`, `title`, `auth_required`, `landmarks`
  (named regions of the page with their key interactive elements),
  `forms` (per-form: `id`, `selector`, `fields[]`, `submit`), `links`
  (a curated list of important outgoing routes, not every `<a>` on the
  page).
- [ ] Each field entry: `name`, `selector`, `type`, `required`,
  `placeholder?`, `validation?`. `type` maps to HTML input types plus
  inferred semantic types (email, password, date). The dotted path
  `pages.<page>.forms.<form>.fields.<name>` is the canonical address
  used by iter-61 scripts via `field: …`.

#### A1b. `flows` reuses the iter-61 script body [0/5]
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
  `crates/ff-rdp-cli/schemas/page-map.schema.json`, sibling to the
  existing `script.schema.json`. Crawler emits files that validate;
  runner (iter-61) and `--check` mode reject malformed maps with a
  useful diagnostic.
- [ ] Document the schema in `kb/reference/page-map-format.md` with a
  worked example for `admin.wardrobe-assistants.ch` (the site used in
  dogfooding sessions 42 and 44).

#### A2. Versioning + schema migration policy [0/1]
- [ ] `version: 1` is mandatory. `ff-rdp` rejects unknown major
  versions with a clear "regenerate the map" error. Minor-version-bump
  fields are tolerated (forward compatibility within a major).

#### A3. Accept YAML input [0/1]
- [ ] `ff-rdp index --out page-map.json` is the default. Loaders for
  `--page-map` references in scripts (iter-61) also accept `.yaml`/`.yml`
  files — same `serde::Deserialize` impl, same in-memory shape.

### B. Crawler

#### B1. `ff-rdp index` subcommand [0/3]
- [ ] Args: optional base URL (defaults to the current tab's origin
  via the daemon), `--out <path>` (defaults to
  `./.ffrdp/page-map.json`), `--depth <n>` (default 2),
  `--max-pages <n>` (default 50), `--include <regex>`,
  `--exclude <regex>`, `--format json|yaml` (default `json`).
- [ ] BFS from the base URL. Same-origin only by default;
  `--cross-origin` opt-in. Respect `robots.txt` (skip disallowed
  paths), with `--ignore-robots` for closed/internal admin tools.
- [ ] Per-page: navigate (reusing iter-61v's document-event gated
  navigate), `settle_page` (iter-59), capture the ARIA tree
  (iter-60's `aria_tree_js_template` from `commands/dom.rs`), extract
  forms (see B2), follow links queued for the next depth level.

#### B2. Form extraction details [0/2]
- [ ] For each `<form>`: `action` → `submit.posts_to`; observed `method`
  attribute or fallback to "POST"; each input gets `name`, `type`,
  `required` (from `required` attr or `aria-required`), `placeholder`,
  `value` if pre-filled.
- [ ] For form-shaped React components without a wrapping `<form>`:
  collect inputs by visual grouping (shared parent ancestor within N
  levels), submit button by `type="submit"` or the only obvious CTA in
  the group.

#### B3. Landmarks [0/1]
- [ ] Emit named landmarks for ARIA-tagged regions: `navigation`,
  `main`, `complementary`, header `banner`, `contentinfo` (footer),
  `search`. Each landmark lists its top-N interactive elements with
  refs and labels.

#### B4. Streaming progress [0/1]
- [ ] Long crawls emit progress to stderr: `[crawler] visited 12/50
  pages, queue=8`. Final JSON written atomically (write-temp +
  rename).

### C. Auth-aware crawling

#### C1. Reuse current session cookies [0/2]
- [ ] Default: if a daemon is connected to Firefox and the user has
  already logged in, reuse the live session — the crawler navigates in
  the existing tab so cookies just work (no separate cookie-jar
  plumbing; iter-61w bounded cookie scoping is already in place).
- [ ] Detect auth-redirect mismatches: if a target URL responds with a
  login redirect (detected via `RdpError::Navigation` + the navigate
  state machine landed in iter-61v), mark the page
  `auth_required: true` and skip it (unless the crawler has been
  authorised).

#### C2. Explicit credentials [0/3]
- [ ] `--cookies-from <path>`: load Netscape-format cookie jar before
  crawling (useful when running headless from CI).
- [ ] `--bearer <token>`: inject `Authorization` header on each
  navigate via the network actor's request-intercept hook (if
  available on this Firefox version; else error out clearly).
- [ ] `--login-script <path>`: run an iter-61 script first that
  performs login, then crawl. Accepts either JSON or YAML script
  files. This is the most-likely-used path for SPA admins.

### D. Runner integration

#### D1. Wire the iter-62-deferred branches [0/3]
- [ ] Replace the
  `"page_map and field target selectors require iter-62 page-map
  support (not yet implemented)"` branch in `runner.rs:573-585` with a
  real `PageMap::resolve_target` call that converts
  `page_map: <dotted.path>` and `field: <dotted.path>` to a concrete
  CSS selector.
- [ ] Replace the matching `assert_network` branch
  (runner.rs:946-948) with `PageMap::resolve_api_route` that maps
  `api_route: <name>` to a method+path pair the network assertion
  checks against.
- [ ] Loader: the runner accepts `--page-map <path>` (CLI flag) or
  falls back to `./.ffrdp/page-map.json` when present. Each unresolved
  `page_map:`/`field:`/`api_route:` reference produces a parse-time
  diagnostic pointing at the script line.

#### D2. `ff-rdp index --check` [0/2]
- [ ] Re-crawl in "verify" mode against an existing page-map. Report
  which selectors/refs/forms have drifted. Useful in CI to catch when
  the UI has changed under existing scripts.
- [ ] Exit non-zero on any drift; emit a structured JSON drift report
  (`--report <path>`) suitable for diffing in code review.

## Acceptance Criteria [0/8]

<!-- Each AC names a live_* test slug and the asserted post-condition,
per the iter-61y/61z discipline (CLAUDE.md "Iteration discipline"
section). The ac-fidelity-check.sh gate enforces this at merge time. -->

- [ ] `live_index_local_fixture`: `ff-rdp index <fixture-base-url>
  --out map.json --depth 2 --max-pages 50` on the 20-page fixture site
  in `crates/ff-rdp-cli/tests/fixtures/page-map-site/` completes in
  ≤30 s and the emitted `map.json` validates against
  `page-map.schema.json`.
- [ ] `live_index_login_form_extraction`: a logged-out crawl of the
  admin Wardrobe Assistants `/sign-in` page emits a `pages.sign-in`
  entry with `forms[0].fields[]` containing `email` (type `email`,
  `required: true`) and `password` (type `password`, `required: true`),
  and `forms[0].submit.posts_to == "/api/auth/sign-in/email"`.
- [ ] `live_index_logged_in_routes`: a `--login-script`-gated crawl
  produces entries for `/`, `/users`, `/bookings`, `/services` (the
  routes session 44 walked manually), each with `auth_required: false`
  on the post-login domain and form metadata or empty `forms: []` as
  appropriate.
- [ ] `live_runner_page_map_resolution`: an iter-61 script with one
  `click: { page_map: pages.sign-in.forms.signin.submit }`, one
  `type: { field: pages.sign-in.forms.signin.fields.email }`, and one
  `assert_network: { api_route: signIn }` runs against the same site
  without producing the
  `"page_map ... not yet implemented"` error. All three reference forms
  resolve through the loaded page-map.
- [ ] `live_flows_login_callable`: a page-map with a `flows.login`
  body that is an iter-61 script object (minus inherited metadata)
  loads, validates against the iter-61 script schema (re-validation
  diagnostic points at `flows.login.steps[i]` on failure), and is
  callable from another script via `run: { flow: login }`.
- [ ] `live_index_check_detects_drift`: `ff-rdp index --check` against
  a deliberately stale map (a selector mutated in the fixture site
  after the original crawl) flags the drifted selector in the JSON
  drift report and exits non-zero.
- [ ] `test_handwritten_page_map_validates`: a hand-written page-map
  (committed as a test fixture, in both JSON and YAML forms) validates
  against the shipped JSON Schema — i.e. it's a real format, not a
  documentation suggestion.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D
  warnings && cargo test --workspace -q` clean, plus
  `cargo run -p xtask -- check-iteration-plan
  kb/iterations/iteration-62-page-map-index.md` clean.

## Design notes

- The crawler is **pragmatic, not principled**. It does not try to
  emulate browser behaviour exhaustively (no shadow-DOM traversal in
  v1, no service-worker registration awareness, no hover-to-reveal
  menus unless a Radix-style trigger is already known from a prior
  `dom` capture). The point is "useful map", not "complete graph."
- Reusing the live Firefox session for crawling is what makes the
  auth story tractable. Re-implementing storage state import/export
  the way Playwright does is out of scope; the daemon already has the
  cookies and iter-61w bounded their lifetime safely.
- Refs from the page-map are *not* the same as iter-60 runtime refs.
  Page-map refs are stable selectors (CSS or Playwright-style locator
  inference); runtime refs only live for the duration of a session
  (bounded by iter-61w's RefStore). The script runner needs to
  translate `page_map: x.y.z` → CSS selector and feed it to the
  underlying command — D1 is where that translation lands.
- Alignment with iter-61 is by design: same `$schema`-style
  discriminator, same JSON-default / YAML-input policy, same
  validation philosophy (reject unknown keys), and `flows` reuses the
  exact iter-61 script body shape. The four targeting forms
  (`selector` / `ref` / `page_map` / `field`) and the network-target
  forms (`url_contains` / `api_route`) are mutually exclusive *per
  step* — already enforced by `script.schema.json`.
- Discipline alignment with iter-61y/61z: every AC names a `live_*` or
  `test_*` slug; `first_call_sites` is populated (the two
  iter-62-deferred branches in `runner.rs` are concrete first
  consumers); `dogfood_path` is reproducible from a single shell
  block. The `cargo xtask check-iteration-plan` lint and the
  `ac-fidelity-check.sh` merge gate both pass against this plan.
- Where this iteration explicitly stops: no diff-view UI for
  `--check`, no auto-PR generation for drift, no fuzz-testing of
  forms. All on the roadmap for follow-ups.

## Out of scope

- Shadow-DOM traversal.
- Service-worker / route-prefetching awareness.
- Hover-to-reveal menu discovery (no synthetic mouseenter probing).
- Full link checking (broken-link detection, redirect chains).
- Diff-view UI for `--check` reports — JSON output only.
- Cross-site / cross-origin crawls beyond the `--cross-origin` flag.

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
  during crawl (`settle_page`, `wait_for_predicates`).
- [[iteration-60-compact-responses-refs]] — ARIA-tree output used by
  the crawler to populate landmarks (`commands/dom.rs`
  `aria_tree_js_template`).
- [[iteration-61-script-runner-recorder]] — script integration; the
  recorder may emit `page_map:` references when the source map is
  present. Contains the iter-62-deferred branches D1 wires up.
- [[iteration-61v-navigate-and-screenshot-completion]] — navigate
  state machine used by C1 auth-redirect detection.
- [[iteration-61w-security-hardening-and-cleanup]] — RefStore caps
  and cookie scoping the crawler reuses.
- [[iteration-61z-discipline-skill-integration]] — discipline gates
  this plan is structured to pass.
