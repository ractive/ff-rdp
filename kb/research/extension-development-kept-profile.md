---
title: Developing a WebExtension with ff-rdp and one kept profile
status: completed
created: 2026-11-16
tags:
  - research
  - addons
  - webextension
  - launch
  - profile
type: research
---

# Developing a WebExtension with ff-rdp and one kept profile

Field notes from developing tab-mcp for Firefox (ractive/tab-mcp, iteration 44) with
`ff-rdp launch --temporary-addon` ([[iteration-297-launch-temporary-addon]],
[[temporary-addon-install]]). Firefox 160.0 on macOS, ff-rdp 0.4.4. Meant as material for
the README or a skill: how to loop on an extension without starting from scratch each time.

## The problem

A temporary add-on is gone when Firefox quits. With a fresh temporary profile per launch
(ff-rdp's default) every launch is a new installation:

- a new UUID, so a new `moz-extension://<UUID>` origin (anything that pinned or paired with
  it, such as a server that allows the origin, sees a stranger);
- empty `storage.local`, so the extension's settings and state are gone;
- host access the user granted at run time (optional permissions) is gone;
- the profile's own state (sign-ins, test pages' storage) is gone.

## The setup

One profile directory kept between launches, with two prefs in its `user.js`:

```js
user_pref("extensions.webextensions.keepUuidOnUninstall", true);
user_pref("extensions.webextensions.keepStorageOnUninstall", true);
```

and each launch:

```sh
ff-rdp launch --profile ~/.cache/my-ext/firefox-profile --debug-port 6000 --replace \
  --temporary-addon ./dist [URL]
```

- `--profile` reuses the directory; ff-rdp appends its devtools and onboarding prefs to the
  `user.js` and leaves lines already there alone (seen: the two prefs above stayed first).
- `--replace` stops the Firefox running on that port first, so running the same command
  after a rebuild is the whole reload loop.
- An MV3 extension's `host_permissions` are granted at install (Firefox 127+,
  [[temporary-addon-install]]), so a development build that declares its host access there
  instead of in `optional_host_permissions` needs no click after any launch.

tab-mcp's version of it: `packages/tab-mcp-extension/scripts/firefox-dev.ts` (builds with
host access granted at install, creates the profile with the prefs, runs the launch above).

## What was checked

A probe extension: its background adds 1 to a counter in `storage.local` at every start;
its content script writes the counter and `runtime.getURL("")` into the page; read with
`ff-rdp eval` on <https://example.com> after each launch.

| Profile | Between the launches | Counter | UUID |
|---|---|---|---|
| kept, with the prefs | `launch --replace` | 1, then 2 | the same |
| kept, without the prefs | `launch --replace` | 1, then 2 | the same |
| kept, without the prefs | `ff-rdp stop`, then `launch` | 1, then 2 | the same |

So **with ff-rdp's own stop the prefs make no difference**: `stop` and `--replace` end
Firefox with SIGTERM (SIGKILL after 2 s), Firefox does not run the temporary add-on's
uninstall, and its UUID (`extensions.webextensions.uuids` in `prefs.js`) and storage
(`storage/default/moz-extension+++<UUID>`) stay in the profile.

The prefs matter when the add-on is really uninstalled:

- **Remove, then Load Temporary Add-on again** (a real uninstall): a new UUID without
  `keepUuidOnUninstall`, the same with it (ff-rdp `addon uninstall` + `install`, iter-298).
  Not **Reload**: an earlier version of this note said Reload gave a new UUID and empty
  storage without the prefs; measured, it keeps both (see "Reload keeps UUID and storage"
  below). The hand observation was most likely a Remove and Load.
- **WebDriver BiDi `webExtension.uninstall`, then `install` again**: with
  `keepUuidOnUninstall` the UUID stayed (tab-mcp's Firefox end-to-end test).
- **A normal quit** (closing the window, Cmd-Q) is expected to uninstall temporary add-ons
  the same way: not tested here, since ff-rdp has no way to quit Firefox normally and an
  `osascript` quit would hit the user's own Firefox. The prefs are the safety net for it.

Also seen: an extension that registers its content scripts at run time
(`scripting.registerContentScripts` in its background) had them in place for the URL given
to `launch`: launch installs first and navigates after, and the registration was done by
then (tab-mcp's `document_start` script defined `document.modelContext` on example.com's
first load).

## Reload keeps UUID and storage (tab-mcp, 2026-11-16)

ff-rdp 0.4.4 (94a73d11), Firefox 160, macOS, headless, a fresh profile **without** the two
prefs. A probe extension adds 1 at every background start to a counter in `storage.local`
and to one in IndexedDB; its content script writes both and `runtime.getURL("")` into
example.com.

| Step | `storage.local` | IndexedDB | UUID |
|---|---|---|---|
| `addon install` | 1 | 1 | `716c216b…` |
| `addon reload <id>` | 2 | 2 | the same (`previous_uuid` too) |

- tab-mcp itself, reloaded the same way: the same UUID. A page that had registered a WebMCP
  tool kept it with no page reload (the same `document.modelContext` object, the tool
  listed and run), which is the "dead object" case above, fixed in tab-mcp.
- So the prefs matter only for a real uninstall (Remove, BiDi `webExtension.uninstall`,
  presumably a normal quit), not for `addon reload`, `stop` or `--replace`.
- tab-mcp's `scripts/firefox-dev.ts --reload` uses it: rebuild, then `ff-rdp addon reload`.
- `ff-rdp navigate` refuses `moz-extension:` URLs (also with `--allow-unsafe-urls`), so an
  extension page's storage cannot be read through `eval`; a content script reporting into a
  web page, as above, works.

## Not checked

- Whether host access granted at run time (an optional permission the user allowed)
  survives a `stop` (SIGTERM) and relaunch. Granting needs a user gesture, which a headless
  launch cannot give. Declaring it in `host_permissions` for the development build avoids
  the question.

## Ideas for ff-rdp

- **Reload a temporary add-on in a running Firefox** (`ff-rdp addon reload PATH`, or
  `launch --temporary-addon` on an already running instance): `installTemporaryAddon` on the
  same path replaces it in place, as about:debugging's Reload does. That keeps the tabs open
  and tests what an extension update does to them: in tab-mcp it found that open pages lost
  their WebMCP tools, because the old content script's exported functions became dead
  objects ("can't access dead object" in the page).
- **A profile option for extension work**, e.g. `launch --keep-addon-state`, that writes
  the two prefs into the profile's `user.js`.
- **A normal quit** (`ff-rdp stop --graceful`, through `Services.startup.quit` in the
  parent process) for tests of what a real quit does.

## Triage (ff-rdp, 2026-11-16)

- **Reload in a running Firefox:** accepted, folded into the existing `addon install/list`
  backlog line as `addon reload`. The claim that `installTemporaryAddon` on the same path
  replaces in place is unverified; about:debugging may use the WebExtension descriptor's
  `reload` instead. Check that in Firefox source before building it.
- **`launch --keep-addon-state`:** declined. The table above shows the prefs change nothing
  under ff-rdp's own `stop` and `--replace`. They matter only after a real uninstall, which
  `addon reload` would cause, so they get documented with that command, not a flag.
- **`stop --graceful`:** declined for now. The only motivating case (a normal quit uninstalls
  temporary add-ons) is untested here, and the prefs above already cover it.
- **Reload checked against Firefox source (iter-298, [[iteration-298-addon-command]]):**
  about:debugging's Reload is the WebExtension descriptor's `reload`
  (`debug-targets.js:161-175`), which for a temporary add-on re-runs
  `AddonManager.installTemporaryAddon` on the add-on's own source
  (`XPIDatabase.sys.mjs:1505-1519`) — so both readings above were right, they are the same
  call. It is an upgrade, not an uninstall (`XPIProvider.sys.mjs:2305-2311`): on Firefox 160
  `ff-rdp addon reload` kept the UUID **without** the prefs, which contradicts the
  hand-observed "new UUID" for about:debugging's Reload above (not re-checked by hand; a
  Remove + Load would give exactly that). Storage across a reload: kept (measured, see
  "Reload keeps UUID and storage"). An
  uninstall followed by an install gave a new UUID without `keepUuidOnUninstall` and the
  same UUID with it. Details: [[rdp/actors/descriptors/webextension-descriptor]].
