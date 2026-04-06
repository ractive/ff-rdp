---
title: "Gradle Daemon Architecture Reference"
type: research
date: 2026-04-06
status: complete
tags: [research, gradle, daemon, architecture, reference]
---

# Gradle Daemon Architecture

Reference for ff-rdp daemon design. Gradle solves the same problem: expensive JVM startup amortized across builds via a persistent background daemon.

## Key Design Decisions

| Aspect | Gradle's Choice | ff-rdp Applicability |
|--------|----------------|---------------------|
| IPC transport | TCP loopback (127.0.0.1) — all platforms | Same — simplest cross-platform |
| Protocol | Custom binary (Java serialization), NOT a proxy | We use transparent proxy instead (simpler) |
| Discovery | Registry file (`~/.gradle/daemon/<version>/registry.bin`) with PID + port | Same pattern: `~/.ff-rdp/daemon-{host}-{port}.json` |
| Auto-start | First build auto-starts daemon | Same |
| Bypass flag | `--no-daemon` (spawns single-use daemon) | Same flag name |
| Idle timeout | 3 hours (configurable) | 5 minutes (our commands are fast) |
| Multiple instances | One per Gradle version + JVM config | One per Firefox host:port |
| Crash recovery | Client detects stale registry, cleans up | Same |
| Concurrency | One build at a time per daemon (serialized) | Same — one CLI client at a time |

## Why Gradle Uses a Custom Protocol (and We Don't Need To)

Gradle's daemon is the **build execution engine** — it holds the project model, task graph, and class cache in memory. The CLI sends a `Build` message with args, and the daemon does the work. This is fundamentally different from a proxy.

ff-rdp's daemon is a **transparent TCP proxy** — it forwards raw Firefox RDP frames. The CLI does all the work. This means:
- No command parsing in daemon
- No state management in daemon
- No protocol translation
- Zero changes to CLI command logic

This is possible because ff-rdp commands are stateless and fast (<100ms each), whereas Gradle builds are long-running and stateful.

## Sources

- [How Gradle Works Part 2 - Inside The Daemon](https://blog.gradle.org/how-gradle-works-2)
- [Gradle Daemon documentation](https://docs.gradle.org/current/userguide/gradle_daemon.html)
- [Formally document Gradle Daemon protocol (Issue #20586)](https://github.com/gradle/gradle/issues/20586)
