---
name: dogfood
description: >
  Run a dogfooding session against a real website using ff-rdp. Use this skill whenever
  the user says /dogfood, "dogfood the CLI", "run a dogfooding session", "test ff-rdp on
  a real site", "smoke test all commands", or wants to exercise ff-rdp end-to-end against
  a live page. Also trigger when the user asks to "check what's working", "test recent
  changes", or "regression test ff-rdp".
---

# Dogfooding Session

You are running a hands-on dogfooding session of the `ff-rdp` CLI against a real website.
The goal is to exercise the tool the way a real user or LLM agent would — discovering
friction, verifying recent fixes, and finding new bugs.

This is NOT a rigid playbook. Be curious, follow interesting threads, and adapt to what
you find. The best dogfooding sessions uncover surprises.

## Before you start

### Pick a target site

If the user specified a URL, use that. Otherwise pick something interesting — a site with
real complexity (SPAs, third-party scripts, forms, dynamic content). Good defaults:
- `https://www.comparis.ch/hypotheken` (complex SPA, heavy third-party, good for perf)
- `https://news.ycombinator.com` (simple, fast, good baseline)
- A localhost dev server if the user is iterating on a project

Variety matters — don't always use the same site. Check which sites previous sessions used
and try something different.

### Determine the session number

```bash
hyalo find --property type=dogfooding --sort property:date --reverse --limit 1 --format text
```

The new session number is one higher than the most recent. If the highest is 36, this is 37.
(Some sessions have slug-style names like `dogfooding-session-nova-template-...` — those
don't count toward the numbering.)

### Check what's new

Recent changes inform what to focus on. Skim the git log and recent iterations:

```bash
git log --oneline -20
hyalo find --property type=iteration --property status=completed --sort property:date --reverse --limit 5 --format text
```

New or recently-fixed commands deserve extra attention. If iter-41 added `scroll`, make
sure to exercise scroll thoroughly. If iter-43 added `eval --file`, try piping a script.

Also check the previous dogfooding session for known-broken items worth re-testing:
```bash
hyalo find --property type=dogfooding --sort property:date --reverse --limit 1 --jq '.results[0].file'
```
Read its "Still Broken" or "Issues" section — those are your regression candidates.

### Launch Firefox

```bash
ff-rdp launch --headless --port 6000
```

If Firefox is already running on port 6000, skip this. Verify with `ff-rdp tabs`.

## The session

Structure your session in loose phases. Spend real time on each — don't just fire commands
and move on. Interpret the output, try variations, chain commands together.

### Phase 1: Navigate and orient

Get onto the target site and understand the page.

- `navigate <url>` — does it complete? How long?
- `tabs` — verify tab state
- `page-text` — does it extract meaningful content?
- `snapshot` — is the semantic structure sensible?
- `screenshot -o /tmp/dogfood.png` — visual check (read the image)

If the site requires cookie consent, handle it:
```bash
ff-rdp eval 'document.querySelector("[data-testid=consent-accept], .consent-accept, #onetrust-accept-btn-handler, button[id*=accept]")?.click()'
```

### Phase 2: Explore interactively

Actually USE the site like a human would. This is where you get creative.

- **Search/forms**: `click`, `type`, `wait --selector`, `screenshot`
- **Navigation flow**: `navigate`, `back`, `forward`, `reload`
- **Scroll**: `scroll down`, `scroll to "selector"`, `scroll bottom`
- **Dynamic content**: `wait --text "loaded"`, `eval`, `console --level error`

Don't just test that commands execute — test that they produce *useful* output.
Does `dom "article"` give you what you'd need to understand the page? Does
`geometry ".header"` tell you something actionable?

### Phase 3: Performance and network

- `perf vitals` — are the numbers plausible?
- `perf summary` — resource breakdown
- `perf audit` — flagged issues
- `network --format text` — request inventory
- `network --follow` + navigate to a new page — does streaming work?

Try `--jq` filters to extract specific data. Try `--fields` to customize output.
Try `--format text` vs default JSON.

### Phase 4: Accessibility and structure

- `a11y` — tree structure
- `a11y contrast --fail-only` — WCAG violations
- `dom stats` — DOM complexity
- `computed ".some-element" display,font-size,color` — CSS debugging
- `styles ".element"` — full style inspection
- `responsive ".content" --widths 320,768,1024` — breakpoint behavior

### Phase 5: Advanced / edge cases

Pick a few of these based on what's interesting:

- `eval --file <script>` or `echo '...' | ff-rdp eval --stdin` — complex JS
- `cookies` and `storage localStorage` — data inspection
- `sources` — loaded scripts
- `screenshot --full-page` — does the full page capture work?
- Chained workflows: navigate → wait → screenshot → eval → type → screenshot
- `--jq` edge cases: nested paths, array filters, missing fields
- Error cases: bad selectors, nonexistent elements, invalid URLs
- `llm-help` — is the reference complete and accurate?
- `recipes` — do the examples actually work?

### Phase 6: Regression candidates

Re-test anything flagged as broken in the previous session. Note whether it's fixed,
still broken, or broken differently.

## What to look for

Beyond pass/fail, notice:

- **Friction**: Did you have to retry with different flags? Was the error message helpful?
- **Surprises**: Output format unexpected? Missing data? Extra noise?
- **Missing features**: Did you reach for something that doesn't exist?
- **Performance**: Commands that hang or take unusually long
- **Stderr noise**: Debug messages leaking into output?
- **Consistency**: Does `--format text` match JSON semantically?
- **Documentation**: Would `--help` have told you what you needed?

## Writing the report

Create the session file in the KB:

```bash
hyalo create dogfooding/dogfooding-session-<N>.md \
  --property title="Dogfooding Session <N>" \
  --property type=dogfooding \
  --property date=$(date +%Y-%m-%d) \
  --property status=completed \
  --property site="<site-url>" \
  --property commands_tested='[list, of, commands, used]' \
  --tag dogfooding
```

If `hyalo create` isn't available, use the Write tool with proper frontmatter.

Structure the report like this (adapt freely — not every section applies):

```markdown
# Dogfooding Session <N>

One-line summary of what was tested and the overall vibe.

## What's New Since Last Session
Brief list of features/fixes landed since the previous dogfooding.

## Regression Checks
| Command | Previous Status | Current Status | Notes |
|---------|----------------|----------------|-------|

## Smoke Test Results
| Command | Status | Notes |
|---------|--------|-------|

## Findings

### What Works Well
Highlights — commands or workflows that were satisfying to use.

### Issues Found
Numbered list. For each: what you tried, what happened, what you expected.

### Feature Gaps
Things you wanted but don't exist yet.

## Summary
- X commands tested, Y passed, Z issues found
- Key takeaway in one sentence
```

Link to the previous session and any relevant iterations with `[[wikilinks]]`.

## Ground rules

- **Be honest**: If something is broken, say so clearly. Don't paper over issues.
- **Be specific**: Include the exact command, the exact output (or first few lines),
  and what you expected instead.
- **Be creative**: The best bugs are found off the beaten path. Try weird inputs,
  unusual flag combinations, rapid sequences.
- **Don't stop at the first error**: A command failing is data, not a reason to bail.
  Note it and keep going.
- **Use agents**: Delegate parallelizable exploration to subagents if available
  (e.g., "test all perf commands" and "test all a11y commands" simultaneously).
