---
branch: iter-298/addon-command
date: 2026-11-16
depends_on:
  - iteration-297-launch-temporary-addon
status: done
type: iteration
title: "Iteration 298: ff-rdp addon install/list/reload/uninstall in a running Firefox"
tags:
  - iteration
  - addons
  - webextension
---

# Iteration 298: `ff-rdp addon`

`launch --temporary-addon` ([[iteration-297-launch-temporary-addon]]) loads an
unsigned extension only while starting Firefox. Reloading an extension in a
Firefox that keeps running keeps its tabs open, which is how tab-mcp found its
content script's exports turning into dead objects on update
([[research/extension-development-kept-profile]]); a relaunch cannot test that.

```
ff-rdp addon install ./dist
ff-rdp addon list
ff-rdp addon reload ./dist        # after a rebuild
ff-rdp addon uninstall my-ext@example.com
```

Protocol: [[rdp/actors/addons]], [[rdp/actors/descriptors/webextension-descriptor]],
[[rdp/actors/root]].

## Reload mechanism

about:debugging's Reload sends the WebExtension descriptor's `reload`
(`devtools/client/aboutdebugging/src/actions/debug-targets.js:161-175`,
`devtools/server/actors/descriptors/webextension.js:217-220`). For a temporary
add-on that runs `AddonManager.installTemporaryAddon(addon._sourceBundle)`
(`toolkit/mozapps/extensions/internal/XPIDatabase.sys.mjs:1505-1519`) — the same
call as `installTemporaryAddon` on the same path — which replaces the running
version through `BootstrapScope.update`, not an uninstall
(`XPIProvider.sys.mjs:2305-2311`, `2340-2391`). `addon reload` sends the
descriptor `reload`, found by id or by the add-on's `file:` source URL.

## Tasks [6/6]

- [x] Core: `RootActor::list_addons` (`listAddons`), `WebExtensionDescriptorActor::reload`
      (new spec `specs/webextension_descriptor.rs`), `AddonsActor::uninstall_addon`
      (`uninstallAddon`); replies recorded from Firefox 160 by
      `live_298_record_addon_commands`, including the unknown-id error.
- [x] CLI: `addon install|list|reload|uninstall` (`commands/addon.rs`); path checks and
      the install call shared with `launch --temporary-addon`, whose behaviour is unchanged.
- [x] `addon list` mirrors about:debugging's filter (debuggable, not hidden) with
      `--include-hidden` for everything; `addon reload` reports the UUID before and after.
- [x] Help: `addon` and each subcommand, the `keepUuidOnUninstall` /
      `keepStorageOnUninstall` prefs in `addon reload --help`; "(iter-297, …)" removed
      from `launch --help`; skill drift blessed.
- [x] Tests: unit tests per request/reply, e2e per subcommand and path error on the mock
      server, live tests `live_298_*`.
- [x] kb: addons actor, new WebExtension descriptor doc, root `listAddons`, research note
      triage, backlog line ticked.

## Acceptance criteria [8/8]

- [x] `ff-rdp addon install <dir>` into a running Firefox exits 0 and reports the
      absolute path and the add-on id; an `.xpi` path is accepted the same way.
- [x] A missing path, or a directory without `manifest.json`, fails with a `User` error
      before Firefox is contacted; a manifest Firefox rejects fails with Firefox's message.
- [x] `ff-rdp addon list` shows the installed temporary add-on with its id, name,
      `temporarily_installed: true`, UUID and `file:` source URL; `--jq` filters it.
- [x] After changing the add-on's content script, `ff-rdp addon reload <path>` (or `<id>`)
      exits 0, the open tab is still there with its document not reloaded, and a page
      loaded afterwards carries the new content script's mark.
- [x] `addon reload` keeps the add-on's UUID (Firefox 160, without any pref).
- [x] `ff-rdp addon uninstall <id>` removes the temporary add-on; an unknown id fails with
      a `User` error that says no add-on has that id.
- [x] Install → uninstall → install gives a new UUID without
      `extensions.webextensions.keepUuidOnUninstall` and the same UUID with it.
- [x] `launch --temporary-addon` still passes its live tests (`live_297_*`).
