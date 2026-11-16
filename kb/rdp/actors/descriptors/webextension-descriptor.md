---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - actor
  - descriptor
  - addons
  - webextension
date: 2026-11-16
firefox_files:
  - devtools/shared/specs/descriptors/webextension.js
  - devtools/server/actors/descriptors/webextension.js
  - devtools/server/actors/webbrowser.js
  - devtools/client/aboutdebugging/src/actions/debug-targets.js
  - toolkit/mozapps/extensions/internal/XPIDatabase.sys.mjs
  - toolkit/mozapps/extensions/internal/XPIInstall.sys.mjs
  - toolkit/mozapps/extensions/internal/XPIProvider.sys.mjs
title: WebExtensionDescriptorActor
---

# WebExtensionDescriptorActor (typeName `"webExtensionDescriptor"`)

One per add-on the AddonManager knows. Returned by the root actor's
`listAddons` ([[root]]); consumed by `ff-rdp addon list` and `ff-rdp addon
reload` (iteration 298, [[iteration-298-addon-command]]).

- Spec: `devtools/shared/specs/descriptors/webextension.js:12-45`
- Server: `devtools/server/actors/descriptors/webextension.js:78-220`
- ff-rdp: `crates/ff-rdp-core/src/actors/webextension_descriptor.rs`,
  spec `crates/ff-rdp-core/src/specs/webextension_descriptor.rs`

## Acquisition: `listAddons`

`{"to": "root", "type": "listAddons"}` → `{"addons": [<form>, …]}`
(`root.js` spec lines 54-61). The server (`devtools/server/actors/root.js:357-385`)
asks `BrowserAddonList.getList()` (`webbrowser.js:688-699`) for **every**
add-on — `AddonManager.getAllAddons()`: system add-ons, built-in themes and GMP
plugins included — and keeps one descriptor per add-on id for the connection.
Each `listAddons` puts the descriptors in a fresh pool and destroys the previous
one, so descriptor ids are valid only on that connection.

`iconDataURL: true` (an `Option`, so a top-level packet field) makes the server
fetch each icon into a `data:` URL first (lines 372-374); ff-rdp leaves it out.
A root actor without add-on support throws `noAddons` (line 360).

After the first `listAddons` the root actor pushes `addonListChanged`
(`{"from": "root", "type": "addonListChanged"}`) on install/uninstall/reload.

about:debugging filters the list to `a.debuggable && (!a.hidden || showHiddenAddons)`
(`debug-targets.js:244-250`); `ff-rdp addon list` does the same unless
`--include-hidden`.

## form()

`webextension.js:89-124`. Fields whose value is `undefined` are absent from the
packet. Recorded (Firefox 160, `crates/ff-rdp-core/tests/fixtures/list_addons_response.json`):

```
{"actor": "server1.conn0.webExtensionDescriptor18", "debuggable": true, "hidden": false,
 "iconURL": null, "id": "temporary-addon-fixture@ff-rdp.test", "isSystem": false,
 "isWebExtension": true,
 "manifestURL": "moz-extension://6d369cf1-827f-4e2f-941b-8662090f636f/manifest.json",
 "name": "ff-rdp temporary add-on fixture", "temporarilyInstalled": true,
 "traits": {"supportsReloadDescriptor": true, "watcher": true},
 "url": "file://<FIXTURES>/temporary_addon_mv3/", "warnings": []}
```

- `manifestURL`'s host is the add-on's internal UUID (its `moz-extension://`
  origin); `null` when the extension has no active policy (built-in themes,
  GMP plugins).
- `url` is `addon.sourceURI`: the `file:` URL of the directory (trailing `/`)
  or `.xpi` a temporary add-on was installed from; absent for built-ins.
- GMP plugins (`gmp-gmpopenh264`, `gmp-widevinecdm`) carry no `debuggable`,
  `hidden`, `isSystem` or `temporarilyInstalled` at all.
- **No version field.** `addon list` cannot report one.

## Methods

| Method | Args | Returns | ff-rdp |
|---|---|---|---|
| `reload` | — | `{addon: json}` (server returns `{}`) | `WebExtensionDescriptorActor::reload` → `addon reload` |
| `reloadDescriptor` | `bypassCache` (ignored) | `{}` | not mirrored — calls `reload` (lines 213-215) |
| `terminateBackgroundScript` | — | `{}` | not mirrored |
| `getWatcher` | `isServerTargetSwitchingEnabled` | watcher | not mirrored |

### `reload` — what about:debugging's Reload button sends

`debug-targets.js:161-175`: `reloadTemporaryExtension(id)` finds the descriptor
front by id in `listAddons` (`devtools/client/fronts/root.js:270-274`) and calls
`reload()`. Server side (`webextension.js:217-220`) awaits
`AddonWrapper.reload()` (`XPIDatabase.sys.mjs:1505-1519`):

- **temporary add-on**: `AddonManager.installTemporaryAddon(addon._sourceBundle)` —
  the very call `installTemporaryAddon` on the same path makes. It loads the
  manifest first (`XPIInstall.sys.mjs:4833`) and then, because an add-on with
  that id is installed, goes through `BootstrapScope.update`
  (`XPIInstall.sys.mjs:4962-4974`, `XPIProvider.sys.mjs:2340-2391`): the old
  version shuts down with an upgrade/downgrade reason and the new one starts.
  For two WebExtensions `update` passes `callUpdate = true`, and
  `_uninstall` then skips the bootstrap `uninstall` call
  (`XPIProvider.sys.mjs:2305-2311`), so the extension's `uninstall` event
  (`Extension.sys.mjs:3284-3291`) and its cleanup (UUID, storage,
  permissions; `Extension.sys.mjs:520-600`) do not run.
- any other add-on: disabled and re-enabled.

So the descriptor `reload` and a second `installTemporaryAddon` on the same path
do the same thing for a temporary add-on. ff-rdp uses the descriptor `reload`
because it is what about:debugging does and it needs only the id.

Reply recorded (Firefox 160, `webextension_reload_response.json`):
`{"addon": {}, "from": "server1.conn0.webExtensionDescriptor18"}` — the spec's
`RetVal("json")` wraps the server's `{}`.

A manifest that no longer loads fails with only `Extension is invalid` (the
descriptor does not append `additionalErrors` as `installTemporaryAddon` does);
the running version stays installed, because the manifest is read before it is
touched. Observed in Firefox 160.

### Observed on Firefox 160 (iteration 298)

- The add-on's UUID is the same before and after `reload`, **without** the
  `keepUuidOnUninstall` pref (also after a second `installTemporaryAddon` on the
  same path). An `uninstallAddon` + `installTemporaryAddon` gives a new UUID
  unless `extensions.webextensions.keepUuidOnUninstall` is `true`.
- Open tabs stay open and their documents are not reloaded (a global set in the
  page before the reload is still there). Firefox injects the new version's
  `content_scripts` into the matching open page (the fixture's `document_start`
  mark changed from `v1` to `v2` in the old document) and into pages loaded
  afterwards.
