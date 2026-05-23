---
type: rdp-note
tags: [rdp, firefox-server, actor, descriptor]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/descriptors/tab.js
  - devtools/shared/specs/descriptors/tab.js
---

# TabDescriptorActor (typeName `"tabDescriptor"`)

Represents one Firefox tab (`<xul:browser>` or `<iframe mozbrowser>`). Returned by `RootActor.listTabs()` / `getTab()`.

- Source: `devtools/server/actors/descriptors/tab.js` (343 lines).
- Spec:   `devtools/shared/specs/descriptors/tab.js`.

## form()

```
{
  actor: <actorID>,
  browserId, browsingContextID, isZombieTab, outerWindowID, selected, title, url,
  traits: { watcher: true, supportsReloadDescriptor: true, supportsNavigation: true }
}
```

## Methods

| Method | Returns | Behavior |
|---|---|---|
| `getTarget()` | targetForm | Connect to the WindowGlobalTargetActor in the content process via `connectToFrame`. Returns the target's form. Stored as `targetActorForm`. |
| `getWatcher({isServerTargetSwitchingEnabled, isPopupDebuggingEnabled})` | watcher actor | Lazy-creates a [[rdp/actors/watcher]] with `BROWSER_ELEMENT` session context bound to this tab's browser. |
| `getFavicon()` | rawData (bytes) | PlacesUtils.favicons lookup, may return null. |
| `navigateTo(url, waitForLoad=true)` | promise | Installs an `nsIWebProgressListener` then `browsingContext.loadURI(uri, {triggeringPrincipal: systemPrincipal})`. Resolves on `STATE_STOP` (or `STATE_START` if `waitForLoad=false`). |
| `goBack()` / `goForward()` | — | `browsingContext.goBack/goForward()`. |
| `reloadDescriptor({bypassCache})` | — | `browsingContext.reload(LOAD_FLAGS_BYPASS_CACHE \| NONE)`. |

## Events

- `descriptor-destroyed` — fired on destroy.

## Lifecycle

- Created by RootActor's tabList; pooled in `_tabDescriptorActorPool`.
- On destroy, emits `descriptor-destroyed`, nulls `_browser`, supercalls.
- If the tab is destroyed mid-`getTarget`, the awaited promise rejects with `{error: "tabDestroyed"}` and the watcher is told `notifyTargetDestroyed(targetActorForm)`.

## Gotchas for ff-rdp

- **`navigateTo` matches the URL exactly via `originalURI.spec`**. If Firefox normalizes the URL (e.g. adds trailing `/`), the listener won't fire and the promise hangs. Always pre-normalize.
- **Targets created by `getTarget` are NOT created via JSWindowActors** — they use the legacy message-manager path. Watcher targets created by `watchTargets` use the new JSWindowActor path. The same WindowGlobal can have two co-existing target actors briefly during transition.
- `traits.supportsNavigation: true` — tab descriptor is the **only** descriptor that supports navigate.
- The triggering principal is `systemPrincipal` — `navigateTo` can load `chrome://` URLs (use with caution).
- Zombie tab (`isZombieTab`) means content hasn't loaded yet; calls to `getTarget` will fail.
