---
type: rdp-note
tags: [rdp, firefox-server, actor, performance, profiler]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/perf.js
  - devtools/shared/specs/perf.js
---

# PerfActor (typeName `"perf"`)

Wraps **the Gecko Profiler** (`Services.profiler` / `nsIProfiler`). Singleton on RootActor.

- Source: `devtools/server/actors/perf.js` (285 lines).
- Spec:   `devtools/shared/specs/perf.js`.

## Methods

- `startProfiler({entries, duration, interval, features, threads})` → bool.
  - Defaults: `entries=1_000_000`, `duration=0`, `interval=1`, `features=["js","stackwalk","cpu","memory"]`, `threads=["GeckoMain","Compositor"]`.
  - Sets `activeTabID = RecordingUtils.getActiveBrowserID()`.
- `stopProfilerAndDiscardProfile()` — drop without retrieval.
- `getProfileAndStopProfiler(debugPath?)` — legacy JSON profile path.
- `getProfileDataAsGzippedArrayBufferThenStop()` — modern bulk path; returns gzipped ArrayBuffer.
- `getPreviouslyRetrievedAdditionalInformation(handle)` — symbolication info.
- `isActive() → bool`.
- `getSupportedFeatures() → string[]`.
- `getAllFeatures()`, `getBufferUsageInPercent()`, `getCaptureHandle()`.

## Events

- `profiler-started`, `profiler-stopped` — relayed from `Services.obs`.

## Lifecycle

- Singleton from RootActor.getRoot().
- On unsupported platforms (`!"nsIProfiler" in Ci`) most methods no-op or return false.

## Gotchas

- The profiler is **process-wide**, not per-tab. Starting it captures *every* thread you asked for system-wide.
- For ff-rdp's perf-style needs (Lighthouse-like Web Vitals), use the **PerformanceObserver / Performance API via [[console]] `evaluateJSAsync`** — much lighter than Gecko Profiler.
- Profile data is huge (multi-MB gzipped). The bulk transfer uses the RDP bulk-data path, not regular JSON packets.
