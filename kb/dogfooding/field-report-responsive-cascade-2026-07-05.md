---
title: "Field report: responsive is not media-query-truthful; cascade winner ignores media context"
type: field-report
date: 2026-07-05
status: triaged
source: external agent session (responsive-debugging use case, Prämienrechner form)
tags: [dogfooding, field-report, responsive, cascade, doctor, errors]
---

# Field report — responsive/cascade media-query truthfulness (2026-07-05)

Verbatim feedback from another agent session that used ff-rdp for a real
responsive-debugging task. Triaged into [[iteration-98-media-query-truthfulness]].

## What worked (keep doing this)

> The agent-first design decisions are the best I've seen in a browser CLI:
> `--format text` really is 3–10× leaner than the JSON (and both were parseable
> every time), built-in `--jq`, contextual follow-up hints, meaningful exit
> codes, `doctor` as a single triage command, and secret redaction in trace
> output. The interaction model — auto-waiting click/type with
> `--wait-for text:`/`--wait-for-network` — let me fill and submit the
> Prämienrechner form and read the resulting toast in two commands, no
> screenshot round-trips. `page-text`, `dom --text`, `eval --stringify`,
> `computed`, and `cascade` are exactly the "measure, don't eyeball" tools I
> needed [...]. Launching next to the user's real browser without fighting
> over profiles, or attaching to an already-listening instance, both just
> worked.

## Defect 1 — `responsive` produced a physically impossible CSS state

> At a claimed 390px viewport, `html` measured 390 but media queries never
> flipped (`(min-width:1024px)` styles stayed active, so shell-main reported
> 980px). If I hadn't had an independent Playwright measurement, this would
> have sent me hunting a nonexistent regression in code we'd just fixed. It
> should either drive Firefox's real RDM emulation or self-check `matchMedia`
> against the requested width and refuse/warn when they disagree.

## Defect 2 — `cascade` winner flag ignores media-query context

> `cascade`'s winner flag marked `min-width: 0` as winning while `computed`
> correctly reported 980px from the `(width >= 1024px)` rule. For a command
> whose whole purpose is "explain why this value wins," that's actively
> misleading.

## Nit 1 — `doctor` binary_staleness fires in foreign repos

> `doctor`'s binary_staleness check compared the binary against the neon
> repo's git HEAD (it apparently uses cwd's HEAD — it should only fire inside
> the ff-rdp checkout).

## Nit 2 — errors print twice

> Errors print twice (human line + JSON envelope).

## Net verdict

> I'd reach for it over claude-in-chrome for headless/CI-style verification
> (no extension dependency, scriptable, token-frugal) and over raw
> headless-Chrome screenshots for anything layout-related — but until
> `responsive` and `cascade` are media-query-truthful, I'd cross-check any
> viewport-dependent conclusion with real viewport emulation. Fix those two
> and it's the best agent-facing browser tool I've used.
