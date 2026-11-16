---
type: rdp-note
tags:
- rdp
- firefox-server
- actor
- addons
- webextension
- launch-command
date: 2026-11-16
firefox_files:
- devtools/shared/specs/addon/addons.js
- devtools/server/actors/addon/addons.js
- devtools/server/actors/utils/actor-registry.js
- toolkit/mozapps/extensions/internal/XPIInstall.sys.mjs
title: AddonsActor
---

# AddonsActor (typeName `"addons"`)

Installs an unsigned WebExtension as a **temporary add-on** — the path
about:debugging, `web-ext run` and the VS Code Firefox plugin use
(`addons.js:21-22`, bug 1578108). Consumed by `ff-rdp launch --temporary-addon`
(iteration 297, [[iteration-297-launch-temporary-addon]]); research in
[[research/temporary-addon-install]].

## Firefox references

| File | Lines | Purpose |
|------|-------|---------|
| `devtools/shared/specs/addon/addons.js` | 12-31 | Spec — `installTemporaryAddon`, `uninstallAddon` |
| `devtools/server/actors/addon/addons.js` | 23-89 | Server implementation |
| `devtools/server/actors/utils/actor-registry.js` | 129-133 | Registered as a global actor, prefix `addons` |
| `toolkit/mozapps/extensions/internal/XPIInstall.sys.mjs` | 4827-4843 | `AddonManager.installTemporaryAddon` → temporary install location |

## Acquisition

A **global** actor: its id is the `addonsActor` field of the root actor's
`getRoot` reply (e.g. `server1.conn0.addonsActor2`). See [[root]].
ff-rdp reads it with `AddonsActor::from_root`
(`crates/ff-rdp-core/src/actors/addons.rs`).

## Methods

| Method | Args | Returns | Since | ff-rdp |
|---|---|---|---|---|
| `installTemporaryAddon` | `addonPath: string`, `openDevTools: nullable:boolean` | `{addon: json}` | actor re-added Fx 70 (bug 1578108); `openDevTools` Fx 106 (bugs 1787409, 1789245) | `AddonsActor::install_temporary_addon` |
| `uninstallAddon` | `addonId: string` | `{}` | Fx 113 (bug 1823457) | not mirrored (no caller; temporary add-ons die with Firefox) |

Versions are the `browser/config/version.txt` value at each bug's commit in
mozilla-central.

### `installTemporaryAddon`

Request (as ff-rdp sends it):

```
{"to": "<addonsActor>", "type": "installTemporaryAddon",
 "addonPath": "/abs/path/to/extension-or.xpi", "openDevTools": false}
```

- `addonPath` goes straight into `new FileUtils.File(addonPath)`
  (`addons.js:32`): it must be an **absolute native path** — a directory with
  `manifest.json` or an `.xpi`. On Windows, strip the `\\?\` prefix
  `canonicalize` adds.
- `openDevTools: true` opens a toolbox for the add-on (desktop only).

Success reply (recorded, Firefox 160 — `crates/ff-rdp-core/tests/fixtures/install_temporary_addon_response.json`):

```
{"addon": {"id": "temporary-addon-fixture@ff-rdp.test", "actor": false},
 "from": "server1.conn0.addonsActor2"}
```

`actor` is the literal `false` (`addons.js:68-75`: a pseudo add-on object; the
TODO to return a real actor was never done). An extension without a
`browser_specific_settings.gecko.id` gets a generated temporary id
(`XPIInstall.sys.mjs:732-734`).

Error reply (recorded, invalid manifest — `install_temporary_addon_error_response.json`):

```
{"from": "…addonsActor2", "error": "Error",
 "message": "Could not install add-on at '<path>': Error: Extension is invalid\nReading manifest: Property \"version\" is required",
 "fileName": "resource://devtools/server/actors/addon/addons.js", "lineNumber": 41, "columnNumber": 13,
 "stack": "installTemporaryAddon@resource://devtools/server/actors/addon/addons.js:41:13\n"}
```

`error` is the JS error's `name` (`devtools/shared/protocol/Actor.js:133-136`),
so `"Error"`, not `"unknownError"`. The message appends
`error.additionalErrors` (`addons.js:35-41`), which carries the concrete
manifest problem.

### `uninstallAddon`

Uninstalls only an add-on that is `temporarilyInstalled`; anything else throws
`Could not uninstall add-on "<id>"` (`addons.js:78-88`). Used by the fixture
recorder to leave the shared Firefox clean; no product caller.

## Gotchas

- No signing needed: `isCorrectlySigned` returns `true` for the temporary
  location (`XPIDatabase.sys.mjs:458-462`), so `xpinstall.signatures.required`
  does not matter.
- MV3 host permissions and content-script origins are granted at install while
  `extensions.originControls.grantByDefault` is `true` (the default) — see
  [[research/temporary-addon-install]].
- A temporary add-on is gone when Firefox exits; there is no persistence.
