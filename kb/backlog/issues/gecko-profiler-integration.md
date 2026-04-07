---
title: "Gecko Profiler integration via PerfActor"
type: feature
status: open
priority: low
discovered: 2026-04-07
tags: [profiler, perf, protocol]
---

# Gecko Profiler integration via PerfActor

Firefox's PerfActor exposes the Gecko Profiler — CPU profiling with function-level
call stacks, layout/paint/GC markers, and network markers. Data that the Performance
API cannot provide.

## Proposed commands

```sh
ff-rdp profile start                        # start profiling
ff-rdp profile stop --save profile.json     # save full profile for profiler.firefox.com
ff-rdp profile stop                         # compact summary (stretch goal)
```

## Output challenge

Raw Gecko profiler dumps are tens of megabytes — call stacks, thousands of samples,
marker arrays. This is NOT LLM-friendly. The primary use case is saving the profile
to a file and opening it in profiler.firefox.com.

A CLI summary (top functions, marker counts, hot paths) would require parsing and
collapsing the full Gecko profiler format — significant implementation effort.

## Recommendation

- Phase 1: `profile start` / `profile stop --save` — capture and save. Simple to
  implement, immediately useful for developers. The LLM can say "I captured a
  profile, open profile.json in profiler.firefox.com to see the details."
- Phase 2 (stretch): `profile stop --summary` — top-N hot functions, marker
  breakdown. Only if there's demand.
