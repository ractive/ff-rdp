---
title: "Iteration 11: Native Cookie Access via StorageActor"
status: in-progress
branch: iter-11/native-cookies
date: 2026-04-06
tags: [iteration, cookies, storage-actor]
---

# Iteration 11: Native Cookie Access via StorageActor

## Background

Protocol discovery (see [[storage-actor-protocol]]) revealed that cookies are accessed via the WatcherActor's `watchResources("cookies")` method, not through a direct actor on `getTarget`. This iteration replaces the JS-based cookie access with native StorageActor calls.

## Tasks

- [x] Protocol discovery: StorageActor via WatcherActor
- [x] Document findings in KB
- [ ] Implement `StorageActor` in ff-rdp-core (watchResources, getStoreObjects, getFields)
- [ ] Add `CookieInfo` struct with all protocol fields
- [ ] Replace JS-based `cookies` CLI command with native StorageActor
- [ ] Add `--name` filter for cookies
- [ ] Record test fixtures from live Firefox
- [ ] Add unit tests for StorageActor parsing
- [ ] Add e2e tests for cookies command
- [ ] Quality gates: fmt, clippy, test
- [ ] Create PR
