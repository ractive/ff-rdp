use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use base64::Engine as _;
use ff_rdp_core::{
    COMPATIBLE_FIREFOX_MIN, Grip, LongStringActor, ProcessInfo, ProtocolError, RootActor,
    ScreenshotActor, ScreenshotContentActor, TabActor, WebConsoleActor,
};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_direct;
use super::js_helpers::eval_or_bail;

/// Build the JavaScript injected into the page to capture a screenshot.
///
/// Uses `CanvasRenderingContext2D.drawWindow`, a Firefox-specific privileged
/// API that renders a region starting at `(0, 0)` into an offscreen canvas.
/// Returns a `data:image/png;base64,...` string on success, or `null` when
/// `drawWindow` is not available (removed in some Firefox configurations).
///
/// `height_override` selects the capture height:
/// - `None` (viewport, default): `window.innerHeight`.
/// - `Some(HeightOverride::FullPage)`: `document.scrollingElement.scrollHeight`.
/// - `Some(HeightOverride::Explicit(n))`: the literal value `n`.
fn build_screenshot_js(height_override: Option<HeightOverride>) -> String {
    let height_expr = match height_override {
        None => "window.innerHeight || document.documentElement.clientHeight || 600".to_owned(),
        Some(HeightOverride::FullPage) => {
            // Take the max of scrollHeight candidates — different browsers
            // populate different properties.
            "Math.max(\
              document.documentElement.scrollHeight,\
              document.body ? document.body.scrollHeight : 0,\
              (document.scrollingElement && document.scrollingElement.scrollHeight) || 0,\
              window.innerHeight || 0\
            )"
            .to_owned()
        }
        Some(HeightOverride::Explicit(n)) => n.to_string(),
    };
    format!(
        "(function() {{\n  var w = window.innerWidth || document.documentElement.clientWidth || 800;\n  var h = {height_expr};\n  var canvas = document.createElement('canvas');\n  canvas.width = w;\n  canvas.height = h;\n  var ctx = canvas.getContext('2d');\n  if (!ctx || typeof ctx.drawWindow !== 'function') {{ return null; }}\n  ctx.drawWindow(window, 0, 0, w, h, 'rgb(255,255,255)');\n  return canvas.toDataURL('image/png');\n}})()"
    )
}

/// Height mode selected by the user.
#[derive(Copy, Clone, Debug)]
enum HeightOverride {
    FullPage,
    Explicit(u32),
}

/// Options accepted by [`run`].
pub(crate) struct ScreenshotOpts<'a> {
    pub(crate) output_path: Option<&'a str>,
    pub(crate) base64_mode: bool,
    pub(crate) full_page: bool,
    pub(crate) viewport_height: Option<u32>,
}

/// Data URL prefix returned by `canvas.toDataURL('image/png')`.
const PNG_DATA_URL_PREFIX: &str = "data:image/png;base64,";

/// Detect the Firefox-internal "Unable to load actor module" failure that
/// indicates a screenshot actor cannot be instantiated on this build.
///
/// Firefox surfaces this as an `unknownError` with a message similar to:
/// `Error occurred while creating actor' .../screenshotActor: Error: Unable
/// to load actor module 'devtools/server/actors/screenshot' …`.
///
/// The marker substring is stable across Firefox versions; matching on it
/// lets us distinguish a missing-module situation (where a clean
/// version-mismatch hint is the right UX) from genuine capture failures
/// (e.g. headless missing, large pages OOM-ing) where we need the raw error.
fn is_actor_module_load_failure(err: &ProtocolError) -> bool {
    // Display includes both the Firefox `error` code and the `message` text.
    err.to_string().contains("Unable to load actor module")
}

/// Detect the Firefox 149+ ESM `global` option regression specifically.
///
/// `capture-screenshot.js` uses `ChromeUtils.importESModule` without the
/// `global` option, which fails on Firefox 149+ DevTools distinct globals.
/// This specific error means the chrome-scope loader workaround is applicable.
fn is_esm_global_option_error(err: &ProtocolError) -> bool {
    err.to_string()
        .contains("global option is required in DevTools distinct global")
}

/// Build the canonical user-facing message for a missing screenshot actor.
///
/// Centralised so the legacy and Firefox 149+ paths produce identical wording,
/// and so the e2e test can match the exact phrasing.  The message names
/// `doctor` per the iter-53 contract and includes the observed Firefox
/// version when known so users can compare against the supported floor.
fn version_mismatch_message() -> String {
    let observed = match crate::connection_meta::remembered_version() {
        Some(v) => format!("{v}"),
        None => "unknown".to_owned(),
    };
    format!(
        "screenshot actor unavailable on Firefox {observed}; minimum supported version: {COMPATIBLE_FIREFOX_MIN}. \
         hint: upgrade Firefox or run `ff-rdp doctor` for the full compatibility report."
    )
}

/// Take a screenshot and return the result value without printing.
///
/// Called by the script runner, which handles its own NDJSON output.
pub fn run_core(cli: &Cli, opts: &ScreenshotOpts<'_>) -> Result<serde_json::Value, AppError> {
    let height_override = match (opts.full_page, opts.viewport_height) {
        (true, Some(_)) => {
            return Err(AppError::User(
                "screenshot: --full-page and --viewport-height are mutually exclusive".to_owned(),
            ));
        }
        (true, None) => Some(HeightOverride::FullPage),
        (false, Some(n)) => Some(HeightOverride::Explicit(n)),
        (false, None) => None,
    };
    let output_path = opts.output_path;
    let base64_mode = opts.base64_mode;
    let screenshot_js = build_screenshot_js(height_override);
    let screenshot_js = screenshot_js.as_str();
    // Screenshot always connects directly to Firefox, bypassing the daemon.
    // The daemon's watcher subscription interferes with the two-step screenshot
    // protocol, causing Firefox-side timeouts.
    let mut ctx = connect_direct(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let eval_result = eval_or_bail(
        &mut ctx,
        &console_actor,
        screenshot_js,
        "screenshot JS threw an exception",
    )?;

    // Resolve the result — the data URL may come back as a LongString when the
    // PNG is large enough to exceed Firefox's inline-string threshold.
    let data_url = if let Some(url) = resolve_string(&mut ctx, &eval_result.result)? {
        url
    } else {
        // drawWindow unavailable.  Try fallbacks in order:
        //   1. Legacy single-step screenshotContentActor (Firefox < 149)
        //   2. Two-step protocol: screenshotContentActor.prepareCapture +
        //      root screenshotActor.capture (Firefox 149+)
        //
        // Clone actor IDs to release the borrow on `ctx.target` before
        // taking a mutable borrow on `ctx` for transport calls.
        let sc_actor = ctx.target.screenshot_content_actor.clone();
        let browsing_ctx_id = ctx.target.browsing_context_id;

        if let Some(actor) = sc_actor {
            // Try legacy method first.
            match ScreenshotContentActor::capture(
                ctx.transport_mut(),
                actor.as_ref(),
                height_override.is_some_and(|h| matches!(h, HeightOverride::FullPage)),
            ) {
                Ok(capture) => capture.data,
                Err(legacy_err) if legacy_err.is_unrecognized_packet_type() => {
                    // Legacy method not available — try the Firefox 149+ two-step protocol.
                    try_two_step_screenshot(
                        &mut ctx,
                        &actor,
                        browsing_ctx_id,
                        height_override.is_some_and(|h| matches!(h, HeightOverride::FullPage)),
                    )?
                }
                Err(e) if is_actor_module_load_failure(&e) => {
                    // Screenshot actor module cannot be loaded on this Firefox
                    // build — surface a clean version-mismatch message rather
                    // than the raw Firefox stack trace.
                    return Err(AppError::User(format!(
                        "screenshot: {}",
                        version_mismatch_message()
                    )));
                }
                Err(e) => {
                    return Err(AppError::User(format!(
                        "screenshot: screenshotContentActor capture failed ({e}) — \
                         screenshots require headless mode; relaunch with: ff-rdp launch --headless"
                    )));
                }
            }
        } else {
            return Err(AppError::User(
                "screenshot: drawWindow not available and no screenshotContentActor found — \
                 screenshots require headless mode; relaunch with: ff-rdp launch --headless"
                    .to_owned(),
            ));
        }
    };

    let b64 = data_url.strip_prefix(PNG_DATA_URL_PREFIX).ok_or_else(|| {
        AppError::User(format!(
            "screenshot: unexpected data URL format (expected prefix '{PNG_DATA_URL_PREFIX}')"
        ))
    })?;

    // Decode bytes only for dimension extraction and file output.
    // For base64 mode the raw `b64` string is used directly — no need to
    // decode and re-encode.
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| AppError::from(anyhow::anyhow!("screenshot: base64 decode failed: {e}")))?;

    // Infer dimensions from PNG header: width at bytes 16–19, height at 20–23.
    let (width, height) = png_dimensions(&png_bytes).unwrap_or((0, 0));

    let results = if base64_mode {
        // Return the base64 string directly — strip the data-URL prefix but
        // do not decode+re-encode; the `b64` slice is already valid
        // standard-base64.
        json!({
            "base64": b64,
            "width": width,
            "height": height,
            "bytes": png_bytes.len(),
        })
    } else {
        let dest = resolve_output_path(output_path)
            .map_err(|e| AppError::from(anyhow::anyhow!("screenshot: {e}")))?;

        std::fs::write(&dest, &png_bytes)
            .with_context(|| format!("screenshot: could not write to '{}'", dest.display()))
            .map_err(AppError::from)?;

        let abs_path = dest
            .canonicalize()
            .unwrap_or(dest)
            .to_string_lossy()
            .into_owned();

        json!({
            "path": abs_path,
            "width": width,
            "height": height,
            "bytes": png_bytes.len(),
        })
    };
    Ok(results)
}

pub fn run(cli: &Cli, opts: &ScreenshotOpts<'_>) -> Result<(), AppError> {
    let results = run_core(cli, opts)?;
    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    let envelope = output::envelope(&results, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::Screenshot);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

/// Firefox 149+ two-step screenshot protocol.
///
/// Step 1: `screenshotContentActor.prepareCapture` → viewport DPR/zoom/rect
/// Step 2: `screenshotActor.capture` (root actor) → PNG data URL
///
/// `full_page` is forwarded to both steps so Firefox captures the full scroll
/// height rather than just the visible viewport.
///
/// Returns the `data:image/png;base64,...` string on success.
fn try_two_step_screenshot(
    ctx: &mut super::connect_tab::ConnectedTab,
    sc_actor: &ff_rdp_core::ActorId,
    browsing_ctx_id: Option<u64>,
    full_page: bool,
) -> Result<String, AppError> {
    let browsing_ctx_id = browsing_ctx_id.ok_or_else(|| {
        AppError::User(
            "screenshot: Firefox 149+ screenshot requires a browsing context ID \
             which was not found in the target response. \
             Try upgrading ff-rdp or filing a bug with your Firefox version."
                .to_owned(),
        )
    })?;

    // Step 1: prepare — collect viewport DPR/zoom from the content process actor.
    let prep =
        ScreenshotContentActor::prepare_capture(ctx.transport_mut(), sc_actor.as_ref(), full_page)
            .map_err(|e| {
                if is_actor_module_load_failure(&e) {
                    AppError::User(format!("screenshot: {}", version_mismatch_message()))
                } else {
                    AppError::User(format!(
                        "screenshot: screenshotContentActor.prepareCapture failed ({e})"
                    ))
                }
            })?;

    // Step 2: capture — call the root-level screenshotActor.
    let screenshot_actor = ScreenshotActor::get_actor_id(ctx.transport_mut()).map_err(|e| {
        AppError::User(format!(
            "screenshot: could not find root screenshotActor ({e}) — \
             this Firefox version may not support the two-step screenshot protocol"
        ))
    })?;

    let capture_result = ScreenshotActor::capture(
        ctx.transport_mut(),
        &screenshot_actor,
        browsing_ctx_id,
        full_page,
        &prep,
    );

    match capture_result {
        Ok(data) => Ok(data),
        Err(ref e) if is_esm_global_option_error(e) => {
            // Firefox 149+ regression: `capture-screenshot.js` calls
            // `ChromeUtils.importESModule` without the `global` option,
            // which fails in the DevTools distinct global.  Fall back to the
            // chrome-scope loader workaround that loads the same module via
            // the DevTools loader (which has the correct global).
            try_chrome_scope_screenshot(ctx, browsing_ctx_id, full_page)
        }
        Err(ref e) if is_actor_module_load_failure(e) => {
            // Generic actor-module-load failure not caused by the ESM global
            // option issue — surface a clean version-mismatch message.
            Err(AppError::User(format!(
                "screenshot: {}",
                version_mismatch_message()
            )))
        }
        Err(e) => Err(AppError::User(format!(
            "screenshot: screenshotActor.capture failed ({e}) — \
             screenshots require headless mode; relaunch with: ff-rdp launch --headless"
        ))),
    }
}

/// Chrome-scope screenshot fallback for Firefox 149+ where the root
/// `screenshotActor` module fails to load due to an ESM `global` option bug.
///
/// Strategy:
///  1. `listProcesses` → find the parent process descriptor.
///  2. `getTarget` on the process descriptor → get the chrome-privileged
///     `consoleActor`.
///  3. Fire an async `evaluateJSAsync` that:
///     - Uses `resource://devtools/shared/loader/Loader.sys.mjs` (accessible
///       from chrome scope) to `require()` `capture-screenshot.js` — this
///       bypasses the broken `ChromeUtils.importESModule` call in that module.
///     - Calls `captureScreenshot({…}, BrowsingContext.get(bcId))` which
///       returns a Promise.
///     - When the Promise resolves, writes the PNG bytes to a temp file via
///       `nsIFile` + `nsIBinaryOutputStream`.
///     - On error, writes the error message to an adjacent `.err` file.
///  4. Poll the filesystem for the temp file or the error sentinel.
///  5. Return the PNG bytes to the caller.
///
/// The async JS fires-and-forgets: `evaluateJSAsync` returns immediately with
/// `"ok"` while Firefox's event loop resolves the Promise in the background.
/// The poll timeout is 10 seconds; screenshots typically complete in < 500 ms.
///
/// `browsing_context_id` is the numeric ID of the content page to capture.
/// `full_page` controls whether the entire scroll height is captured.
fn try_chrome_scope_screenshot(
    ctx: &mut super::connect_tab::ConnectedTab,
    browsing_context_id: u64,
    full_page: bool,
) -> Result<String, AppError> {
    // Step 1: find the parent process.
    let processes = RootActor::list_processes(ctx.transport_mut()).map_err(|e| {
        AppError::User(format!(
            "screenshot: could not list processes for chrome-scope fallback ({e})"
        ))
    })?;

    let parent_proc = processes
        .iter()
        .find(|p: &&ProcessInfo| p.is_parent)
        .ok_or_else(|| {
            AppError::User("screenshot: no parent process found in listProcesses".to_owned())
        })?;

    // Step 2: get chrome console actor.
    let chrome_target = TabActor::get_process_target(ctx.transport_mut(), &parent_proc.actor)
        .map_err(|e| {
            AppError::User(format!(
                "screenshot: could not get parent process target ({e})"
            ))
        })?;

    let chrome_console = chrome_target.console_actor.clone();

    // Step 3: build temp file paths.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let tmp_png = std::env::temp_dir().join(format!("ff_rdp_chrome_cap_{ts}.png"));
    let tmp_err = std::env::temp_dir().join(format!("ff_rdp_chrome_cap_{ts}.err"));

    // Build a JSON-safe path string (forward slashes work on all platforms
    // inside Firefox's nsIFile on macOS/Linux; on Windows the JS receives a
    // backslash-escaped string from serde_json).
    let out_path_js = serde_json::to_string(&tmp_png.to_string_lossy().as_ref()).unwrap();
    let err_path_js = serde_json::to_string(&tmp_err.to_string_lossy().as_ref()).unwrap();

    let js = format!(
        r#"(function() {{
  var outPath = {out_path_js};
  var errPath = {err_path_js};
  var bcId    = {browsing_context_id};
  var full    = {full_page};

  function writeBytes(path, arr) {{
    var f = Cc["@mozilla.org/file/local;1"].createInstance(Ci.nsIFile);
    f.initWithPath(path);
    var s = Cc["@mozilla.org/network/file-output-stream;1"].createInstance(Ci.nsIFileOutputStream);
    s.init(f, 0x04|0x08|0x20, 0o644, 0);
    var b = Cc["@mozilla.org/binaryoutputstream;1"].createInstance(Ci.nsIBinaryOutputStream);
    b.setOutputStream(s); b.writeByteArray(arr); b.close(); s.close();
  }}
  function writeText(path, text) {{
    var f = Cc["@mozilla.org/file/local;1"].createInstance(Ci.nsIFile);
    f.initWithPath(path);
    var s = Cc["@mozilla.org/network/file-output-stream;1"].createInstance(Ci.nsIFileOutputStream);
    s.init(f, 0x04|0x08|0x20, 0o644, 0);
    var o = Cc["@mozilla.org/intl/converter-output-stream;1"].createInstance(Ci.nsIConverterOutputStream);
    o.init(s, "UTF-8"); o.writeString(text); o.close(); s.close();
  }}

  try {{
    var {{ loader }} = ChromeUtils.importESModule(
      "resource://devtools/shared/loader/Loader.sys.mjs",
      {{ global: "current" }}
    );
    var {{ captureScreenshot }} = loader.require("devtools/server/actors/utils/capture-screenshot");
    var bc = BrowsingContext.get(bcId);
    if (!bc) {{ writeText(errPath, "no BrowsingContext for id " + bcId); return "err:no-bc"; }}
    captureScreenshot({{ fullpage: full, dpr: 1.0, snapshotScale: 1.0 }}, bc)
      .then(function(r) {{
        var dataUrl = r.data;
        var b64 = dataUrl.replace(/^data:image\/png;base64,/, "");
        var bin = atob(b64);
        var arr = new Uint8Array(bin.length);
        for (var i = 0; i < bin.length; i++) arr[i] = bin.charCodeAt(i);
        writeBytes(outPath, arr);
      }})
      .catch(function(e) {{ writeText(errPath, e.name + ": " + e.message); }});
    return "ok";
  }} catch(e) {{
    writeText(errPath, "sync: " + e.name + ": " + e.message);
    return "err:" + e.message;
  }}
}})()"#
    );

    // Fire the async capture.
    let kick_result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &chrome_console, &js)
        .map_err(|e| {
            AppError::User(format!(
                "screenshot: chrome-scope eval failed to start ({e})"
            ))
        })?;

    // Check the synchronous return value for early errors.
    if let Grip::Value(serde_json::Value::String(ref s)) = kick_result.result
        && s.starts_with("err:")
    {
        return Err(AppError::User(format!(
            "screenshot: chrome-scope capture failed: {s}"
        )));
    }

    // Step 4: poll for the output file.
    let poll_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if tmp_err.exists() {
            let msg = std::fs::read_to_string(&tmp_err).unwrap_or_default();
            let _ = std::fs::remove_file(&tmp_err);
            return Err(AppError::User(format!(
                "screenshot: chrome-scope capture failed: {msg}"
            )));
        }

        if tmp_png.exists()
            && let Ok(meta) = std::fs::metadata(&tmp_png)
            && meta.len() > 8
        {
            // Read the PNG bytes.
            let png_bytes = std::fs::read(&tmp_png).map_err(|e| {
                AppError::from(anyhow::anyhow!(
                    "screenshot: could not read chrome-scope temp PNG: {e}"
                ))
            })?;
            let _ = std::fs::remove_file(&tmp_png);

            // Encode as a data URL so the caller can use the same
            // prefix-stripping path as the other code paths.
            let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
            return Ok(format!("{PNG_DATA_URL_PREFIX}{b64}"));
        }

        if Instant::now() >= poll_deadline {
            let _ = std::fs::remove_file(&tmp_png);
            let _ = std::fs::remove_file(&tmp_err);
            return Err(AppError::User(
                "screenshot: chrome-scope capture timed out after 10s — \
                 the Firefox async capture promise did not resolve in time"
                    .to_owned(),
            ));
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Resolve the eval result to an `Option<String>`:
/// - `Grip::Null` → `None` (drawWindow unavailable)
/// - `Grip::Value(String)` → `Some(string)`
/// - `Grip::LongString` → fetch the full string and return `Some`
fn resolve_string(
    ctx: &mut super::connect_tab::ConnectedTab,
    grip: &Grip,
) -> Result<Option<String>, AppError> {
    match grip {
        Grip::Null | Grip::Undefined => Ok(None),
        Grip::Value(serde_json::Value::String(s)) => Ok(Some(s.clone())),
        Grip::LongString {
            actor,
            length,
            initial: _,
        } => {
            let full = LongStringActor::full_string(ctx.transport_mut(), actor.as_ref(), *length)
                .map_err(AppError::from)?;
            Ok(Some(full))
        }
        other => Err(AppError::User(format!(
            "screenshot: unexpected result type: {}",
            other.to_json()
        ))),
    }
}

/// Determine the output file path.
///
/// If the caller provided an explicit path, use it.  Otherwise generate a
/// timestamped filename in the current directory.
fn resolve_output_path(output_path: Option<&str>) -> anyhow::Result<PathBuf> {
    if let Some(p) = output_path {
        return Ok(PathBuf::from(p));
    }

    // Generate a unique filename from the current system time (millisecond
    // resolution to avoid collisions when taking multiple screenshots quickly).
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("system clock is before UNIX epoch")?
        .as_millis();

    Ok(PathBuf::from(format!("screenshot-{ts}.png")))
}

/// Extract width and height from a PNG file's IHDR chunk.
///
/// PNG structure: 8-byte signature, then chunks. The first chunk is always
/// IHDR which contains `width` (4 bytes, big-endian) at offset 16 and
/// `height` (4 bytes, big-endian) at offset 20.
fn png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 24 {
        return None;
    }
    let width = u32::from_be_bytes(data[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(data[20..24].try_into().ok()?);
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Cli, Command};
    use clap::Parser as _;

    // ── clap parse tests ────────────────────────────────────────────────────

    #[test]
    fn clap_screenshot_full_page_flag_parsed() {
        let cli = Cli::try_parse_from(["ff-rdp", "screenshot", "--full-page"])
            .expect("should parse --full-page");
        let Command::Screenshot { full_page, .. } = cli.command else {
            panic!("expected Screenshot command");
        };
        assert!(full_page, "--full-page flag must be set");
    }

    #[test]
    fn clap_a11y_limit_and_format_text_parsed() {
        let cli = Cli::try_parse_from(["ff-rdp", "a11y", "--limit", "5", "--format", "text"])
            .expect("should parse a11y --limit 5 --format text");
        assert_eq!(cli.limit, Some(5));
        assert_eq!(cli.format, "text");
        assert!(matches!(cli.command, Command::A11y { .. }));
    }

    #[test]
    fn png_dimensions_minimal_png() {
        // Minimal 1x1 white PNG generated from known bytes.
        let b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4//8/AAX+Av4N70a4AAAAAElFTkSuQmCC";
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap();
        let (w, h) = png_dimensions(&bytes).unwrap();
        assert_eq!(w, 1);
        assert_eq!(h, 1);
    }

    #[test]
    fn png_dimensions_too_short() {
        assert!(png_dimensions(&[0u8; 10]).is_none());
    }

    #[test]
    fn resolve_output_path_explicit() {
        let path = resolve_output_path(Some("/tmp/test.png")).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/test.png"));
    }

    #[test]
    fn resolve_output_path_auto_timestamped() {
        let path = resolve_output_path(None).unwrap();
        let name = path.to_string_lossy();
        assert!(
            name.starts_with("screenshot-")
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("png")),
            "unexpected auto path: {name}"
        );
    }

    #[test]
    fn strip_data_url_prefix() {
        let url = format!("{PNG_DATA_URL_PREFIX}abc123");
        let b64 = url.strip_prefix(PNG_DATA_URL_PREFIX);
        assert_eq!(b64, Some("abc123"));
    }

    #[test]
    fn strip_data_url_prefix_mismatch() {
        let url = "data:image/jpeg;base64,abc";
        assert!(url.strip_prefix(PNG_DATA_URL_PREFIX).is_none());
    }

    #[test]
    fn build_js_default_uses_inner_height() {
        let js = build_screenshot_js(None);
        assert!(js.contains("window.innerHeight"));
        assert!(!js.contains("scrollHeight"));
    }

    #[test]
    fn build_js_full_page_uses_scroll_height() {
        let js = build_screenshot_js(Some(HeightOverride::FullPage));
        assert!(js.contains("scrollHeight"));
        assert!(js.contains("scrollingElement"));
    }

    #[test]
    fn is_actor_module_load_failure_matches_real_firefox_message() {
        use ff_rdp_core::{ActorErrorKind, ProtocolError};
        let err = ProtocolError::ActorError {
            actor: "server1.conn5.screenshotActor9".to_owned(),
            kind: ActorErrorKind::Other("unknownError".to_owned()),
            error: "unknownError".to_owned(),
            message: "Error occurred while creating actor' \
                      server1.conn5.screenshotActor9: \
                      Error: Unable to load actor module 'devtools/server/actors/screenshot' \
                      ChromeUtils.importESModule: global option is required"
                .to_owned(),
        };
        assert!(
            is_actor_module_load_failure(&err),
            "should match the real-world failure shape"
        );
    }

    #[test]
    fn is_actor_module_load_failure_rejects_unrelated_actor_error() {
        use ff_rdp_core::{ActorErrorKind, ProtocolError};
        let err = ProtocolError::ActorError {
            actor: "server1.conn0.child2/screenshotContentActor15".to_owned(),
            kind: ActorErrorKind::Other("unknownError".to_owned()),
            error: "unknownError".to_owned(),
            message: "out of memory".to_owned(),
        };
        assert!(
            !is_actor_module_load_failure(&err),
            "unrelated errors must not match the module-load detector"
        );
    }

    #[test]
    fn is_actor_module_load_failure_rejects_timeout() {
        let err = ff_rdp_core::ProtocolError::Timeout;
        assert!(!is_actor_module_load_failure(&err));
    }

    #[test]
    fn build_js_explicit_height_inlines_value() {
        let js = build_screenshot_js(Some(HeightOverride::Explicit(4321)));
        assert!(
            js.contains("var h = 4321;"),
            "expected explicit height to be inlined, got: {js}"
        );
    }

    #[test]
    fn is_esm_global_option_error_matches_firefox_150_regression() {
        use ff_rdp_core::{ActorErrorKind, ProtocolError};
        let err = ProtocolError::ActorError {
            actor: "server1.conn5.screenshotActor9".to_owned(),
            kind: ActorErrorKind::Other("unknownError".to_owned()),
            error: "unknownError".to_owned(),
            message: "Error occurred while creating actor' \
                      server1.conn5.screenshotActor9: \
                      Error: Unable to load actor module 'devtools/server/actors/screenshot' \
                      ChromeUtils.importESModule: global option is required in DevTools distinct global"
                .to_owned(),
        };
        assert!(
            is_esm_global_option_error(&err),
            "should match the Firefox 150 ESM regression error"
        );
    }

    #[test]
    fn is_esm_global_option_error_rejects_generic_module_load_failure() {
        use ff_rdp_core::{ActorErrorKind, ProtocolError};
        // A module-load failure without the specific ESM global option error.
        let err = ProtocolError::ActorError {
            actor: "server1.conn0.child2/screenshotContentActor15".to_owned(),
            kind: ActorErrorKind::Other("unknownError".to_owned()),
            error: "unknownError".to_owned(),
            message: "Error: Unable to load actor module 'devtools/server/actors/screenshot' \
                      file not found"
                .to_owned(),
        };
        assert!(
            !is_esm_global_option_error(&err),
            "generic module-load failures should not match the ESM regression detector"
        );
    }

    #[test]
    fn is_esm_global_option_error_rejects_unrelated_error() {
        let err = ff_rdp_core::ProtocolError::Timeout;
        assert!(!is_esm_global_option_error(&err));
    }
}
