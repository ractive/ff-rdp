---
type: rdp-note
tags:
- rdp
- firefox-server
- actor
- target-configuration
- emulate
date: 2026-07-09
updated: 2026-10-05
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
`specs::target_configuration`). They back these CLI flags, each applied on the
command's own connection:

| Flag | Wire field | Applied |
|------|------------|---------|
| `screenshot --color-scheme light\|dark` | `colorSchemeSimulation` | before the capture; the command polls `matchMedia` until it reports the scheme |
| `screenshot --media print` | `printSimulationEnabled: true` | same; `--media screen` sends nothing |
| `navigate --user-agent UA` | `customUserAgent` | before `navigateTo`, on the navigation watcher |
| `perf vitals\|summary\|audit --cold` | `cacheDisabled: true` | then a `reload` of the current page, on the navigation watcher (iter-295) |
| `perf compare --cold` | `cacheDisabled: true` | once, before the first `navigateTo`; covers every URL the command loads (iter-295) |

Verified live (Firefox 157, 2026-10-03): the dark/print styles are in the
captured pixels; the UA override reaches the document request's `User-Agent`
header (`HttpBaseChannel` reads `BrowsingContext.customUserAgent`) and
`navigator.userAgent`; and every setting is gone on the next command — the
actor's `destroy()` runs `_restoreParentProcessConfiguration` when the
connection closes. Firefox renders print media with a light colour scheme, so
`prefers-color-scheme: dark` never matches under `printSimulationEnabled`;
`screenshot` refuses `--color-scheme dark --media print`.

`cacheDisabled: true` sets the top browsing context's `defaultLoadFlags` to
`LOAD_BYPASS_CACHE` (`_setCacheDisabled`); child frames inherit it, and
`_restoreParentProcessConfiguration` puts back `LOAD_NORMAL` on disconnect. It
bypasses the HTTP cache only — service worker caches and warm TCP/TLS/DNS state
are untouched, which is why the CLI help says "bypass the HTTP cache" rather
than "first visit". Verified live (2026-10-05): after two plain loads of a page
with `max-age` subresources, `perf summary` reports `total_transfer_size: 0`,
`perf summary --cold` reports the full bytes, and a plain `navigate` + `perf
summary` on a new connection is back to 0.

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

## Configuration fields (wire names) — historical `emulate` mapping

The removed `emulate` command (iter-103, deleted in the 2026-10 reset) exposed
this subset; the "`emulate` flag" column is what it accepted, not a current
flag. The current users are listed under *Current use* above.

| Wire field (`SUPPORTED_OPTIONS`) | Type | `emulate` flag (removed) | Notes |
|----------------------------------|------|----------------|-------|
| `cacheDisabled` | bool | `--cache on\|off` | `--cache off` sent `cacheDisabled: true` (inverted); now `perf --cold` |
| `colorSchemeSimulation` | string | `--color-scheme light\|dark\|none` | `none` = system default; maps to `prefersColorSchemeOverride`; now `screenshot --color-scheme` |
| `customUserAgent` | string | `--user-agent <S>` | empty string restores the original UA; now `navigate --user-agent` |
| `overrideDPPX` | number | `--dppx <F>` | `0` clears the override |
| `printSimulationEnabled` | bool | `--print on\|off` | composed with `screenshot` for print audits; now `screenshot --media print` |
| `touchEventsOverride` | string | `--touch on\|off` | enum: `"enabled"` / `"none"` (not a bool) |
| `javascriptEnabled` | bool | `--js on\|off` | the server **reloads the document** when this changes |
| `setTabOffline` | bool | `--offline on\|off` | `navigator.onLine` / fetch failures; reload to reflect |

Fields present in the dict that `emulate` never exposed: `customFormatters`,
`rdmPaneOrientation`, `reloadOnTouchSimulationToggle`, `restoreFocus`,
`serviceWorkersTestingEnabled`, `isTracerFeatureEnabled`.

## Lifetime

Configuration lives as long as the RDP connection that set it. ff-rdp opens
one connection per command (the daemon is gone since the 2026-10 reset), so
every setting covers only the command that applied it. Under the old daemon it
lasted until the daemon restarted, and the `emulate` envelope carried a
`lifetime_warning` for one-shot `--no-daemon` runs.

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
