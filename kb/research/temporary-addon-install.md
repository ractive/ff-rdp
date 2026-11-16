---
title: Temporary add-on install over RDP
status: completed
created: 2026-11-16
tags:
  - research
  - rdp
  - addons
  - webextension
  - launch
type: research
---

# Temporary add-on install over RDP

Research for [[iteration-297-launch-temporary-addon]]: can `ff-rdp launch` load an
unsigned WebExtension into release Firefox, headless, the way about:debugging and
`web-ext run` do? Source citations are from the local mozilla-central checkout
(`/Users/james/devel/firefox`, 154.0a1, 2026-07-08); live results from Firefox
160.0 on macOS. Protocol reference: [[rdp/actors/addons]].

## Protocol

- The root actor's `getRoot` reply carries `addonsActor` (a global actor,
  `devtools/server/actors/utils/actor-registry.js:129-133`).
- Spec `devtools/shared/specs/addon/addons.js:12-31`:
  - `installTemporaryAddon { addonPath: string, openDevTools: nullable:boolean }`
    → `{ addon: json }`.
  - `uninstallAddon { addonId: string }` → `{}`.
- Server `devtools/server/actors/addon/addons.js`:
  - `addonPath` → `new FileUtils.File(addonPath)` (line 32): an absolute native
    path to a directory or `.xpi`.
  - Reply `addon` is `{ id, actor: false }` (line 75) — never a real actor.
  - Failure throws `Could not install add-on at '<path>': <error>` plus
    `error.additionalErrors` (lines 34-41). protocol.js sends it as
    `{error: "Error", message, fileName, lineNumber, columnNumber, stack}`
    (`devtools/shared/protocol/Actor.js:133-150`).
  - `uninstallAddon` only removes `temporarilyInstalled` add-ons (lines 78-88).
- History (`browser/config/version.txt` at each commit): actor re-added in Fx 70
  (bug 1578108, 2019-09-02); `openDevTools` Fx 106 (bugs 1787409, 1789245);
  `uninstallAddon` Fx 113 (bug 1823457).

Recorded replies: `crates/ff-rdp-core/tests/fixtures/install_temporary_addon_response.json`
and `install_temporary_addon_error_response.json` (manifest without `version`:
`Error: Extension is invalid\nReading manifest: Property "version" is required`).

## Prefs and signing

No pref beyond the devtools prefs `launch` already writes
(`devtools.debugger.remote-enabled`, `devtools.debugger.prompt-connection`,
`devtools.chrome.enabled`). `xpinstall.signatures.required` does not apply:
`XPIDatabase.sys.mjs:458-462` returns `isCorrectlySigned = true` for the
`KEY_APP_TEMPORARY` location. Verified live: the unsigned fixture installs on
release Firefox 160 with a fresh `launch` profile.

## MV3 host permissions

**Finding:** a temporarily installed MV3 extension gets its `host_permissions`
and content-script match origins granted at install, with no user action, on
Firefox 127+. Its `document_start` content script ran on the first page in
Firefox 160 (live test `live_297_directory_addon_marks_first_page`).

Why (source):
- MV3 extensions use origin controls: `originControls = manifestVersion >= 3`
  (`Extension.sys.mjs:2003`). Content-script `matches` count as manifest origins
  (`getManifestOrigins`, lines 1275-1290).
- On `startupReason === "ADDON_INSTALL"` (which a temporary install is), the
  origins are added to `ExtensionPermissions` when
  `manifest.granted_host_permissions` **or** `extensions.originControls.grantByDefault`
  is true (`Extension.sys.mjs:4411-4420`).
- `extensions.originControls.grantByDefault` defaults to `true`
  (`modules/libpref/init/all.js:3123`), added by bug 1889402 ("Grant origin
  permissions during install for mv3", 2024-05-03, Firefox 127.0a1).
- Temporary and privileged installs keep the `granted_host_permissions` manifest
  key (`Extension.sys.mjs:2018-2023`); for other installs it is forced to false.
  Before Firefox 127 that key was the only install-time grant (checked at
  124.0a1: `this.originControls && this.manifest.granted_host_permissions &&
  startupReason === "ADDON_INSTALL"`). So on Firefox 120-126 an MV3 extension
  needs `"granted_host_permissions": true` in its manifest for a temporary
  install to grant origins — source-derived, not live-tested (no old Firefox
  here).

`launch` therefore writes no extra pref and runs no privileged code to grant
permissions.

## Startup URL ordering

Firefox's startup URL loads before RDP is up, so an add-on installed afterwards
misses that document. With `--temporary-addon`, `launch` starts Firefox without
the URL, installs every add-on, then `navigateTo`s the selected tab. Live: the
first page carries the mark and `performance.getEntriesByType('navigation')[0].type`
is `navigate` (no reload); the local server saw one `GET /`.

`navigateTo` replies as soon as the load starts. Returning then handed the
caller a tab mid target-switch: a `dom` run right after `launch` failed 5 of 5
times (`noSuchActor` on the old console actor, `tabDestroyed` from the tab
descriptor, or a 10 s recv timeout). `launch` now uses the tab descriptor's
`navigateTo(url, waitForLoad: true)` (Firefox 140+, bug 1937738,
`devtools/server/actors/descriptors/tab.js:258-333`), which replies when the
load of the requested URL (matched by `originalURI`, so redirects count) stops.
On older Firefox it falls back to the WindowGlobalTarget `navigateTo` plus a
`listTabs` poll for the requested URL (or any URL other than the start and
`about:blank`). Afterwards 5 of 5 immediate `dom` calls succeeded. Every
navigation failure or timeout is a `results.warnings` entry, not a failed launch;
an error page (`deniedPortAccess`) never resolves `waitForLoad`, so it costs the
full `--timeout`.

## `data:` pages

A content script declared with only `matches: ["<all_urls>"]` did **not** run in a
top-level `data:` document (Firefox 160: `dom 'html[data-ff-rdp-temporary-addon]'
--count` → 0; the same launch then navigated to an `http://127.0.0.1` page → 1).
`data:` is in `<all_urls>`'s permitted schemes (`MatchPattern.cpp:83-85`), but a
top-level `data:` document has a null principal and is matched only as an opaque
document (`WebExtensionPolicy.cpp:1242-1268`, `match_origin_as_fallback`). Tests
serve pages over local `http:` instead.
