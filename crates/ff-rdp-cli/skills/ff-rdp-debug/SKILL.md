---
name: ff-rdp-debug
description: Symptom-routed web-bug debug playbooks driven by ff-rdp. Triggers on phrases like "/ff-rdp-debug", "debug this page", "why is X failing in the browser", "login doesn't work", "form submit isn't working", "page is broken", "ChunkLoadError", "manifest parse error". Runs deterministic probe sequences against a live Firefox tab via the ff-rdp CLI, terminating early at the first conclusive signal.
user_invocable: true
---

# ff-rdp-debug

A symptom-routed debug skill for web bugs, driven entirely by the `ff-rdp` CLI
against a live Firefox tab. Every playbook is a small hypothesis tree: 2–5
probe commands, each with a `signal → conclude` rule that lets the skill
**exit early** the moment it has enough evidence to name the failing layer.

This skill exists because LLM-driven debugging without structure tends to
"poke around" — drill into the first interesting signal, miss the
conclusive one, and end with a confident wrong diagnosis. The playbooks are
the antidote: deterministic command sequences derived from real dogfooding
bugs, each ending in a *named layer* (e.g. "Set-Cookie stripped at edge",
"React onChange not fired by value-only mutation") rather than "check
network".

## Trigger phrases

The skill activates on natural-language descriptions of a web bug. The
catalog of trigger keywords below maps to specific playbooks. Phrasings
are deliberately non-expert ("login doesn't work" beats "session cookie
not persisted") because that's what real users type.

Top-level triggers: `/ff-rdp-debug`, "debug this page", "why is X failing
in the browser", "page is broken", "something's wrong with the site",
"site doesn't work right".

## Symptom router

The router is **keyword-first, deterministic**. Match the user's phrasing
against the table below; if multiple playbooks match, run the *most
specific* first (longest matched phrase wins). No match → run `K0` (broad
sweep) and then prompt for a more specific symptom.

| Keywords (any of)                                                                | Playbook(s)        |
|----------------------------------------------------------------------------------|--------------------|
| set-cookie, login, cookie missing, session, "stays logged out", 401 after login  | `A1` then `A2`     |
| samesite, "cookie shows in devtools but not on server", "works in chrome breaks in firefox", 401 on every api | `A2`               |
| chunk, "Loading chunk", "ChunkLoadError", module, "broken after deploy"          | `E1`               |
| manifest, webmanifest, pwa, "manifest parse error"                               | `E3`               |
| "no network", "button does nothing", "submit goes nowhere", "request never fires"| `B5`               |
| react, "value didn't update", "dropdown empty", autocomplete, "controlled input" | `C1`               |
| dropdown, "role=option", combobox, headlessui, downshift, "option unclickable"   | `C2`               |
| consent, overlay, unclickable, banner, OneTrust, Didomi, cmp, "clicks do nothing"| `C3`               |
| "trailing slash", "JSON.parse", "unexpected token <", "API returns HTML"         | `D2`               |
| (nothing matched, or "page is broken", "something weird")                        | `K0`               |

After multi-match, the skill announces which playbook it's running and
why, so the user can redirect early. Example: "matched `A1` (set-cookie
strip) and `A2` (samesite); running `A1` first because it terminates
faster — interrupt if your symptom is firefox-only."

## Prelude commands

Every invocation begins with:

1. `ff-rdp doctor` — verify daemon up + Firefox connected. If it fails,
   run `ff-rdp launch --headless --auto-consent` first, then retry
   `doctor`.
2. `ff-rdp tabs` — pick the active target. If no tab matches the URL the
   user mentioned, run `ff-rdp launch --headless --auto-consent <url>`
   (or `ff-rdp navigate <url>` if a tab exists).

If the user has not named a URL, assume the current focused tab is the
target and skip the launch step.

## Capture-diff primitive

Several playbooks (auth, storage, consent) need a *before/after* view of
console + cookies + storage around a single user action. Treat this as a
reusable pattern, not a CLI flag:

```
pre  := { console (last 20), cookies, localStorage keys, URL }
act  := <single ff-rdp action>     # e.g. click, type, navigate
post := pre-shape, re-captured
diff := what's new in console / what changed in cookies / what changed in storage
```

Exact commands the skill runs for `pre` and `post`:

```bash
ff-rdp console --limit 20 --jq '.results'
ff-rdp cookies --jq '.results'
ff-rdp storage local --jq '.results | keys'
ff-rdp eval 'location.href'
```

Then the action (one command), then the same four commands again. The
`diff` is computed in-agent (set-diff on cookie names, console messages,
localStorage keys; string-equal on URL).

Playbooks that use capture-diff: `A1`, `A2`, `C3` (and `A3`/`F1`/`F2` in
later tiers).

## Hypothesis-tree early exit

Playbooks are **not** checklists. Each probe step has either:

- a **conclusive** signal — skill terminates and emits the diagnosis
  block, OR
- a **narrowing** signal — skill proceeds to next step, having ruled
  out one branch.

Between steps the skill emits a short structured update:

```
## Step N: <command> → <one-line summary>
- known: <what we've established>
- ruled out: <playbook IDs no longer in play>
- next: <step N+1 description, or "concluding">
```

This lets the user interrupt at any point if they recognize the
diagnosis before the skill formally terminates.

## Output contract

Final report shape (on conclusive diagnosis):

```
## Diagnosis: <layer label>

**Evidence:**
- command: `ff-rdp …`
  key: <jq path>: <value>
- command: `ff-rdp …`
  key: <jq path>: <value>

**Ruled out:**
- <playbook ID> — <one-line reason>

**Recommended fix:** <one line, layer-specific>
```

On `K0` / inconclusive, the skill outputs:

```
## Inconclusive — broad sweep complete

Three follow-up questions (answer any one):
1. <question>
2. <question>
3. <question>
```

## Playbook index

Tier 1 (v0 ship target — 10 playbooks):

| ID  | Layer                                   | Trigger keywords                                                |
|-----|-----------------------------------------|-----------------------------------------------------------------|
| A1  | edge/proxy strips Set-Cookie            | set-cookie, login, "stays logged out", session                  |
| A2  | browser drops cookie (SameSite/Secure)  | samesite, secure-on-http, "401 on every api"                    |
| B5  | request never fires                     | "button does nothing", "no network", submit goes nowhere        |
| C1  | React onChange not fired (value-only)   | react, autocomplete, "value didn't update", controlled input    |
| C2  | custom dropdown (role-based, portal)    | dropdown, combobox, role=option, headlessui                     |
| C3  | consent banner intercepts clicks        | consent, overlay, banner, OneTrust, Didomi                      |
| D2  | trailing-slash redirect → JSON parse    | "JSON.parse", "unexpected token <", "API returns HTML"          |
| E1  | ChunkLoadError after deploy             | chunk, "Loading chunk", deploy                                  |
| E3  | manifest fetch returns HTML             | manifest, webmanifest, pwa                                      |
| K0  | fallback / unknown                      | (nothing matched)                                               |

Each playbook lives in `playbooks/<ID>.md`:

- [[playbooks/A1]] — Set-Cookie stripped at the edge
- [[playbooks/A2]] — SameSite / Secure dropping cookie before send
- [[playbooks/B5]] — Request never fires
- [[playbooks/C1]] — React onChange not fired by direct value mutation
- [[playbooks/C2]] — Custom dropdown can't be clicked
- [[playbooks/C3]] — Consent banner intercepting clicks
- [[playbooks/D2]] — Trailing-slash redirect → JSON parse error
- [[playbooks/E1]] — ChunkLoadError after deploy
- [[playbooks/E3]] — Manifest fetch returns HTML
- [[playbooks/K0]] — Unknown symptom / broad sweep

## Fallback for iter-57 flags

Two flags from iter-57 tighten Tier 1 playbooks:

- `network --headers` — A1, A2, B5, E3 use this to read response headers.
  **Fallback:** if the running `ff-rdp` rejects `--headers`, run
  `curl -i <url>` against the same endpoint and reason from those
  headers.
- `click --wait-for-network <pattern>` — A1, B5, C-family.
  **Fallback:** `ff-rdp click <sel>` then `sleep 4` then
  `ff-rdp network --filter <pattern>`.

Playbooks detect missing flags by inspecting `--help` output once at
prelude time. If absent, they silently substitute the fallback path.

## How to add a playbook

See [[kb/skills/ff-rdp-debug-playbooks]] for the full 32-playbook catalog
(Tier 1 in this skill is the first 10). To promote a Tier 2/3 entry: copy
the catalog block into a new `playbooks/<ID>.md`, build a fixture under
`evals/fixtures/<ID>/`, and add the keywords to the symptom router table
above.
