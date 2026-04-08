use std::path::PathBuf;

use anyhow::Context as _;
use base64::Engine as _;
use ff_rdp_core::{Grip, LongStringActor, ScreenshotContentActor};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::eval_helpers::eval_or_bail;

/// JavaScript injected into the page to capture a screenshot.
///
/// Uses `CanvasRenderingContext2D.drawWindow`, a Firefox-specific privileged
/// API that renders the current viewport into an offscreen canvas.  Returns a
/// `data:image/png;base64,...` string on success, or `null` when `drawWindow`
/// is not available (removed in some Firefox configurations).
const SCREENSHOT_JS: &str = r"(function() {
  var w = window.innerWidth || document.documentElement.clientWidth || 800;
  var h = window.innerHeight || document.documentElement.clientHeight || 600;
  var canvas = document.createElement('canvas');
  canvas.width = w;
  canvas.height = h;
  var ctx = canvas.getContext('2d');
  if (!ctx || typeof ctx.drawWindow !== 'function') { return null; }
  ctx.drawWindow(window, 0, 0, w, h, 'rgb(255,255,255)');
  return canvas.toDataURL('image/png');
})()";

/// Data URL prefix returned by `canvas.toDataURL('image/png')`.
const PNG_DATA_URL_PREFIX: &str = "data:image/png;base64,";

pub fn run(cli: &Cli, output_path: Option<&str>, base64_mode: bool) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let eval_result = eval_or_bail(ctx.transport_mut(), &console_actor, SCREENSHOT_JS)?;

    // Resolve the result — the data URL may come back as a LongString when the
    // PNG is large enough to exceed Firefox's inline-string threshold.
    let data_url = if let Some(url) = resolve_string(&mut ctx, &eval_result.result)? {
        url
    } else {
        // drawWindow unavailable — try screenshotContentActor fallback.
        // Clone the actor ID to release the borrow on `ctx.target` before
        // we take a mutable borrow on `ctx` for the transport call.
        let actor_id = ctx.target.screenshot_content_actor.clone();
        if let Some(actor) = actor_id {
            let capture = ScreenshotContentActor::capture(ctx.transport_mut(), actor.as_ref())
                .map_err(|e| {
                    AppError::User(format!(
                        "screenshot: drawWindow not available and screenshotContentActor \
                         capture failed ({e}) — screenshots require headless mode; \
                         relaunch with: ff-rdp launch --headless"
                    ))
                })?;
            capture.data
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

    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&results, 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
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
}
