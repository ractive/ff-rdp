---
branch: iter-144/session-hygiene-followup
date: 2026-08-11
depends_on:
  - kb/iterations/iteration-142-session-hygiene.md
dogfood_path: |
  ff-rdp launch --headless --auto-consent --port 6100
  ff-rdp navigate https://www.bbc.com/news --port 6100
  ff-rdp tabs --port 6100
  # → auto_consent must not report success unless something was actually
  #   dismissed; a Consent-O-Matic Options tab must not pollute --tab N indices
  ff-rdp screenshot --full-page --port 6100
  # → BBC News full-page capture must not duplicate the sticky header
first_call_sites: []
status: done
---

# Iteration 144: session hygiene follow-up — consent honesty, screenshot dedup, locale

From [[dogfooding-session-63]], carried over from [[iteration-142-session-hygiene]] — Themes C
and D, plus the console-locale item from Theme F, were deferred rather than landed unverified.
iteration-142's own Notes section explicitly sanctions this: "Themes are independent; if the
iteration runs long, land [what verifies] and defer [what doesn't] to a sibling plan rather than
ticking ACs that were not verified." iteration-142 landed Themes A, B, E, and the `wait
--sleep-ms` half of Theme F with live-Firefox-verified fixes; this plan picks up the rest.

## Why these three were deferred (not just "ran out of time")

- **Theme C (`--auto-consent` honesty)**: `launch --auto-consent`'s `auto_consent: true` field is
  set unconditionally from the CLI flag (`crates/ff-rdp-cli/src/commands/launch.rs:603`) — it
  reports "the extension was installed", not "something was dismissed". `launch` returns before
  any page is loaded, so it structurally *cannot* know at that point whether the
  Consent-O-Matic extension will find/dismiss anything later. Making the field honest requires
  either renaming its semantics (`extension_installed` vs. a real dismiss signal) or moving the
  honesty check to `navigate`/`consent accept` — a design decision affecting the JSON contract of
  at least two commands, not a same-iteration drop-in fix.
- **Theme C (BBC CMP coverage)**: adding a second CMP adapter (`#bbccookies-continue-button`)
  needs its own scoped design (how does auto-detection choose between Sourcepoint and a
  BBC-style adapter without site-specific hardcoding?) plus a live network test against
  www.bbc.com — reasonable scope for a dedicated iteration, not a bolt-on.
- **Theme D (full-page screenshot header dedup)**: the stitching pass lives in the same code
  iteration-135 (screenshot-ff153-capture-drift) hardened against real capture-quality
  regressions; freezing sticky/fixed elements mid-stitch is exactly the kind of change that
  needs careful before/after pixel verification against that iteration's fixtures, not a quick
  pass.
- **Theme F (console locale pinning)**: `crates/ff-rdp-cli/src/commands/launch.rs`'s `USER_JS`
  constant *already* sets `intl.accept_languages`, `intl.locale.requested`, and
  `intl.locale.matchOS` to pin English — added all the way back in iter-61j (dogfood-51), git
  history confirms (`git log -S intl.accept_languages`). Dogfooding session 63 observed German
  console output anyway, on a *later* iteration. This means either (a) the existing pref pin is
  insufficient because the underlying Firefox binary has no `en-US` language pack installed (a
  requested locale a browser can't satisfy silently falls back — a real, known Firefox behavior
  on distro packages that ship only a localized langpack) and a different fix is needed (e.g.
  bundling/detecting available langpacks, or a different pref), or (b) the report predates the
  iter-61j fix and is stale. **This iteration's implementer has no non-English-locale Firefox
  available to reproduce with** — the dev environment is English macOS. Do not guess at a fix
  without reproducing the symptom first (per the run guidance below); if a non-English Firefox
  build/langpack cannot be obtained, say so and re-defer rather than landing an unverifiable
  "fix".

## Themes

### Theme C — `--auto-consent` honesty + BBC-style CMP coverage

Two asks, potentially two sub-iterations if the first turns out to need more design than expected:

1. Decide what `launch`'s `auto_consent` field should mean now that it can't attest to a real
   dismiss. Candidates: rename to `auto_consent_extension_installed` (keep `auto_consent` for a
   future field that *can* attest, once one exists) and add a genuine dismiss-attestation signal
   surfaced by `navigate` or a new `consent status`. Whichever direction, `--help`/docs must match
   and existing consumers of the `auto_consent` field (grep before renaming) must not silently
   break — this may itself want a small research note before code.
2. Add a BBC-style CMP adapter (`#bbccookies-continue-button` / "Yes, I agree") to whatever
   auto-detection `consent accept` uses today, without hardcoding to BBC specifically if a more
   general non-Sourcepoint pattern exists (check what heuristics Consent-O-Matic itself already
   ships before writing a bespoke one).
3. `--auto-consent` leaves a permanent `Consent-O-Matic Options` tab in every `tabs` listing —
   filter it out of `tabs`, or close it once the extension has initialized.

### Theme D — full-page screenshot duplicates the sticky header

Freeze sticky/fixed elements after the first capture band, or capture in a single pass where
possible. Verify against iteration-135's fixtures (`kb/iterations/iteration-135-*`) before and
after — that iteration exists specifically because screenshot stitching is easy to regress.

### Theme F (carryover) — console locale reproducibility

Reproduce the symptom first: obtain or build a Firefox with a non-English UI locale (a langpack
install, or `MOZ_LOCALE`-style override, or a `--lang` flag if headless Firefox has one) and
confirm German (or any non-English) console/error text actually appears despite the existing
`intl.*` prefs in `USER_JS`. Only then diagnose why the existing pin doesn't hold. If no
non-English Firefox can be obtained in the implementation environment, defer again with a note
rather than shipping a guess.

## Acceptance Criteria [4/5]

- [x] live_144_auto_consent_field_honest: `launch --auto-consent`'s reported field never claims a
      dismiss happened when `tabs`/a follow-up check shows the CMP banner still present —
      verified: `results.auto_consent_extension_installed=true` and the old `auto_consent` key is
      gone (`crates/ff-rdp-cli/tests/live/live_144_session_hygiene_followup.rs`)
- [x] live_144_bbc_cmp_dismissed: `consent accept` dismisses BBC's cookie banner
      (`#bbccookies-continue-button`) on www.bbc.com — verified live against the real site:
      `results = {"cmp":"bbc","action":"accepted"}` and the control's post-click bounding rect is
      zero-size (`crates/ff-rdp-cli/tests/live/live_144_session_hygiene_followup.rs`,
      network-gated on `FF_RDP_LIVE_NETWORK_TESTS=1`)
- [x] live_144_no_consent_o_matic_tab_leak: `tabs` after `--auto-consent` does not include a
      `Consent-O-Matic Options` entry (or it is filtered from the listing) — verified
      (`crates/ff-rdp-cli/tests/live/live_144_session_hygiene_followup.rs`)
- [x] live_144_full_page_no_duplicate_header: full-page capture of a sticky-header page (BBC
      News or an equivalent fixture) has no repeated header band, verified pixel-level — verified
      against a deterministic local fixture via PNG row decoding
      (`crates/ff-rdp-cli/tests/live/live_144_session_hygiene_followup.rs`); see that test's
      module doc for why this lands as a forward-looking regression guard rather than a
      reproduced-then-fixed defect — the historic BBC symptom could not be reproduced in this
      environment despite a deliberate before/after attempt (also tried directly against the
      real BBC page; no duplicate found there either). The freeze/restore mitigation is landed
      regardless, per DEC-028.
- [deferred — new plan: kb/iterations/iteration-147-console-locale-repro.md] live_144_console_locale_pinned:
      re-deferred a second time — this implementation environment has only an English macOS
      Firefox available (checked for a `--lang` flag and a `MOZ_LOCALE`-style override; neither
      exists on this build), so the symptom cannot be reproduced here either. See
      [[iteration-147-console-locale-repro]] for what would unblock it.

## Notes

Same independence rule as iteration-142: these three sub-themes don't depend on each other. If
Theme F still can't be reproduced in whatever environment implements this plan, split it into its
own plan again rather than blocking C/D, and say so explicitly rather than silently dropping the
AC.

- **Precedent from [[iteration-143-native-a11y-tree]]** (merged ahead of this plan landing): two
  patterns there are directly reusable here.
  1. *Restore-only-what-you-changed* (DEC-027): `AccessibilityActor::enable_service` is only
     paired with a matching `disable_service` when the caller's own opt-in call is what turned the
     state on, never when it was already in that state for another reason. Theme C's
     `auto_consent` field-honesty redesign is the same shape of problem (a command reporting on
     browser-global/session state it did not unilaterally create) — worth checking whether the
     same "did I cause this, or was it already true" check applies before inventing a new
     contract.
  2. *Bounded deadlines on RDP calls that can stall instead of error* (`A11Y_WALKER_TIMEOUT`,
     iter-143 Theme C, working around the iter-136 walker stall): if Theme D's screenshot-stitch
     investigation or Theme F's locale reproduction turns up an RDP call that blocks instead of
     failing fast, narrowing the transport's read timeout around just that call (via
     `RdpTransport::set_read_timeout`/`read_timeout`, restoring the previous value afterward) is
     the established pattern rather than a bespoke one.
