---
title: "Consent Dismissal Research"
type: research
status: completed
date: 2026-04-08
tags:
  - consent
  - cmp
  - extension
---

# Consent Dismissal Research

## Problem

Consent Management Platform (CMP) pop-ups block automated tooling. They interfere with
performance measurements, DOM/accessibility analysis, and screenshot capture. Different sites
use different CMPs (Consentmanager, OneTrust, Cookiebot, Didomi, Quantcast, etc.).

## Approaches Evaluated

### 1. Browser Extension (Consent-O-Matic) — CHOSEN

Install the [Consent-O-Matic](https://github.com/cavi-au/Consent-O-Matic) extension into
the Firefox profile. It automatically detects and dismisses consent banners from all major CMPs.

**Pros:**
- Community-maintained rule database covering 100+ CMPs
- Zero runtime overhead after initial installation
- Works transparently — no post-navigation hooks needed
- MIT licensed, open source
- ~111k daily users on AMO — well tested

**Cons:**
- Requires network access on first run (to download XPI from AMO; cached afterwards)
- Extension updates require updating the cached XPI URL
- Binary size unaffected (downloaded at runtime, not embedded)

**Implementation:** Download XPI from AMO, cache in platform cache dir, copy as
`{extension-id}.xpi` into `<profile>/extensions/`. Firefox auto-installs extensions from
the profile `extensions/` directory on startup. No signing workarounds needed since AMO
extensions are already Mozilla-signed.

### 2. JS Injection — REJECTED

Detect CMP and run dismiss script after each navigation.

**Pros:**
- No extension dependency
- Works with any profile configuration

**Cons:**
- Fragile — must maintain selector/script database per CMP
- Must hook every navigation event
- Race conditions between page load and injection timing
- Significant ongoing maintenance burden
- Already partially implemented in cookies.rs for detection only — extending
  to dismissal would require per-CMP scripts

### 3. Profile Pre-configuration — REJECTED

Pre-set consent cookies in the Firefox profile before launch.

**Pros:**
- No runtime overhead
- No extension dependency

**Cons:**
- Completely site-specific — must know exact cookie names/values per site
- Doesn't scale to arbitrary sites
- Consent cookies expire and change format
- No generic solution possible

## Decision

**Use Consent-O-Matic extension** via `--auto-consent` flag on `ff-rdp launch`.

- Extension ID: `gdpr@cavi.au.dk`
- AMO URL: `https://addons.mozilla.org/firefox/downloads/file/4515369/consent_o_matic-1.1.5.xpi`
- Cache location: `<platform-cache-dir>/ff-rdp/extensions/gdpr@cavi.au.dk.xpi`
- Requires `--profile` or `--temp-profile` (extension must be placed before Firefox starts)

## Known CMP-specific Workarounds (for reference)

These site-specific workarounds remain useful for targeted automation:

| CMP | Workaround |
|-----|-----------|
| Consentmanager | `window.cmp_noscreen = true` |
| OneTrust | Set `OptanonAlertBoxClosed` cookie |
| Cookiebot | `window.CookieConsent.submit()` |
| Didomi | `window.didomiConfig = {notice: {enable: false}}` (before load) |
