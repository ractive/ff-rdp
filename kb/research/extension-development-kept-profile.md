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
- `ff-rdp navigate` refused `moz-extension:` URLs (also with `--allow-unsafe-urls`), so an
  extension page's storage could not be read through `eval`; a content script reporting into
  a web page, as above, worked. Since backlog/navigate-moz-extension it is allowed, see
  "Opening an extension page" below.

## Opening an extension page (ff-rdp, 2026-11-16)

`ff-rdp navigate moz-extension://<uuid>/page.html` is allowed by default since
backlog/navigate-moz-extension. Checked first because content-tab navigation to `file:` and
`about:support` crashes Firefox.

**How the load happens.** `navigate` sends the `WindowGlobalTarget`'s `navigateTo`, which sets
`this.window.location = request.url` from the DevTools server in the tab's content process
(`devtools/server/actors/targets/window-global.js:1418`, mozilla-firefox/firefox `f612f4a7`), so
the triggering principal is the DevTools code's (system), not the page's — inferred, not traced:
the page below is not web-accessible, which a page-triggered load would refuse.
(`launch --temporary-addon --url` uses the tab descriptor's `navigateTo` instead:
`browsingContext.loadURI` from the parent with the system principal, `descriptors/tab.js:325`.)

**Why `file:` crashes and `moz-extension:` does not.** A load requested by a content process is
checked in the parent (`nsDocShellLoadState.cpp`, `ContentTriggeredURILoadIsAllowed`, line 55);
a failure is an IPC `FatalError` ("Illegal load attempt of %s URL from %s"), which is the
`file:` crash. For a URI a null principal may not link to, the check falls through to
`ValidatePrincipalCouldPotentiallyBeLoadedBy` (`dom/ipc/ProcessIsolation.cpp:1496`), which
returns true for any `moz-extension` origin from any content process ("Web content can contain
extension content frames and contain extension content scripts, so any content process may
send us an extension's principal", line 1611), while `file:` is only allowed for the file
process (line 1633).

**`web_accessible_resources`.** Not needed: the fixture's `page.html` is not listed and loads.
The privileged triggering principal passes `CheckLoadURI`; a web page could not do this load.

**Measured** (Firefox 160.0, macOS, headless, the MV3 fixture with `page.html` and the
`storage` permission, a local `http:` page):

| Step | Result |
|---|---|
| `navigate moz-extension://<uuid>/page.html` | `committed_url` the extension page, `ready_state: complete`, `status: null` (`no_document_request`) |
| `tabs` | same tab, new `browsingContextID` (11 → 13): the tab changed process |
| `eval --stringify '[location.protocol, typeof browser.runtime.id, browser.runtime.id]'` | `["moz-extension:", "string", "temporary-addon-fixture@ff-rdp.test"]` |
| `eval --stringify 'await browser.storage.local.set({…}); await browser.storage.local.get()'` | the stored object |
| `dom '#extension-page' --text` | the page's heading |
| `console` | the page script's `console.log`, source `moz-extension://<uuid>/page.js` |
| `navigate moz-extension://<uuid>/missing.html` | commits, no crash |
| `navigate http://127.0.0.1:…/`, then `eval`, `dom` | work; the content script's mark is set |
| a second round trip | the same |

Firefox stayed up through all of it. Live test: `live_moz_extension_navigate.rs`.

**Why by default, not behind a flag.** It reaches only add-ons installed in the Firefox being
driven (random per-profile UUID host), and a client on the debug port can already read
everything in that browser, extension storage included, so nothing new is disclosed.

Not checked: Windows and Linux (the policy check above is platform-independent); an add-on
page opened while its add-on is reloaded or uninstalled.

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
