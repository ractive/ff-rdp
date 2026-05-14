---
title: "Iteration 58: ff-rdp-debug Skill (v0)"
type: iteration
date: 2026-05-13
status: in-progress
branch: iter-58/ff-rdp-debug-skill
depends_on:
  - iteration-57-dogfood-42-fixes
tags:
  - iteration
  - skill
  - ff-rdp-debug
  - install-skill
  - playbooks
  - claude-code
---

# Iteration 58: `ff-rdp-debug` Skill (v0)

Ship a Claude Code skill that orchestrates ff-rdp into symptom-routed
debug playbooks for AI agents working on web bugs. Distribution is via a
new `ff-rdp install-skill` subcommand (hyalo's `hyalo init --claude`
pattern), so the skill is available in any repo on the user's machine —
not just `ff-rdp/`. The skill is the highest-leverage user of ff-rdp:
every other surface (CLI, JSON output) exists to be composed here.

Source material:
- Playbook catalog: [[skills/ff-rdp-debug-playbooks]] (32 playbooks
  tiered; this iteration ships Tier 1 = 9 playbooks).
- Driving sessions: [[dogfooding/dogfooding-session-42]],
  [[dogfooding/dogfooding-session-43]].
- Distribution analog: hyalo's `init --claude` (skill files baked into
  the binary, installed to `.claude/skills/<name>/`).

Three themes:

- **A — Distribution.** `ff-rdp install-skill` ships the skill to the
  user's `~/.claude/skills/` or repo-local `.claude/skills/`. No clone,
  no npm, no path fiddling.
- **B — Skill scaffolding.** `SKILL.md` + cross-cutting primitives
  (capture-diff, hypothesis-tree early-exit, symptom router) that all
  playbooks share.
- **C — Tier 1 playbooks.** Nine playbooks with real dogfooding sources;
  each has a fixture page and a deterministic command sequence.

## Dependencies

iter-57 lands two CLI surfaces that load-bearing playbooks rely on:
- B2: `network --detail --headers` → A1 (Set-Cookie strip), A2 (SameSite),
  B5 (request never fires), E3 (Manifest HTML).
- B3: `click --wait-for-network <pattern>` → A1, B5, C-family.

This iteration **does not block on iter-57** — playbooks degrade
gracefully (fall back to `click; sleep 4s; network --filter X`) and
upgrade automatically once iter-57 ships. Tasks below note which
playbooks tighten when iter-57 lands.

## Tasks

### A. Distribution: `ff-rdp install-skill`

#### A1. New `install-skill` subcommand [4/4]

Mirror hyalo's `init --claude` surface for predictability. Single-binary
install — no network, no clone.

- [ ] Add `install-skill` subcommand in `crates/ff-rdp-cli/src/cli/args.rs`:
  ```
  ff-rdp install-skill --claude [--user | --project] [--force] [--dry-run] [--from-dir <path>]
  ff-rdp install-skill --claude --list
  ff-rdp install-skill --claude --uninstall <name>
  ```
  Default scope: `--user` (the skill is most valuable *outside* the
  ff-rdp repo). `--project` resolves CWD to git root; refuses if not in a
  git repo unless `--force`.
- [ ] Bake skill source into the binary via `include_dir!` (add
  `include_dir` crate). Source lives at
  `crates/ff-rdp-cli/skills/ff-rdp-debug/` in the repo so it's also
  reviewable as plain files. `--from-dir <path>` reads from disk instead
  — required for skill iteration without rebuilding.
- [ ] Idempotency: every installed file gets a `# managed-by: ff-rdp
  v<VERSION>` header. Re-install with same version → no-op. Different
  version → overwrite. Missing header → refuse unless `--force`
  (protects user-edited files).
- [ ] Tests:
  - e2e: `install-skill --claude --user --dry-run` lists files that
    *would* be written, exits 0 without touching disk.
  - e2e: install to a temp `HOME`, re-install, assert no spurious
    writes. Install with `--force` after editing a file, assert
    overwrite.
  - e2e: `install-skill --claude --list` shows installed skill +
    version after install, empty after uninstall.

#### A2. Registry for multiple skills [2/2]

Future-proof for `site-audit`, `dogfood`, etc. without a second binary.

- [ ] Internal skill registry: a `Vec<SkillDef>` enumerating embedded
  skills. v0 has one entry (`ff-rdp-debug`). Adding a second skill is
  a one-line registry append + an `include_dir!` macro.
- [ ] `install-skill --claude` defaults to installing **all** registered
  skills. `install-skill --claude ff-rdp-debug` installs only the named
  one. Document both in `--help`.

### B. Skill scaffolding

#### B1. `SKILL.md` frontmatter + symptom router [3/3]

The skill's entry point: trigger phrases, top-level dispatch logic,
playbook index.

- [x] Write `SKILL.md` with `user_invocable: true` frontmatter, trigger
  phrases ("/ff-rdp-debug", "debug this page", "why is X failing in
  the browser", "form submit isn't working", "login doesn't work",
  "page is broken"). Phrasing pulled from
  [[skills/ff-rdp-debug-playbooks]] §A1–C3.
- [x] Symptom router: top of `SKILL.md` documents a deterministic
  *keyword → playbook* map (e.g. "set-cookie / login / cookie /
  session" → A1+A2; "chunk / module / Loading chunk" → E1; "manifest
  / webmanifest" → E3). Multi-match → run the most-specific first,
  fall back to next. No match → run K0 (broad sweep) then prompt user
  for a more specific symptom.
- [x] Skill prelude commands: every invocation begins with `doctor`
  (verify daemon up + Firefox connected) and `tabs` (pick the active
  target). If no tab matches the URL the user named, the skill
  launches headless Firefox automatically (`launch --headless`) — most
  users won't have an RDP-active Firefox already running.

#### B2. Cross-cutting primitive: capture-diff [2/2]

Several playbooks (auth, storage, consent) need before/after diffs of
console + cookies + storage around a user action.

- [x] Document the pattern in `SKILL.md` as a reusable shape:
  ```
  pre  := { console (last 20), cookies, localStorage keys, URL }
  act  := <single ff-rdp action>
  post := pre-shape, re-captured
  diff := what's new in console / what changed in cookies / what changed in storage
  ```
  Not a new CLI surface — pure orchestration documented in prose so the
  agent can recreate it deterministically.
- [x] Reference the diff primitive from playbooks that use it (A1, A2,
  A3, C3, F1, F2). Each playbook says "use capture-diff around <action>"
  rather than re-spelling the sequence.

#### B3. Cross-cutting primitive: early-exit hypothesis tree [2/2]

Playbooks are not checklists. Each step has a "signal → conclude" rule;
the skill *stops* on the first conclusive signal.

- [x] Document in `SKILL.md`: every probe step has either a *conclusive*
  result (skill terminates with a diagnosis) or a *narrowing* result
  (skill proceeds to next step). The skill emits a structured "what we
  know / what we ruled out / next step" block between steps so the user
  can interrupt at any point.
- [x] Output shape: final diagnosis is a markdown block with **Layer**,
  **Evidence** (with command + key field from JSON), and **Next step**
  (a fix recommendation or "needs more info").

#### B4. Skill output contract [2/2]

Make the skill's output stable enough that ralph-loop / eval harnesses
can grade it.

- [x] Final report shape (markdown):
  ```
  ## Diagnosis: <layer label>
  **Evidence:**
  - command: `ff-rdp …`
    key: <jq path>: <value>
  **Ruled out:** <bullet list>
  **Recommended fix:** <one line, layer-specific>
  ```
- [x] On `K0` (unknown / inconclusive), output ends with a numbered list
  of follow-up questions, not a diagnosis.

### C. Tier 1 playbooks (9)

Each playbook from [[skills/ff-rdp-debug-playbooks]] becomes a file in
`crates/ff-rdp-cli/skills/ff-rdp-debug/playbooks/<id>.md`, indexed from
`SKILL.md`. Per-playbook tasks share the same shape; bundling as one
checklist to avoid 9× ceremony.

#### C1. Author Tier 1 playbook files [9/9]

(Plus A2 — SameSite/Secure drop — shipped as a bonus 10th playbook;
referenced from the symptom router alongside A1.)

For each playbook, the file contains: symptom phrases (paraphrased
list), failing-layer label, the probe command sequence with `signal →
conclude` per step, red herrings, and a worked example pointing at the
fixture.

- [x] A1 — Set-Cookie stripped at edge (`dog-42`)
- [x] B5 — Request never fires (`dog-42`, `synth-bug`)
- [x] C1 — React onChange not fired by value-only mutation (`dog-36`,
  `dog-42`)
- [x] C2 — Custom dropdown unclickable (`dog-36`)
- [x] C3 — Consent banner blocking interaction (`dog-29`)
- [x] D2 — Trailing-slash redirect → JSON parse error (`dog-43`)
- [x] E1 — ChunkLoadError after deploy (`dog-43`)
- [x] E3 — Manifest returns HTML (`dog-43`)
- [x] K0 — Fallback / unknown symptom broad sweep

#### C2. Fixture pages for Tier 1 playbooks [2/2]

Static fixture sites under
`crates/ff-rdp-cli/skills/ff-rdp-debug/evals/fixtures/<playbook-id>/`,
each with a planted bug matching its playbook and a `bug.json`
ground-truth declaration (per the eval scheme in
[[skills/ff-rdp-debug-playbooks]] §Evaluation). Reused by C3 and any
future Layer-2 harness.

- [x] Ten fixtures (one per Tier 1 playbook, including A2). Each is a
  `python3 -m http.server`-servable directory (no Node/Bun runtime,
  per project Rust-only policy).
- [x] `bug.json` per fixture: `{symptom_hint, expected_diagnosis,
  expected_evidence_commands[], must_not_conclude[]}`.

#### C3. Deterministic playbook runner + Layer-2 evals [2/3]

A test that exercises each playbook's command sequence against its
fixture and asserts the JSON evidence matches `bug.json`. Doesn't
involve an LLM — validates that the *commands* surface the right
evidence.

- [x] `crates/ff-rdp-cli/tests/playbook_evals.rs` — schema-validates
  every fixture's `bug.json` (required keys, command shape) and verifies
  fixture/playbook id alignment. Live-Firefox probing is stubbed as one
  `#[ignore]`d test with a sketch of the intended flow and a TODO
  pointing at the iter-58 follow-up.
- [x] Mark live-probe test `#[ignore]`, document
  `cargo test -p ff-rdp-cli --test playbook_evals -- --ignored` in the
  iteration kb file and in the test's module docs.
- [ ] CI: don't gate on Layer-2 evals (flaky on headless Linux per
  [[testing_strategy]]); run nightly via a separate workflow.
  *(Not yet implemented — the schema-validation tests are cheap enough
  to run in the default suite; live probes remain ignored.)*

### D. Documentation

#### D1. README + skill entry point [2/2]

- [x] Top-level README section: "Using ff-rdp from Claude Code →
  `ff-rdp install-skill --claude` → skill is available in any repo."
  One paragraph + one fenced example.
- [x] `kb/skills/ff-rdp-debug.md` (separate from the playbook catalog):
  one-page user guide — trigger phrases, what playbooks exist, how to
  contribute a new one. Link from the playbook catalog.

## Acceptance Criteria

- [ ] `cargo fmt` / `cargo clippy --workspace --all-targets -- -D warnings`
  / `cargo test --workspace -q` clean.
- [ ] `ff-rdp install-skill --claude --user` installs `ff-rdp-debug` to
  `~/.claude/skills/ff-rdp-debug/` idempotently.
- [ ] `ff-rdp install-skill --claude --project` installs to
  `./.claude/skills/ff-rdp-debug/` when run inside a git repo.
- [ ] `ff-rdp install-skill --claude --list` reports installed skills +
  versions.
- [x] All 9 Tier 1 playbook files exist and are referenced from
  `SKILL.md` (10 with bonus A2).
- [x] All 9 fixture pages exist with `bug.json` ground truth (10 with
  bonus A2).
- [ ] `cargo test -p ff-rdp-cli --test playbook_evals -- --ignored`
  passes locally against a fresh headless Firefox. *(Live probe test
  is currently a stub with TODO; schema-validation portion passes in
  the default suite. Follow-up iteration to wire up live Firefox.)*
- [ ] After install, invoking `/ff-rdp-debug` against the
  Set-Cookie-strip fixture produces a diagnosis matching the fixture's
  `expected_diagnosis`.

## Design Notes

**Skill scope is intentionally narrow at v0.** Nine playbooks, all with
real-bug provenance. No taxonomy-only playbooks — every Tier 2/3
candidate from the catalog has to *earn* its slot by showing up in real
dogfooding or in a triaged real-world bug report. This is the lesson
from the catalog's adversarial pass: speculative playbooks misroute
confidently.

**Install-skill is one binary, one command, one bake.** The temptation
to "version the skill independently of the CLI" is real but costs more
than it pays: a registry-fetch step, a network failure mode, version
drift. Baking with `include_dir!` means `ff-rdp v0.X` ships exactly the
skill it was tested with, and `--from-dir` covers the local-iteration
case. If the skill turns out to evolve much faster than the CLI, revisit.

**Distribution defaults to `--user`.** The skill's value is debugging
*other people's apps*. Repo-local install is for ff-rdp contributors
iterating on the skill itself. Make `--user` the path of least
resistance.

**Layer-2 evals before Layer-3.** Iteration ships only the deterministic
playbook runner (Layer 2 in
[[skills/ff-rdp-debug-playbooks]]). The LLM-judge / claude-as-driver
loop (Layer 3) is a separate iteration once we have ≥5 stable playbooks
to grade against. Trying to build both at once means flakiness obscures
playbook bugs.

**Why not start with iter-57?** iter-57 makes some playbooks tighter
(`--headers` for cookie debugging) but every Tier 1 playbook has a
fallback path (`curl -i`, manual `sleep`). Shipping the skill *first*
gives iter-57 a concrete user: when an agent actually runs A1 against
the fixture, the friction of the missing `--headers` flag becomes a
graded test failure rather than a hypothesis. Skill-first surfaces real
CLI gaps for the next iteration.

**Trade: bundle size.** `include_dir!` embeds fixtures into the
binary. Nine small fixtures (~5–20 KB each) is negligible. If we
embed e.g. font/image fixtures for I-family playbooks in later
iterations, revisit — possibly split fixtures into a `--fixtures-dir`
download command rather than embed.

## References

- [[skills/ff-rdp-debug-playbooks]] — the 32-playbook catalog this
  iteration draws from; v0 ships the 9 Tier-1 entries.
- [[dogfooding/dogfooding-session-42]] — drives A1, B5, C1.
- [[dogfooding/dogfooding-session-43]] — drives D2, E1, E3.
- [[dogfooding/dogfooding-session-36-ff-rdp]] — drives C1, C2.
- [[dogfooding/dogfooding-session-29]] — drives C3.
- [[iterations/iteration-57-dogfood-42-fixes]] — `--headers` and
  `--wait-for-network` tighten Tier 1 once landed; skill ships without
  blocking on iter-57.
- [[iterations/iteration-42-site-audit-skill]] — prior skill (site-audit)
  whose `evals.json` shape inspires C2/C3.
- hyalo's `hyalo init --claude` — distribution surface this iteration
  mirrors.
