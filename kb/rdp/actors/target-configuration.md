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

## Current use (2026-10-03): per-command emulation flags

`crates/ff-rdp-core/src/actors/target_configuration.rs` has two stateless
calls: `TargetConfigurationActor::for_watcher` (`getTargetConfigurationActor`
on the tab watcher) and `update_configuration` (`updateConfiguration`, typed by
`specs::target_configuration`). They back three CLI flags, each applied on the
command's own connection:

| Flag | Wire field | Applied |
|------|------------|---------|
| `screenshot --color-scheme light\|dark` | `colorSchemeSimulation` | before the capture; the command polls `matchMedia` until it reports the scheme |
| `screenshot --media print` | `printSimulationEnabled: true` | same; `--media screen` sends nothing |
| `navigate --user-agent UA` | `customUserAgent` | before `navigateTo`, on the navigation watcher |

Verified live (Firefox 157, 2026-10-03): the dark/print styles are in the
captured pixels; the UA override reaches the document request's `User-Agent`
header (`HttpBaseChannel` reads `BrowsingContext.customUserAgent`) and
`navigator.userAgent`; and every setting is gone on the next command — the
actor's `destroy()` runs `_restoreParentProcessConfiguration` when the
connection closes. Firefox renders print media with a light colour scheme, so
`prefers-color-scheme: dark` never matches under `printSimulationEnabled`;
`screenshot` refuses `--color-scheme dark --media print`.

`update_configuration` checks the echoed `configuration` for every key it sent
and fails naming a key that is missing or different, since Firefox drops
unsupported keys silently (see the iter-133 note below).

## History

Changes per-target ("page environment") settings without requiring
browser-wide pref changes. Obtained via
`WatcherFront::get_target_configuration_actor` (the watcher's
`getTargetConfigurationActor` method). Was consumed by the `ff-rdp emulate`
command (iter-103) and the `set_cache_disabled` cache-bypass path.

**2026-10 reset/structure-core update:** the `emulate` command no longer
exists in `ff-rdp-cli` (removed in an earlier reset phase) and nothing else
ever called `TargetConfigurationFront` outside its own unit tests and
`WatcherFront::get_target_configuration_actor`, which fed it and had the same
problem. Both types — along with `get_target_configuration_actor` — were
deleted from `crates/ff-rdp-core/src/fronts/` (now removed; see
[[watcher]]) as part of the `fronts/`→`actors/` collapse. This page is kept as
a historical record of the wire protocol, which is unchanged and could still
back a future `emulate`-equivalent command.

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

## Dead primitive removed (iter-133)

`TargetConfigurationFront::set_custom_viewport_size` sent
`{"customViewport": {"width", "height"}}` — a field **not** in
`SUPPORTED_OPTIONS` (see the table above). Empirically the server strips it
silently (echoes `{}`, `innerWidth` unchanged); a positive-control mixed
patch confirmed the echo faithfully reports accepted vs. dropped options.
Removed in iter-133 along with its unit test — see
[[viewport-emulation]] for the full empirical trace and RDM-internals proof
that viewport sizing is never an RDP wire feature (RDM does it via
parent-chrome CSS on a frame element it owns).

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
- [[viewport-emulation]] — iter-133 research spike proving no RDP actor sizes
  a viewport; grounds the dead-primitive removal above.
