---
title: "ff-rdp-debug — User Guide"
type: skill
status: shipped
date: 2026-05-14
tags:
  - skill
  - ff-rdp-debug
  - claude-code
  - debug
  - user-guide
---

# `ff-rdp-debug` — User Guide

`ff-rdp-debug` is a [[Claude Code]] skill that turns the `ff-rdp` CLI
into a **symptom-routed debugger** for web bugs. You describe the bug
in plain language; the skill picks a playbook, runs 2–5 deterministic
probes against a live Firefox tab, and reports the failing layer with
evidence — terminating as soon as it has enough signal.

See [[skills/ff-rdp-debug-playbooks]] for the full 32-playbook catalog
this v0 draws from. This guide covers the **10 Tier-1 playbooks** that
ship with the skill.

## Install

The skill is baked into the `ff-rdp` binary; no clone, no npm:

```sh
ff-rdp install-skill --claude
```

Default scope is `--user` (installs to `~/.claude/skills/ff-rdp-debug/`)
so the skill is available in every repo. Use `--project` to install to
the current repo's `.claude/skills/` instead.

After install, the skill auto-loads in any Claude Code session.

## Trigger phrases

The skill activates on any of:

- `/ff-rdp-debug`
- "debug this page" / "why is this page broken"
- "login doesn't work" / "stays logged out"
- "form submit isn't working" / "button does nothing"
- "ChunkLoadError" / "Loading chunk failed"
- "manifest parse error" / "PWA install prompt missing"
- "API returns HTML" / "JSON.parse unexpected token <"
- "page is broken" (fallback / broad sweep)

Full keyword → playbook routing table is in the skill's `SKILL.md`.

## Tier 1 playbooks

| ID  | Layer                                  | Triggers on                                            |
|-----|----------------------------------------|--------------------------------------------------------|
| A1  | Set-Cookie stripped at edge            | login fails silently / no session cookie               |
| A2  | SameSite / Secure cookie drop          | 401 on every api / works in Chrome, breaks in Firefox |
| B5  | Request never fires                    | button does nothing, no network on click               |
| C1  | React onChange not fired               | `type` fills field but framework state is stale        |
| C2  | Custom dropdown can't be clicked       | role=option selectors don't match                      |
| C3  | Consent banner intercepts clicks       | clicks succeed but nothing happens                     |
| D2  | Trailing-slash redirect → JSON parse   | "unexpected token <" / API returns HTML                |
| E1  | ChunkLoadError after deploy            | blank page after deploy, hashed chunk 404s             |
| E3  | Manifest fetch returns HTML            | manifest parse error in console                        |
| K0  | Unknown — broad sweep                  | no clear symptom; fallback                             |

Each playbook lives at
`crates/ff-rdp-cli/skills/ff-rdp-debug/playbooks/<id>.md` in this repo,
and at `~/.claude/skills/ff-rdp-debug/playbooks/<id>.md` after install.

## How a session looks

```
> /ff-rdp-debug login doesn't work on this page

Skill: matched A1 (set-cookie strip) and A2 (samesite).
Running prelude…
  $ ff-rdp doctor          → OK
  $ ff-rdp tabs            → tab #1 is http://staging.example.com/login

Running A1.
  Step 1: ff-rdp cookies   → 0 session cookies before submit
  Step 2: ff-rdp click '#submit' --wait-for-network '/api/login'
                            → status=200, no Set-Cookie in response
  Step 3: ff-rdp cookies   → still 0 session cookies
                            → conclusive signal

## Diagnosis: edge/proxy

**Evidence:**
- command: `ff-rdp network --filter '/api/login' --detail --headers`
  key: results[0].response_headers["Set-Cookie"]: (absent)
- command: `ff-rdp cookies`
  key: results: []  (before AND after submit)

**Ruled out:**
- A2 — no cookie exists in the browser to even drop
- B5 — request fired, returned 200

**Recommended fix:** inspect the CDN/proxy config for the auth path;
common cause is a `Cache-Control: public` rule on `/api/login` causing
the CDN to strip `Set-Cookie`. Re-test via `curl -i` against the
upstream origin to confirm.
```

## Contributing a new playbook

Promote a Tier 2 or Tier 3 entry from
[[skills/ff-rdp-debug-playbooks]] by:

1. Copy the catalog block (symptom phrases, probe sequence, layer
   label) into a new file at
   `crates/ff-rdp-cli/skills/ff-rdp-debug/playbooks/<ID>.md`.
2. Author the playbook frontmatter (`id`, `title`, `layer`,
   `symptom_keywords`, `sources`).
3. Build a static fixture at
   `crates/ff-rdp-cli/skills/ff-rdp-debug/evals/fixtures/<ID>/` with
   `index.html` and `bug.json`.
4. Add the keyword(s) to the symptom router table in `SKILL.md`.
5. Run `cargo test -p ff-rdp-cli --test playbook_evals` to confirm the
   fixture/playbook pair is well-formed.

The skill source lives entirely as plain markdown — the only
"compilation" is `include_dir!` baking it into the binary. Open a PR
and the next `ff-rdp` release will pick it up.

## See also

- [[skills/ff-rdp-debug-playbooks]] — full 32-playbook catalog
- [[iterations/iteration-58-ff-rdp-debug-skill]] — this iteration plan
- [[dogfooding/dogfooding-session-42]] — real-bug provenance for A1,
  B5, C1
- [[dogfooding/dogfooding-session-43]] — real-bug provenance for D2,
  E1, E3
