---
title: "Iteration 30: Auto-Consent / CMP Dismissal"
type: iteration
status: completed
date: 2026-04-08
tags:
  - iteration
  - consent
  - cmp
  - ux
  - automation
branch: iter-30/auto-consent
---

# Iteration 30: Auto-Consent / CMP Dismissal

Consent Management Platform (CMP) pop-ups block automated tooling. Evaluate
approaches and implement the best one.

## Background

When using ff-rdp for automated analysis, consent screens block interaction
and skew performance/DOM/a11y measurements. Different sites use different CMPs
(Consentmanager, OneTrust, Cookiebot, Didomi, Quantcast, etc.) with no
universal "disable" API.

### Known CMP-specific workarounds
- **Consentmanager**: `window.cmp_noscreen = true` (used by comparis.ch)
- **OneTrust**: Set `OptanonAlertBoxClosed` cookie
- **Cookiebot**: `window.CookieConsent.submit()`
- **Didomi**: `window.didomiConfig = {notice: {enable: false}}` (before load)

## Research

- [x] Evaluate **browser extension approach** — install a consent-dismissing
  extension in the headless Firefox profile. Candidates:
  - [Consent-O-Matic](https://github.com/cavi-au/Consent-O-Matic) —
    open-source, community-maintained rules for all major CMPs
  - [I don't care about cookies](https://www.i-dont-care-about-cookies.eu/) —
    popular, available as Firefox extension
  - uBlock Origin with CMP filter lists (e.g. EasyList Cookie)
- [x] Evaluate **JS injection approach** — detect CMP and run dismiss script
  after each navigation. Pros: no extension dependency. Cons: fragile, must
  maintain selector database.
- [x] Evaluate **profile pre-configuration** — pre-set consent cookies in the
  Firefox profile before launch. Pros: no runtime overhead. Cons: site-specific.
- [x] Document findings in `kb/research/consent-dismissal.md`

## Implementation

Based on research, implement the chosen approach:

- [x] If extension approach: add `--auto-consent` flag to `ff-rdp launch` that
  installs the chosen extension into the profile automatically
- [x] If JS approach: add `--auto-consent` flag that runs CMP detection +
  dismissal after each `navigate` command
- [x] Ensure `--auto-consent` works with both daemon and direct connection
- [x] Test against at least 3 sites with different CMPs (Consentmanager,
  OneTrust, Cookiebot)
- [x] Document the feature and its limitations

## Test Fixtures

All e2e test fixtures must be recorded from a real Firefox instance — never hand-craft them.
Run with `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored` to record fixtures.
