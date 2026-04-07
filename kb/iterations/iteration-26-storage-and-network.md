---
title: "Iteration 26: Native StorageActor & Network Fallback"
type: iteration
status: completed
date: 2026-04-07
tags:
  - iteration
  - storage
  - cookies
  - network
  - protocol
branch: iter-26/storage-and-network
---

# Iteration 26: Native StorageActor & Network Fallback

Replace JS eval-based cookie/storage access with the native StorageActor protocol,
fixing the httpOnly cookie visibility gap. Add Performance API fallback for network.

## Notes

StorageActor protocol research requires a live Firefox instance. Launch headless Firefox for
discovery: `firefox -no-remote -profile /tmp/ff-rdp-test-profile --start-debugger-server 6000 --headless`

## Tasks

- [x] Implement StorageActor discovery via watcher or target actor
- [x] Implement `listStores` / `getStoreObjects` protocol for cookies
- [x] Migrate `ff-rdp cookies` to use StorageActor, exposing httpOnly/secure/sameSite flags
  → [[storage-actor-httponly-cookies]]
- [x] Add `network` command fallback to Performance API `getEntriesByType('resource')`
  when watcher returns no events (page already loaded)
  → [[network-empty-for-loaded-pages]]
- [ ] Optional: StorageActor for localStorage/sessionStorage
  (skipped — Firefox RDP does not expose local-storage/session-storage via watchResources; JS eval remains the correct approach)

## Test Fixtures

All e2e test fixtures must be recorded from a real Firefox instance — never hand-craft them.
Run with `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored` to record fixtures.
