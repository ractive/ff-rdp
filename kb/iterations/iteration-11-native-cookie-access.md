---
title: "Iteration 11: Native Cookie Access via StorageActor"
status: completed
branch: iter-11/native-cookies
date: 2026-04-06
tags:
  - iteration
  - cookies
  - storage-actor
---

# Iteration 11: Native Cookie Access via StorageActor

## Background

Protocol discovery (see [[research/storage-actor-protocol]]) revealed that cookies are accessed via the WatcherActor's `watchResources("cookies")` method, not through a direct actor on `getTarget`. This iteration replaces the JS-based cookie access with native StorageActor calls.

## Tasks

- [x] Protocol discovery: StorageActor via WatcherActor
- [x] Document findings in KB
- [x] Implement `StorageActor` in ff-rdp-core (watchResources, getStoreObjects, getFields)
- [x] Add `CookieInfo` struct with all protocol fields
- [x] Replace JS-based `cookies` CLI command with native StorageActor
- [x] Add `--name` filter for cookies
- [x] Record test fixtures from live Firefox
- [x] Add unit tests for StorageActor parsing
- [x] Add e2e tests for cookies command
- [x] Quality gates: fmt, clippy, test
- [x] Create PR
