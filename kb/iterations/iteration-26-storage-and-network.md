---
title: "Iteration 26: Native StorageActor & Network Fallback"
type: iteration
status: planned
date: 2026-04-07
tags: [iteration, storage, cookies, network, protocol]
branch: iter-26/storage-and-network
---

# Iteration 26: Native StorageActor & Network Fallback

Replace JS eval-based cookie/storage access with the native StorageActor protocol,
fixing the httpOnly cookie visibility gap. Add Performance API fallback for network.

## Tasks

- [ ] Implement StorageActor discovery via watcher or target actor
- [ ] Implement `listStores` / `getStoreObjects` protocol for cookies
- [ ] Migrate `ff-rdp cookies` to use StorageActor, exposing httpOnly/secure/sameSite flags
  → [[storage-actor-httponly-cookies]]
- [ ] Add `network` command fallback to Performance API `getEntriesByType('resource')`
  when watcher returns no events (page already loaded)
  → [[network-empty-for-loaded-pages]]
- [ ] Optional: StorageActor for localStorage/sessionStorage
