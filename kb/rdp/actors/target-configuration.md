---
type: rdp-note
tags:
- rdp
- firefox-server
- actor
- target-configuration
- emulate
date: 2026-07-09
firefox_files:
- devtools/shared/specs/target-configuration.js
- devtools/server/actors/target-configuration.js
title: TargetConfigurationActor
---

# TargetConfigurationActor

Changes per-target ("page environment") settings without requiring
browser-wide pref changes. Obtained via
`WatcherFront::get_target_configuration_actor` (the watcher's
`getTargetConfigurationActor` method). Consumed by the `ff-rdp emulate`
command (iter-103) and the `set_cache_disabled` cache-bypass path.

## Firefox references

| File | Lines | Purpose |
|------|-------|---------|
| `devtools/shared/specs/target-configuration.js` | 14-29 | `SUPPORTED_OPTIONS` dict — every configurable field, all `nullable:*` |
| `devtools/server/actors/target-configuration.js` | 267-315 | `updateConfiguration` dispatch (one setter per field) |
| `devtools/server/actors/target-configuration.js` | 330-368 | Teardown / restore-defaults logic mirrored by `TargetConfigurationFront::reset` |

## Method

- `updateConfiguration({configuration})` — merge a partial patch into the
  target's live configuration. Response echoes the full configuration. Every
  field is nullable, so a call touches only the keys it names.

## Configuration fields (wire names)

The `emulate` CLI exposes the agent-relevant subset:

| Wire field (`SUPPORTED_OPTIONS`) | Type | `emulate` flag | Notes |
|----------------------------------|------|----------------|-------|
| `cacheDisabled` | bool | `--cache on\|off` | `--cache off` → `cacheDisabled: true` (inverted) |
| `colorSchemeSimulation` | string | `--color-scheme light\|dark\|none` | `none` = system default; maps to `prefersColorSchemeOverride` |
| `customUserAgent` | string | `--user-agent <S>` | empty string restores the original UA |
| `overrideDPPX` | number | `--dppx <F>` | `0` clears the override |
| `printSimulationEnabled` | bool | `--print on\|off` | composes with `screenshot` for print audits |
| `touchEventsOverride` | string | `--touch on\|off` | enum: `"enabled"` / `"none"` (not a bool) |
| `javascriptEnabled` | bool | `--js on\|off` | server **reloads the document** when this changes |
| `setTabOffline` | bool | `--offline on\|off` | `navigator.onLine` / fetch failures; reload to reflect |

Fields present in the dict but not exposed by `emulate`: `customFormatters`,
`rdmPaneOrientation`, `reloadOnTouchSimulationToggle`, `restoreFocus`,
`serviceWorkersTestingEnabled`, `isTracerFeatureEnabled`.

## Lifetime

Configuration lives as long as the RDP connection that set it. Under the
daemon that means "until the daemon restarts"; a `--no-daemon` one-shot
process discards the setting on disconnect — the `emulate` envelope then
carries a `lifetime_warning`.

## Spec-fidelity note (iter-103)

The colour-scheme wire field is `colorSchemeSimulation`, **not** `colorScheme`
— an earlier `set_color_scheme_simulation` sent the wrong key and was never
exercised end-to-end (the only live coverage tested cache). iter-103 corrected
the field and added a live probe
(`live_emulate_color_scheme_dark`) asserting `prefers-color-scheme: dark`
actually flips.

## See also

- [[watcher]] — supplies this actor via `getTargetConfigurationActor`.
- [[iteration-103-target-configuration-cli]] — the `emulate` command iteration.
