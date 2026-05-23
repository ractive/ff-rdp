---
type: rdp-note
tags: [rdp, firefox-server, actor, root]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/root.js
  - devtools/shared/specs/root.js
---

# RootActor (typeName `"root"`)

The first actor a client speaks to after a successful handshake. Its actorID is fixed at `"root"`.

- Source: `devtools/server/actors/root.js`
- Spec: `devtools/shared/specs/root.js`

## sayHello (the greeting packet)

Not a regular RDP request — emitted spontaneously on connect. Returns:

```
{ from: "root", applicationType: "browser", testConnectionPrefix, traits: { networkMonitor, resources: {…}, supportsEnableWindowGlobalThreadActors, supportsCommentNodesDisplayControl, workerConsoleApiMessagesDispatchedToMainThread } }
```

`traits.resources` is the authoritative map of which resource types this build supports as **root** resources.

## Methods

| Method | Args | Returns | Notes |
|---|---|---|---|
| `connect` | `frontendVersion` (opt, Fx 133+) | `{}` | Negotiation handshake. |
| `getRoot` | — | json | Returns the global actor inventory (preferenceActor, deviceActor, perfActor, screenshotActor, …) for the top-level singletons. |
| `listTabs` | — | `array:tabDescriptor` | One TabDescriptor per browser tab. |
| `getTab` | `{browserId}` | `tabDescriptor` | Look up by stable browserId. |
| `listAddons` | `{iconDataURL}` | `array:webExtensionDescriptor` | |
| `listWorkers` | — | `{workers: array:workerDescriptor}` | Shared/dedicated/service workers. |
| `listServiceWorkerRegistrations` | — | `{registrations}` | |
| `listProcesses` | — | `array:processDescriptor` | One per content/parent process. |
| `getProcess` | `id` | `processDescriptor` | Id 0 = parent process (Browser Toolbox). |
| `watchResources` | `array:string` | `{}` | Root-scoped resources (e.g. extensions-backgroundscript-status). |
| `unwatchResources` | `array:string` | (oneway) | |
| `clearResources` | `array:string` | (oneway) | |
| `requestTypes` | — | json | Lists all known request-type names this actor handles. |

## Events

- `tabListChanged` — fires once after a `listTabs`, then suppressed until `listTabs` is called again.
- `workerListChanged`, `addonListChanged`, `serviceWorkerRegistrationListChanged`, `processListChanged`.
- `resources-available-array` / `resources-destroyed-array` — for root-scoped resources.

## Lifecycle

- Created by `devtools/server/startup/devtools-server.js` per connection.
- Owns LazyPool of global actors (perf, screenshot, preference, device, …).
- `destroy()` walks all sub-pools (tabDescriptor, processDescriptor, workerDescriptor, …) and tears them down.

## Gotchas

- The `*ListChanged` events only fire **once** per `list*` call — clients must re-call `listTabs()` after each event to re-arm.
- Most modern functionality is on the [[watcher]] reachable via `TabDescriptor.getWatcher` — RootActor is increasingly thin.
- ApplicationType "browser" is hard-coded; GeckoView uses a different startup path.
