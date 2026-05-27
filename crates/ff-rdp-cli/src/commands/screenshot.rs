use std::path::PathBuf;

use anyhow::Context as _;
use base64::Engine as _;
use ff_rdp_core::{
    COMPATIBLE_FIREFOX_MIN, CaptureRect, Grip, ProtocolError, ScreenshotActor,
    ScreenshotContentActor,
};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_direct;
use super::js_helpers::eval_or_bail;

/// Options accepted by [`run`].
pub(crate) struct ScreenshotOpts<'a> {
    pub(crate) output_path: Option<&'a str>,
    pub(crate) base64_mode: bool,
    pub(crate) full_page: bool,
    /// Request the bulk-transfer path for screenshot data.
    ///
    /// When `true`, the command attempts to receive the screenshot payload via
    /// `Transport::recv_bulk_with_handler` (streaming directly to the output
    /// writer without a full in-memory base64 buffer).  If Firefox responds with
    /// a JSON packet instead of a bulk frame, the command falls back to the
    /// standard base64 path transparently.
    ///
    /// Note: as of Firefox 130, `screenshot.capture` returns JSON (base64-encoded
    /// PNG).  The bulk path is a daemon-side fast path reserved for future use
    /// when Firefox's screenshot actor gains native bulk-frame support.
    pub(crate) bulk: bool,
    /// `--viewport-height` is accepted for CLI compatibility but is not
    /// supported by the snapshot-actor path.  Passing it returns an error.
    pub(crate) viewport_height: Option<u32>,
    /// When set, the resolved output path must be a descendant of this root.
    pub(crate) output_root: Option<&'a std::path::Path>,
}

/// Data URL prefix returned by the screenshot actor.
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

/// Build the canonical user-facing message for a missing screenshot actor.
///
/// Centralised so the message names `doctor` per the iter-53 contract and
/// includes the observed Firefox version when known.
fn version_mismatch_message() -> String {
    let observed = match crate::connection_meta::remembered_version() {
        Some(v) => format!("{v}"),
        None => "unknown".to_owned(),
    };
    format!(
        "screenshot actor not found in Firefox {observed} root form. \
         Run `ff-rdp doctor` for the full compatibility report \
         (minimum supported: {COMPATIBLE_FIREFOX_MIN})."
    )
}

/// Take a screenshot and return the result value without printing.
///
/// Called by the script runner, which handles its own NDJSON output.
pub fn run_core(cli: &Cli, opts: &ScreenshotOpts<'_>) -> Result<serde_json::Value, AppError> {
    if opts.full_page && opts.viewport_height.is_some() {
        return Err(AppError::User(
            "screenshot: --full-page and --viewport-height are mutually exclusive".to_owned(),
        ));
    }
    if opts.viewport_height.is_some() {
        return Err(AppError::User(
            "screenshot: --viewport-height is not supported; use --full-page or omit the flag \
             to capture the visible viewport"
                .to_owned(),
        ));
    }

    // Screenshot always connects directly to Firefox, bypassing the daemon.
    // The daemon's watcher subscription interferes with the two-step screenshot
    // protocol, causing Firefox-side timeouts.
    let mut ctx = connect_direct(cli)?;

    let sc_actor = ctx.target.screenshot_content_actor.clone();
    let browsing_ctx_id = ctx.target.browsing_context_id;

    let sc_actor = sc_actor.ok_or_else(|| {
        AppError::User(
            "screenshot: no screenshotContentActor found — \
             screenshots require headless mode; relaunch with: ff-rdp launch --headless"
                .to_owned(),
        )
    })?;

    let data_url = try_two_step_screenshot(
        &mut ctx,
        &sc_actor,
        browsing_ctx_id,
        opts.full_page,
        opts.bulk,
    )?;

    let b64 = data_url.strip_prefix(PNG_DATA_URL_PREFIX).ok_or_else(|| {
        AppError::User(format!(
            "screenshot: unexpected data URL format (expected prefix '{PNG_DATA_URL_PREFIX}')"
        ))
    })?;

    // Decode bytes only for dimension extraction and file output.
    // For base64 mode the raw `b64` string is used directly.
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| AppError::from(anyhow::anyhow!("screenshot: base64 decode failed: {e}")))?;

    let (width, height) = png_dimensions(&png_bytes).unwrap_or((0, 0));

    let results = if opts.base64_mode {
        json!({
            "base64": b64,
            "width": width,
            "height": height,
            "bytes": png_bytes.len(),
        })
    } else {
        let dest = resolve_output_path(opts.output_path)
            .map_err(|e| AppError::from(anyhow::anyhow!("screenshot: {e}")))?;

        if let Some(root) = opts.output_root {
            crate::util::safe_io::ensure_within_root(&dest, root).map_err(|e| {
                AppError::User(format!(
                    "screenshot: output path escapes --output-root: {e}"
                ))
            })?;
        }

        crate::util::safe_io::safe_write(&dest, &png_bytes)
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

/// Two-step screenshot protocol (canonical path).
///
/// Step 1: `screenshotContentActor.prepareCapture` → viewport DPR/zoom/rect.
/// Step 2: `screenshotActor.capture` (root actor) → PNG data URL.
///
/// `full_page` requests that Firefox captures the full scroll height rather than
/// just the visible viewport.
///
/// `bulk` — accepted for API compatibility but currently inactive.  As of
/// Firefox 130, `screenshot.capture` always returns JSON (base64-encoded PNG),
/// not a bulk frame.  If `--bulk` is requested, a debug message is logged and
/// the standard JSON path is used.  If Firefox ever adds native bulk screenshot
/// support, restore the bulk path via a separate mechanism (e.g. inspect the
/// JSON reply for a bulk hint, then dispatch a separate bulk read).
///
/// Returns the `data:image/png;base64,...` string on success.
fn try_two_step_screenshot(
    ctx: &mut super::connect_tab::ConnectedTab,
    sc_actor: &ff_rdp_core::ActorId,
    browsing_ctx_id: Option<u64>,
    full_page: bool,
    bulk: bool,
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
    let mut prep =
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

    // For full-page captures: Firefox's `prepareCapture` often returns a
    // viewport-sized rect (or null) even when `fullpage: true` is requested,
    // causing the capture to be clipped to the visible area. Fix: read the
    // actual scroll dimensions from the page and override the rect.
    //
    // This has been the root cause of `--full-page` capturing only the
    // viewport across dogfood sessions 48/49/51/52 (iter-61k A).
    if full_page {
        let console_actor = ctx.target.console_actor.clone();
        let scroll_js = r"(function() {
  var dpr = window.devicePixelRatio || 1;
  var w = Math.max(
    document.documentElement.scrollWidth,
    document.body ? document.body.scrollWidth : 0,
    window.innerWidth || 0
  );
  var h = Math.max(
    document.documentElement.scrollHeight,
    document.body ? document.body.scrollHeight : 0,
    window.innerHeight || 0
  );
  return JSON.stringify({dpr: dpr, scrollW: w, scrollH: h});
})()";
        if let Ok(eval_result) = eval_or_bail(
            ctx,
            &console_actor,
            scroll_js,
            "screenshot: scroll dims eval",
        ) && let Grip::Value(serde_json::Value::String(ref s)) = eval_result.result
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(s)
        {
            let scroll_w = v
                .get("scrollW")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0);
            let scroll_h = v
                .get("scrollH")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0);
            let dpr = v
                .get("dpr")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(1.0);
            if scroll_w > 0.0 && scroll_h > 0.0 {
                // Override prep rect with full-page dimensions in CSS pixels.
                prep.rect = Some(CaptureRect {
                    left: 0.0,
                    top: 0.0,
                    width: scroll_w,
                    height: scroll_h,
                });
                prep.window_dpr = dpr;
            }
        }
        // Non-fatal: if the eval fails we proceed with whatever prepareCapture
        // returned; the capture may still be viewport-sized in that edge case.
    }

    // Step 2: capture — call the root-level screenshotActor.
    //
    // Theme B (iter-84/85): On Firefox 151+, `screenshotActor` may be absent
    // from `getRoot` (it moved to the per-target form or was renamed).
    //
    // Fallback ladder:
    // A. Try `getRoot` → `screenshotActor` (standard path, Firefox 87-149+).
    // B. If absent or module-load failure, try `screenshot_via_target()` which
    //    sends the `screenshot` / `takeScreenshot` request directly to the
    //    WindowGlobalTarget actor (Firefox 151+).
    // C. If the target path also fails, surface a diagnostic error.
    let root_actor_result = ScreenshotActor::get_actor_id(ctx.transport_mut());

    let use_target_fallback = match &root_actor_result {
        Ok(_) => false,
        Err(e) => {
            tracing::debug!(
                target: "ff_rdp_cli::screenshot",
                "screenshotActor absent from getRoot ({e}); trying screenshot_via_target fallback"
            );
            true
        }
    };

    // Log a diagnostic if --bulk was requested.  The bulk path is not active
    // for current Firefox versions (which return JSON, not a bulk frame, for
    // screenshot.capture).  Using the bulk path would send the request via
    // send_capture_request and then call recv_bulk_with_handler; Firefox
    // responds with JSON, the JSON byte was previously peeked-and-lost,
    // poisoning the stream for subsequent commands.  Always use the JSON path.
    if bulk {
        tracing::debug!(
            target: "ff_rdp_cli::screenshot",
            "--bulk requested but current Firefox returns JSON; bulk path inactive"
        );
    }

    if use_target_fallback {
        // Theme B (iter-85) — Firefox 151+ path: send the screenshot request
        // directly to the WindowGlobalTarget actor.
        return ScreenshotActor::screenshot_via_target(
            ctx.transport_mut(),
            browsing_ctx_id,
            full_page,
            &prep,
        )
        .map_err(|e| {
            AppError::User(format!(
                "screenshot: root-form screenshotActor absent and target fallback failed ({e}) — \
                 {} — screenshots require headless mode; relaunch with: ff-rdp launch --headless",
                version_mismatch_message()
            ))
        });
    }

    // Standard path: use the root-level screenshotActor.
    let screenshot_actor = root_actor_result.map_err(|e| {
        AppError::User(format!(
            "screenshot: failed to get screenshotActor — {e}: {}",
            version_mismatch_message()
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
        Err(ref e) if is_actor_module_load_failure(e) => {
            // Module load failure on the root path — try the target fallback
            // before giving up (some FF 151 builds report this error instead of
            // omitting the field from getRoot).
            tracing::debug!(
                target: "ff_rdp_cli::screenshot",
                "screenshotActor module load failure; retrying via screenshot_via_target"
            );
            ScreenshotActor::screenshot_via_target(
                ctx.transport_mut(),
                browsing_ctx_id,
                full_page,
                &prep,
            )
            .map_err(|_| AppError::User(format!("screenshot: {}", version_mismatch_message())))
        }
        Err(e) => Err(AppError::User(format!(
            "screenshot: screenshotActor.capture failed ({e}) — \
             screenshots require headless mode; relaunch with: ff-rdp launch --headless"
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
        assert!(!is_actor_module_load_failure(&err));
    }

    #[test]
    fn is_actor_module_load_failure_rejects_timeout() {
        let err = ff_rdp_core::ProtocolError::Timeout;
        assert!(!is_actor_module_load_failure(&err));
    }
}
