---
type: rdp-note
tags:
  - rdp
  - firefox-server
  - actor
  - descriptor
  - worker
date: 2026-05-23
firefox_files:
  - devtools/server/actors/descriptors/worker.js
  - devtools/shared/specs/descriptors/worker.js
title: WorkerDescriptorActor
---

# WorkerDescriptorActor (typeName `"workerDescriptor"`)

Represents a dedicated worker, shared worker, or service worker. Returned by `RootActor.listWorkers()`.

- Source: `devtools/server/actors/descriptors/worker.js` (192 lines).
- Spec:   `devtools/shared/specs/descriptors/worker.js`.

## form fields

- `actor`, `id`, `url`, `type` (`0=dedicated`, `1=shared`, `2=service`), `name`, `fetch` (service worker), `traits`.

## Methods

- `getTarget()` — attaches a WorkerTargetActor inside the worker thread (via `attachWorker` IPC). Returns the target form.
- For service workers: also exposes `push`, `start`, `unregister` style controls (some on the ServiceWorkerRegistrationActor sibling).

## Lifecycle

- A worker that terminates fires `workerListChanged` on RootActor, but the descriptor sticks around until next `listWorkers()` call refreshes.
- Service workers can be in different states (parsed, installing, installed, activating, activated, redundant) — surface via `state` field.

## Gotchas

- Workers have a stripped-down [[rdp/actors/console]] (worker-listeners only).
- No DOM walker — workers have no document.
- Targeting service workers across navigations is tricky: SW lifecycle is independent of the page's, so worker descriptor lifetime ≠ tab lifetime.
