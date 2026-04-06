---
title: W3C Performance API & Core Web Vitals Research
status: complete
date: 2026-04-06
tags: [performance, cwv, web-vitals, lighthouse, api-design]
related: [[firefox-rdp-protocol]]
---

# W3C Performance API & Core Web Vitals

Research for designing a CLI tool that computes Core Web Vitals via JavaScript evaluation in a browser (Firefox RDP `evaluateJSAsync`).

## 1. Core Web Vitals Metrics

### The Three Core Web Vitals (Google's official set, used in Search ranking)

| Metric | Full Name | What It Measures | Unit | Good | Needs Improvement | Poor |
|--------|-----------|------------------|------|------|-------------------|------|
| **LCP** | Largest Contentful Paint | Loading performance - when the largest visible content element finishes rendering | ms | <= 2500 | <= 4000 | > 4000 |
| **CLS** | Cumulative Layout Shift | Visual stability - sum of unexpected layout shift scores | unitless score | <= 0.1 | <= 0.25 | > 0.25 |
| **INP** | Interaction to Next Paint | Responsiveness - latency of the worst interaction (p98) | ms | <= 200 | <= 500 | > 500 |

### Additional Important Metrics (not CWV but commonly reported)

| Metric | Full Name | What It Measures | Unit | Good | Needs Improvement | Poor |
|--------|-----------|------------------|------|------|-------------------|------|
| **FCP** | First Contentful Paint | When first text/image is painted | ms | <= 1800 | <= 3000 | > 3000 |
| **TTFB** | Time to First Byte | Server responsiveness - time from request to first byte | ms | <= 800 | <= 1800 | > 1800 |

## 2. JavaScript APIs That Expose These Metrics

### 2.1 `performance.getEntriesByType(type)`

Synchronous API that returns all buffered entries of a given type. Suitable for one-shot CLI measurement after page load.

Available entry types and their use:

| Entry Type | Interface | Used For |
|------------|-----------|----------|
| `"navigation"` | `PerformanceNavigationTiming` | TTFB, full page timing breakdown |
| `"resource"` | `PerformanceResourceTiming` | Individual resource load timings |
| `"paint"` | `PerformancePaintTiming` | FCP (entry name = `"first-contentful-paint"`) |
| `"largest-contentful-paint"` | `LargestContentfulPaint` | LCP |
| `"layout-shift"` | `LayoutShift` | CLS |
| `"longtask"` | `PerformanceLongTaskTiming` | Long tasks (>50ms) blocking main thread |
| `"event"` | `PerformanceEventTiming` | INP (interaction latency) |
| `"first-input"` | `PerformanceEventTiming` | First Input Delay (legacy, subsumed by INP) |
| `"element"` | `PerformanceElementTiming` | Custom element timing (requires `elementtiming` attr) |
| `"mark"` | `PerformanceMark` | Custom user marks |
| `"measure"` | `PerformanceMeasure` | Custom user measures |
| `"long-animation-frame"` | `PerformanceLongAnimationFrameTiming` | LoAF entries (Chrome 123+) |

**Important**: `getEntriesByType()` only returns entries that were buffered. Some entry types require a `PerformanceObserver` with `buffered: true` to capture all entries. For a CLI tool doing one-shot measurement after load, we need to use `PerformanceObserver` with `buffered: true` to ensure we get entries that may have been emitted before our script runs.

### 2.2 `PerformanceObserver`

Asynchronous API. Required for some entry types. Pattern:

```javascript
new PerformanceObserver((list) => {
  for (const entry of list.getEntries()) { /* process */ }
}).observe({ type: 'largest-contentful-paint', buffered: true });
```

The `buffered: true` flag retrieves entries emitted before the observer was created -- critical for CLI injection after page load.

### 2.3 `PerformanceObserver.supportedEntryTypes`

Returns an array of supported entry types. Useful for feature detection:

```javascript
PerformanceObserver.supportedEntryTypes
// Chrome: ["element", "event", "first-input", "largest-contentful-paint", "layout-shift", "long-animation-frame", "longtask", "mark", "measure", "navigation", "paint", "resource", "visibility-state"]
// Firefox: ["element", "event", "first-input", "largest-contentful-paint", "layout-shift", "longtask", "mark", "measure", "navigation", "paint", "resource"]
```

## 3. Entry Type Field Reference

### 3.1 `PerformanceNavigationTiming` (type: `"navigation"`)

Used for TTFB and full page timing waterfall.

```
startTime: 0                        // Always 0 for navigation
redirectStart / redirectEnd          // Redirect chain timing
fetchStart                           // When fetch begins (after service worker, cache)
domainLookupStart / domainLookupEnd  // DNS
connectStart / connectEnd            // TCP
secureConnectionStart                // TLS handshake start
requestStart                         // HTTP request sent
responseStart                        // First byte received (TTFB = responseStart - startTime)
responseEnd                          // Last byte received
domInteractive                       // DOM parsing complete
domContentLoadedEventStart / End     // DOMContentLoaded event
domComplete                          // All resources loaded
loadEventStart / loadEventEnd        // load event
duration                             // loadEventEnd - startTime
transferSize                         // Compressed bytes over network
encodedBodySize / decodedBodySize    // Resource sizes
type                                 // "navigate" | "reload" | "back_forward" | "prerender"
activationStart                      // Prerender activation time (0 if not prerendered)
```

**TTFB computation**: `entry.responseStart - entry.activationStart` (clamped to >= 0).

### 3.2 `PerformanceResourceTiming` (type: `"resource"`)

Same timing fields as navigation (DNS, TCP, TLS, request, response) plus:

```
name                    // URL of the resource
initiatorType           // "script", "link", "img", "css", "fetch", "xmlhttprequest", etc.
nextHopProtocol         // "h2", "h3", "http/1.1", etc.
transferSize            // 0 if cached, otherwise compressed size + headers
encodedBodySize         // Compressed body size
decodedBodySize         // Decompressed body size
renderBlockingStatus    // "blocking" | "non-blocking"
responseStatus          // HTTP status code (200, 404, etc.)
serverTiming            // Server-Timing header values
```

### 3.3 `PerformancePaintTiming` (type: `"paint"`)

Minimal entry. Two entries are emitted:
- `name: "first-paint"` -- when any pixel is first rendered
- `name: "first-contentful-paint"` -- when first text/image is rendered

Fields: `name`, `entryType`, `startTime`, `duration` (always 0).

**FCP computation**: find entry where `name === "first-contentful-paint"`, use `startTime - activationStart` (from navigation entry).

### 3.4 `LargestContentfulPaint` (type: `"largest-contentful-paint"`)

Multiple entries emitted as larger elements render. The last entry before user input is the LCP.

```
startTime       // renderTime if available, else loadTime
renderTime      // When element was painted (0 if cross-origin without TAO)
loadTime        // When resource finished loading
size            // Area of the element in pixels
id              // Element's id attribute
url             // URL of the image resource (empty for text)
element         // The DOM element (null after GC)
```

**LCP computation**: take the last entry's `startTime - activationStart` (clamped to >= 0). Must be before `visibilitychange` to `hidden`.

### 3.5 `LayoutShift` (type: `"layout-shift"`)

Emitted for every layout shift, including user-initiated ones.

```
startTime           // When the shift occurred
value               // Layout shift score for this shift (fractional)
hadRecentInput      // true if triggered by user input (exclude these!)
sources             // Array of LayoutShiftAttribution:
  - node            //   The shifted DOM node
  - previousRect    //   DOMRect before shift
  - currentRect     //   DOMRect after shift
```

**CLS computation** (session window algorithm):
1. Filter out entries where `hadRecentInput === true`
2. Group into session windows: consecutive shifts < 1s apart, total window < 5s
3. Sum `value` within each window
4. CLS = the maximum session window sum

### 3.6 `PerformanceEventTiming` (type: `"event"`)

Emitted for events with duration > 104ms (or custom `durationThreshold`, min 16ms).

```
name            // Event type: "pointerdown", "click", "keydown", etc.
startTime       // Event timestamp
duration        // Total event duration (quantized to 8ms)
processingStart // When event handler began
processingEnd   // When event handler finished
interactionId   // Groups related events (e.g., keydown+keyup = one interaction)
cancelable      // Whether event is cancelable
```

**INP computation**:
1. Group entries by `interactionId` (non-zero IDs only, plus `first-input`)
2. For each interaction, take the max `duration` among its entries
3. Keep the 10 longest interactions
4. INP = estimated p98 = `longestInteractions[Math.floor(interactionCount / 50)]`
   - If < 10 interactions stored, use the last (shortest of the longest)
   - This approximates the 98th percentile

### 3.7 `PerformanceLongTaskTiming` (type: `"longtask"`)

```
startTime       // When the long task started
duration        // Task duration (>50ms threshold)
name            // Usually "self"
attribution     // Array with container info (iframe, etc.)
```

### 3.8 `PerformanceLongAnimationFrameTiming` (type: `"long-animation-frame"`)

Chrome 123+. More detailed than longtask.

```
startTime                   // Frame start
duration                    // Total frame duration
renderStart                 // When rendering began
styleAndLayoutStart         // When style/layout recalc began
blockingDuration            // Time the frame was blocked
firstUIEventTimestamp       // First UI event in this frame
scripts                     // Array of PerformanceScriptTiming entries:
  - invokerType             //   "classic-script" | "module-script" | "event-listener" | "user-callback" | "resolve-promise" | "reject-promise"
  - invoker                 //   Identifier of the invoker
  - executionStart          //   Script execution start
  - sourceURL               //   Script URL
  - sourceFunctionName      //   Function name
  - sourceCharPosition      //   Character position in source
  - pauseDuration           //   Time paused (e.g., for sync XHR)
  - forcedStyleAndLayoutDuration  // Forced reflow time
```

## 4. How Lighthouse Computes Performance Scores

### 4.1 Metric Weights (Lighthouse 12+, navigation mode)

| Metric | Weight | p10 (mobile) | Median (mobile) | p10 (desktop) | Median (desktop) |
|--------|--------|--------------|-----------------|---------------|------------------|
| FCP | 10% | 1800ms | 3000ms | 934ms | 1600ms |
| LCP | 25% | 2500ms | 4000ms | 1200ms | 2400ms |
| TBT | 30% | 200ms | 600ms | 150ms | 350ms |
| CLS | 25% | 0.1 | 0.25 | 0.1 | 0.25 |
| SI | 10% | 3387ms | 5800ms | 1311ms | 2300ms |
| INP | 0% (displayed but not scored) | 200ms | 500ms | 200ms | 500ms |

Note: Lighthouse uses **TBT** (Total Blocking Time) as a lab proxy for INP since INP requires real user interaction. TBT sums the blocking portion of all long tasks (duration - 50ms for each task > 50ms).

### 4.2 Log-Normal Scoring Function

Each metric is scored using a log-normal distribution with two control points:
- **p10**: the value at which the score is 0.9 (passing threshold)
- **median**: the value at which the score is 0.5

The algorithm:
1. Compute the shape parameter from `p10` and `median`
2. Calculate the complementary percentile using the error function approximation
3. Clamp to score ranges: [0.9, 1.0] for passing, [0.5, 0.9) for average, [0, 0.5) for failing
4. Apply a slight boost for scores > 0.9 (expands top scores so more reach perfect 1.0)
5. Floor to 2 decimal places

```
score = floor(percentile * 100) / 100

where percentile = (1 - erf(x)) / 2
and x = ln(value/median) * INVERSE_ERFC_ONE_FIFTH / (-ln(p10/median))
```

### 4.3 Overall Performance Score

Weighted arithmetic mean of individual metric scores:

```
overallScore = sum(metricScore[i] * weight[i]) / sum(weight[i])
```

## 5. Practical JavaScript Snippets for CLI Evaluation

### 5.1 One-Shot CWV Collection (inject after page load)

```javascript
(function() {
  const result = { supported: PerformanceObserver.supportedEntryTypes || [] };

  // TTFB + Navigation timing
  const nav = performance.getEntriesByType('navigation')[0];
  if (nav) {
    const activation = nav.activationStart || 0;
    result.ttfb = Math.max(nav.responseStart - activation, 0);
    result.navigation = {
      dns: nav.domainLookupEnd - nav.domainLookupStart,
      tcp: nav.connectEnd - nav.connectStart,
      tls: nav.secureConnectionStart > 0 ? nav.connectEnd - nav.secureConnectionStart : 0,
      request: nav.responseStart - nav.requestStart,
      response: nav.responseEnd - nav.responseStart,
      domInteractive: nav.domInteractive,
      domContentLoaded: nav.domContentLoadedEventEnd,
      domComplete: nav.domComplete,
      load: nav.loadEventEnd,
      transferSize: nav.transferSize,
      protocol: nav.nextHopProtocol,
      type: nav.type,
    };
  }

  // FCP
  const paintEntries = performance.getEntriesByType('paint');
  const fcp = paintEntries.find(e => e.name === 'first-contentful-paint');
  if (fcp) {
    const activation = nav ? (nav.activationStart || 0) : 0;
    result.fcp = Math.max(fcp.startTime - activation, 0);
  }

  return JSON.stringify(result);
})()
```

### 5.2 LCP via PerformanceObserver (buffered)

```javascript
new Promise((resolve) => {
  let lcpValue = 0;
  const activation = (performance.getEntriesByType('navigation')[0] || {}).activationStart || 0;
  new PerformanceObserver((list) => {
    for (const entry of list.getEntries()) {
      lcpValue = Math.max(entry.startTime - activation, 0);
    }
  }).observe({ type: 'largest-contentful-paint', buffered: true });
  // LCP finalizes on user input or visibility change; for CLI, read after a frame
  requestAnimationFrame(() => requestAnimationFrame(() => resolve(lcpValue)));
})
```

### 5.3 CLS via Session Windows

```javascript
new Promise((resolve) => {
  let sessionValue = 0, sessionEntries = [], maxSessionValue = 0;
  new PerformanceObserver((list) => {
    for (const entry of list.getEntries()) {
      if (entry.hadRecentInput) continue;
      const last = sessionEntries.at(-1);
      const first = sessionEntries[0];
      if (sessionValue && last && first &&
          entry.startTime - last.startTime < 1000 &&
          entry.startTime - first.startTime < 5000) {
        sessionValue += entry.value;
        sessionEntries.push(entry);
      } else {
        sessionValue = entry.value;
        sessionEntries = [entry];
      }
      maxSessionValue = Math.max(maxSessionValue, sessionValue);
    }
  }).observe({ type: 'layout-shift', buffered: true });
  requestAnimationFrame(() => requestAnimationFrame(() => resolve(maxSessionValue)));
})
```

### 5.4 Resource Timing Summary

```javascript
JSON.stringify(performance.getEntriesByType('resource').map(e => ({
  name: e.name,
  initiator: e.initiatorType,
  protocol: e.nextHopProtocol,
  transferSize: e.transferSize,
  decodedSize: e.decodedBodySize,
  duration: Math.round(e.duration),
  blocking: e.renderBlockingStatus,
  status: e.responseStatus,
})))
```

## 6. Firefox-Specific Considerations

1. **Firefox supports** most Performance API entry types: `navigation`, `resource`, `paint`, `largest-contentful-paint`, `layout-shift`, `longtask`, `event`, `first-input`, `mark`, `measure`, `element`.
2. **Firefox does NOT support**: `long-animation-frame` (Chrome-only), `visibility-state` (Chrome-only).
3. **INP support in Firefox**: Firefox added `PerformanceEventTiming` with `interactionId` support. Check `PerformanceObserver.supportedEntryTypes` at runtime.
4. **LCP cross-origin**: `renderTime` is 0 for cross-origin images without `Timing-Allow-Origin` header. In that case `loadTime` is used via `startTime`.
5. **Resource Timing**: `responseStatus` and `renderBlockingStatus` may not be available in older Firefox versions.

## 7. Design Implications for CLI Tool

### Recommended approach

1. **Navigate** to the URL via RDP
2. **Wait** for page load (listen for `DOMContentLoaded` and/or network idle)
3. **Inject** a single JavaScript snippet via `evaluateJSAsync` that:
   - Creates `PerformanceObserver` instances with `buffered: true` for all needed types
   - Collects entries synchronously from `getEntriesByType()` where possible
   - Uses `requestAnimationFrame` double-buffering to ensure final LCP/CLS values
   - Returns a structured JSON object with all metrics
4. **Compute** ratings client-side (Rust) using the threshold tables
5. **Optionally** compute a Lighthouse-style weighted score using the log-normal function

### Suggested output structure

```json
{
  "url": "https://example.com",
  "metrics": {
    "ttfb": { "value": 245, "rating": "good" },
    "fcp": { "value": 1200, "rating": "good" },
    "lcp": { "value": 2100, "rating": "good" },
    "cls": { "value": 0.05, "rating": "good" },
    "inp": null,
    "tbt": { "value": 150, "rating": "good" }
  },
  "navigation": {
    "dns": 15,
    "tcp": 30,
    "tls": 25,
    "request": 45,
    "response": 130,
    "dom_interactive": 800,
    "dom_content_loaded": 900,
    "dom_complete": 1500,
    "load": 1550,
    "transfer_size": 45000,
    "protocol": "h2",
    "type": "navigate"
  },
  "resources": [ ... ],
  "score": 92
}
```

### Rating function (Rust implementation)

```rust
fn rate(value: f64, good_threshold: f64, poor_threshold: f64) -> &'static str {
    if value <= good_threshold { "good" }
    else if value <= poor_threshold { "needs-improvement" }
    else { "poor" }
}
```

### INP limitation in lab/CLI context

INP requires real user interactions. In a headless CLI tool, there are no interactions, so INP will always be `null`. Options:
- Report it as N/A for lab measurements
- Simulate a click via RDP and measure that single interaction
- Use TBT as the lab proxy (like Lighthouse does)

### TBT computation (lab proxy for responsiveness)

```javascript
performance.getEntriesByType('longtask')
  .reduce((sum, entry) => sum + Math.max(0, entry.duration - 50), 0)
```

TBT sums the "blocking" portion (> 50ms) of every long task between FCP and TTI (or page load end for simplicity).
