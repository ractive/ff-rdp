---
name: project_screenshot_firefox150
description: Firefox 150 screenshot regression root cause and fix — chrome-scope DevTools loader workaround
type: project
---

Firefox 150 Nightly introduced a regression where `screenshotActor.capture` fails because `capture-screenshot.js` uses `ChromeUtils.importESModule` without the `global` option required in DevTools distinct globals.

**Why:** `capture-screenshot.js` at `resource://devtools/server/actors/utils/capture-screenshot.js:8` calls `ChromeUtils.importESModule` without `{ global: "current" }`, which the DevTools distinct global now requires in Firefox 149+.

**Root cause of version detection failure:** Firefox 150 changed `getDescription` field names from camelCase (`appVersion`, `platformVersion`) to lowercase (`version`, `platformversion`). Fixed in `parse_app_version` in `device.rs`.

**Fix implemented (iter-61f branch):**
1. `device.rs`: `parse_app_version` now also tries lowercase `"version"` / `"platformversion"` fields (Firefox 150+).
2. `root.rs`: Added `RootActor::list_processes()` + `ProcessInfo` type.
3. `tab.rs`: Added `TabActor::get_process_target()` for process descriptor targets (response uses `"process"` key, not `"frame"`).
4. `screenshot.rs`: When `screenshotActor.capture` fails with the ESM global error, fall back to chrome-scope path:
   - `listProcesses` → find parent process
   - `getTarget` on process descriptor → get chrome-privileged consoleActor
   - `evaluateJSAsync` in chrome scope using `DevTools Loader.sys.mjs` to `require("devtools/server/actors/utils/capture-screenshot")`
   - Capture result written to temp file via `nsIFile` + `nsIBinaryOutputStream`
   - Rust polls temp file (50ms interval, 10s timeout) then reads PNG bytes

**How to apply:** When debugging screenshot failures on Firefox 149+, check for the string `"global option is required in DevTools distinct global"` in the error. If present, the chrome-scope workaround should be tried. The DevTools loader (Loader.sys.mjs) can `require()` modules that `ChromeUtils.importESModule` can't load in the DevTools distinct global.
