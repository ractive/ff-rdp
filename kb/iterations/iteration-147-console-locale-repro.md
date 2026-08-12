---
title: "Iteration 147: console locale reproducibility"
type: iteration
date: 2026-08-12
status: planned
branch: iter-147/console-locale-repro
depends_on:
  - kb/iterations/iteration-144-session-hygiene-followup.md
first_call_sites: []
dogfood_path: |
  ff-rdp launch --headless --port 6100
  # → against a Firefox build/langpack whose UI locale is genuinely
  #   non-English, console/error text must still be English (or the fix
  #   that makes it so must be identified and applied)
tags: [iteration]
---

# Iteration 147: console locale reproducibility

Carried over from [[iteration-144-session-hygiene-followup]] Theme F, itself carried over from
[[iteration-142-session-hygiene]] — this is the second consecutive iteration this item has been
re-deferred, for the same reason both times: **no non-English-locale Firefox is available in the
implementation environment.**

## Why this keeps getting deferred instead of guessed at

`crates/ff-rdp-cli/src/commands/launch.rs`'s `USER_JS` constant already sets
`intl.accept_languages`, `intl.locale.requested`, and `intl.locale.matchOS` to pin English —
added in iter-61j (dogfood-51; confirmed still present via `git log -S intl.accept_languages`).
Dogfooding session 63 observed German console output anyway, on a later iteration than iter-61j.
Two live-Firefox environments were checked during iter-144's implementation:

- macOS Firefox (`/Applications/Firefox.app`) — the only Firefox installed in this implementation
  environment. English-only; ships no non-English langpack; headless Firefox has no `--lang` CLI
  flag (checked `firefox --help` output — no locale-override flag exists on this build).
- No `MOZ_LOCALE`-style environment variable is documented for the installed Firefox version
  (searched the running binary's `--help` and the shipped `application.ini`; none found).

Per iteration-142 and iteration-144's own explicit run guidance, landing a "fix" without
reproducing the symptom first is exactly what these plans forbid — a pref change made without
seeing the German output firsthand cannot be verified to do anything.

## What would unblock this

Any of:
- A Firefox build or profile with a genuine non-English langpack installed (e.g. `de` langpack
  via `about:preferences#general` → Language, or a Linux distro package that ships a localized
  build by default rather than `en-US`).
- A documented `MOZ_LOCALE`/similar environment-variable override for headless Firefox, if one
  exists in a Firefox version newer/older than what's checked here — re-check `firefox --help`
  and `about:support` on whatever build is used to implement this plan.
- Direct access to report a diagnostic profile from the dogfooding-session-63 environment (Firefox
  version, OS, langpack state) that reproduced the original German output, to compare against.

## Tasks

### A. Reproduce
- [ ] Obtain or build a Firefox whose UI locale is verifiably non-English (`about:support` →
  "Application Basics" → confirm locale is not `en-US`).
- [ ] Run `ff-rdp launch --headless` against it and trigger a console error (e.g.
  `ff-rdp eval 'undefinedFn()'`) — confirm whether the error text is English or localized despite
  the `intl.*` prefs.

### B. Diagnose (only after A succeeds)
- [ ] If still localized: identify which additional pref or mechanism the existing `intl.*` pin
  misses (e.g. `general.useragent.locale` on older builds, or a required restart/profile-creation
  ordering issue — prefs might apply too late if Firefox already cached the locale from the OS at
  first run). Cite the Firefox source (searchfox) for whatever pref is found missing.
- [ ] If not localized: the iter-142 dogfooding-session-63 report predates the iter-61j fix, or was
  itself measuring something other than console text (e.g. a localized system dialog, not RDP
  eval output) — close this out as "confirmed already fixed" with the repro steps documented,
  rather than landing a no-op change.

### C. Fix (only if B finds a real gap)
- [ ] Add whatever pref/mechanism B identified to `USER_JS` (or wherever it belongs), with a
  comment citing the Firefox source confirming its effect.

## Acceptance Criteria [0/1]

- [ ] live_147_console_locale_pinned: console output is locale-stable on a genuinely
      non-English-locale Firefox — name the reproduction method used in the test. If no
      non-English Firefox can be obtained, re-defer again (a third time) with a note naming what
      was tried, rather than landing an unverifiable "fix" — do not tick this box without a
      passing named test.

## Design notes

Nothing to design until Theme A (reproduction) succeeds — this plan is intentionally
investigation-first, per CLAUDE.md's "reproduce before diagnosing" rule for exactly this class of
carried-over, environment-blocked item.

## Out of scope

Building or CI-provisioning a non-English Firefox specifically for this repro is out of scope
unless Theme A repeatedly fails for lack of one — if so, that provisioning work should itself be
scoped as a follow-up rather than folded into this investigation.

## References

- [[iteration-144-session-hygiene-followup]]
- [[iteration-142-session-hygiene]]
- [[decision-log]] — DEC-028
