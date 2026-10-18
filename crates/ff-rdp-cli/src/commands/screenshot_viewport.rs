//! `screenshot --viewport WxH`: a phone-width capture on the live tab.
//!
//! RDP has no viewport sizing (Responsive Design Mode is parent-chrome CSS),
//! and a live window clamps at about 500 CSS px, so this draws the page at
//! the requested size inside an `<iframe>` instead:
//!
//! 1. inject an absolutely positioned wrapper holding `<iframe
//!    src=location.href>` sized exactly `W`x`H` CSS px at the top-left of the
//!    visible viewport;
//! 2. wait for the frame's `load`, switch its in-viewport `loading="lazy"`
//!    images to eager and wait (bounded) for them to load and decode;
//! 3. capture just the wrapper's rect through the root `screenshotActor`;
//! 4. remove the wrapper again — on every exit path.
//!
//! Media queries inside the frame evaluate against `W`, because the frame is
//! its own viewport. The frame loads the URL afresh, so state created by
//! earlier commands (an open dialog, a filled form) is not in the capture.
//!
//! A page can refuse framing (`X-Frame-Options: DENY`, CSP
//! `frame-ancestors`): the frame's document is then not readable from the
//! page. Or the page's own CSP (`frame-src`, `child-src`, `default-src`)
//! forbids it to frame itself: then no `load` fires, only a
//! `securitypolicyviolation` event. Both are reported as a user error naming
//! the cause.

use std::time::{Duration, Instant};

use ff_rdp_core::{CaptureRect, Grip, PrepareCapture, ScreenshotActor, ScreenshotContentActor};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::AppError;

use super::connect_tab::ConnectedTab;
use super::js_helpers::eval_or_bail;

/// Largest accepted `--viewport` edge, in CSS px. Beyond this the capture is
/// a canvas-size failure waiting to happen and no device is that large.
const MAX_EDGE_PX: u32 = 8192;

/// How long images in the frame's first viewport may take to load and decode
/// after the frame's `load` event before the capture goes ahead anyway.
const IMAGE_SETTLE_BUDGET: Duration = Duration::from_secs(5);

/// How long the header probe of a refused frame may take.
const PROBE_BUDGET: Duration = Duration::from_secs(3);

/// Interval between state polls of the injected frame.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Parse a `--viewport WxH` value into CSS pixels.
pub(crate) fn parse_viewport(s: &str) -> Result<(u32, u32), AppError> {
    let invalid = || {
        AppError::User(format!(
            "invalid --viewport '{s}': expected WxH in CSS pixels (e.g. 390x844)"
        ))
    };
    let (w, h) = s.split_once(['x', 'X']).ok_or_else(invalid)?;
    let width: u32 = w.trim().parse().map_err(|_| invalid())?;
    let height: u32 = h.trim().parse().map_err(|_| invalid())?;
    if width == 0 || height == 0 || width > MAX_EDGE_PX || height > MAX_EDGE_PX {
        return Err(AppError::User(format!(
            "invalid --viewport '{s}': width and height must be between 1 and {MAX_EDGE_PX}"
        )));
    }
    Ok((width, height))
}

/// A capture made by [`capture`]: the PNG and the fields it adds to `results`.
pub(crate) struct ViewportCapture {
    pub(crate) png: Vec<u8>,
    pub(crate) fields: Vec<(&'static str, Value)>,
}

/// Capture the connected tab's URL laid out at `width`x`height` CSS px.
///
/// `load_budget` bounds the wait for the frame's `load` event.
pub(crate) fn capture(
    ctx: &mut ConnectedTab,
    (width, height): (u32, u32),
    load_budget: Duration,
) -> Result<ViewportCapture, AppError> {
    let id = call_id();
    let console_actor = ctx.target().console_actor.clone();

    let injected = eval_json(
        ctx,
        &console_actor,
        &build_inject_js(width, height, &id),
        "screenshot --viewport: could not insert the iframe wrapper",
    );
    let result = injected.and_then(|origin| {
        let left = origin.get("left").and_then(Value::as_f64).unwrap_or(0.0);
        let top = origin.get("top").and_then(Value::as_f64).unwrap_or(0.0);
        let window_clipped = origin
            .get("window_clipped")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let settled = wait_for_frame(ctx, &id, load_budget)?;
        let rect = CaptureRect {
            left,
            top,
            width: f64::from(width),
            height: f64::from(height),
        };
        let (png, dppx) = capture_rect(ctx, rect, (width, height))?;
        Ok(ViewportCapture {
            png,
            fields: vec![
                ("capture", json!("live-viewport-iframe")),
                (
                    "viewport",
                    json!({"width": width, "height": height, "dppx": dppx}),
                ),
                ("url", json!(settled.href)),
                ("load_complete", json!(settled.load_complete)),
                ("images_settled", json!(settled.images_settled)),
                ("images_pending", json!(settled.images_pending)),
                ("window_clipped", json!(window_clipped)),
            ],
        })
    });

    // Always remove the wrapper, whatever happened above. A cleanup failure
    // only matters when the capture itself succeeded.
    let cleanup = eval_or_bail(
        ctx,
        &console_actor,
        build_cleanup_js(),
        "screenshot --viewport: could not remove the iframe wrapper",
    );
    match (result, cleanup) {
        (Ok(capture), Ok(_)) => Ok(capture),
        (Ok(_), Err(e)) => Err(AppError::User(format!(
            "screenshot --viewport: captured, but the iframe wrapper could not be removed from \
             the page ({e}); reload the tab to clear it"
        ))),
        (Err(e), _) => Err(e),
    }
}

/// Outcome of [`wait_for_frame`].
struct FrameOutcome {
    href: String,
    load_complete: bool,
    images_settled: bool,
    /// `None` when the frame never loaded, so no images were counted.
    images_pending: Option<u64>,
}

/// One poll of the injected frame's state (see [`build_poll_js`]).
#[derive(Debug, Deserialize, PartialEq)]
struct FrameState {
    /// `loading` | `loaded` | `refused` | `gone`.
    phase: String,
    #[serde(default)]
    images_settled: bool,
    /// First-viewport images still loading; `null` until the frame loaded.
    #[serde(default)]
    pending: Option<u64>,
    #[serde(default)]
    probed: bool,
    #[serde(default)]
    xfo: Option<String>,
    #[serde(default)]
    csp: Option<String>,
    /// The page's own CSP directive that blocked the frame, if one fired.
    #[serde(default)]
    violation: Option<String>,
    /// The frame document's `readyState`, when readable.
    #[serde(default)]
    ready: Option<String>,
    /// The frame's `location.href`, when readable.
    #[serde(default)]
    href: Option<String>,
}

/// Poll until the frame has loaded and its first-viewport images settled.
fn wait_for_frame(
    ctx: &mut ConnectedTab,
    id: &str,
    load_budget: Duration,
) -> Result<FrameOutcome, AppError> {
    let console_actor = ctx.target().console_actor.clone();
    let poll_js = build_poll_js(id);
    let poll = |ctx: &mut ConnectedTab| -> Result<FrameState, AppError> {
        let v = eval_json(
            ctx,
            &console_actor,
            &poll_js,
            "screenshot --viewport: could not read the iframe state",
        )?;
        serde_json::from_value(v).map_err(|e| {
            AppError::User(format!(
                "screenshot --viewport: unexpected iframe state: {e}"
            ))
        })
    };

    // A refused frame: give the header probe a moment, then name the cause.
    let refused = |ctx: &mut ConnectedTab, mut state: FrameState| -> AppError {
        let probe_deadline = Instant::now() + PROBE_BUDGET;
        while !state.probed && Instant::now() < probe_deadline {
            std::thread::sleep(POLL_INTERVAL);
            match poll(ctx) {
                Ok(next) => state = next,
                Err(e) => return e,
            }
        }
        refused_error(&state)
    };
    let gone = || {
        AppError::User(
            "screenshot --viewport: the iframe wrapper disappeared before the capture (did the \
             tab navigate?)"
                .to_owned(),
        )
    };

    // Phase 1: the frame's `load` event.
    let load_deadline = Instant::now() + load_budget;
    let mut state = loop {
        let state = poll(ctx)?;
        match state.phase.as_str() {
            "loaded" => break state,
            "refused" => return Err(refused(ctx, state)),
            "gone" => return Err(gone()),
            _ => {}
        }
        if Instant::now() >= load_deadline {
            // No `load` yet. A readable, parsed frame document is still worth
            // a picture (some resource keeps `load` pending); an unreadable or
            // blank one is not.
            return match readable_frame(&state) {
                Some(href) => Ok(FrameOutcome {
                    href,
                    load_complete: false,
                    images_settled: false,
                    images_pending: state.pending,
                }),
                None => Err(AppError::Timeout(format!(
                    "screenshot --viewport: the page did not load in the iframe within {} ms; \
                     raise --timeout, or use `screenshot --window-size WxH` (a separate one-shot \
                     Firefox capture)",
                    load_budget.as_millis()
                ))),
            };
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    // Phase 2: images in the frame's first viewport (bounded; reported, not fatal).
    let image_deadline = Instant::now() + IMAGE_SETTLE_BUDGET;
    while !state.images_settled && Instant::now() < image_deadline {
        std::thread::sleep(POLL_INTERVAL);
        state = poll(ctx)?;
        // The frame may navigate again (meta refresh, redirect): a new load
        // resets the image wait in the page; a cross-origin one refuses.
        match state.phase.as_str() {
            "refused" => return Err(refused(ctx, state)),
            "gone" => return Err(gone()),
            _ => {}
        }
    }
    Ok(FrameOutcome {
        href: state.href.clone().unwrap_or_default(),
        load_complete: true,
        images_settled: state.images_settled,
        images_pending: if state.images_settled {
            Some(0)
        } else {
            state.pending
        },
    })
}

/// The frame URL when its document is readable and past `loading`.
fn readable_frame(state: &FrameState) -> Option<String> {
    let href = state.href.as_deref()?;
    let ready = state.ready.as_deref()?;
    (href != "about:blank" && ready != "loading").then(|| href.to_owned())
}

/// The user error for a frame whose document the page cannot read.
fn refused_error(state: &FrameState) -> AppError {
    let frame_ancestors = state.csp.as_deref().and_then(|csp| {
        csp.split(';')
            .map(str::trim)
            .find(|d| d.to_ascii_lowercase().starts_with("frame-ancestors"))
    });
    let cause = match (
        state.violation.as_deref(),
        state.xfo.as_deref(),
        frame_ancestors,
    ) {
        (Some(directive), _, _) => format!(
            "the page's own Content-Security-Policy ({directive}) does not allow it to frame \
             itself"
        ),
        (None, Some(xfo), _) => {
            format!("the page refuses to be framed (X-Frame-Options: {xfo})")
        }
        (None, None, Some(fa)) => {
            format!("the page refuses to be framed (Content-Security-Policy: {fa})")
        }
        (None, None, None) => "the page could not be read inside an iframe — framing was \
                               refused, or the frame is not same-origin (a redirect to another \
                               origin, or a data:/file:/about: URL)"
            .to_owned(),
    };
    AppError::User(format!(
        "screenshot --viewport: {cause}. Use `screenshot --window-size WxH` instead, which \
         captures the URL in a separate one-shot Firefox"
    ))
}

/// Capture `rect` (document CSS px) of the top-level document; returns the
/// PNG and the device-pixel ratio it was rendered at.
///
/// Fails when the PNG is not `size` x dppx: a Firefox that ignores the capture
/// rect would otherwise hand back the whole window under a `--viewport` label.
fn capture_rect(
    ctx: &mut ConnectedTab,
    rect: CaptureRect,
    size: (u32, u32),
) -> Result<(Vec<u8>, f64), AppError> {
    use base64::Engine as _;

    let browsing_ctx_id = ctx.target().browsing_context_id.ok_or_else(|| {
        AppError::User(
            "screenshot --viewport: the target has no browsing context id to capture".to_owned(),
        )
    })?;
    let mut prep = match ctx.target().screenshot_content_actor.clone() {
        Some(actor) => {
            ScreenshotContentActor::prepare_capture(ctx.transport_mut(), actor.as_ref(), false)
                .map_err(|e| {
                    AppError::User(format!(
                        "screenshot --viewport: screenshotContentActor.prepareCapture failed ({e})"
                    ))
                })?
        }
        None => PrepareCapture::default_viewport(),
    };
    prep.rect = Some(rect);
    let dppx = prep.window_dpr * prep.window_zoom;

    let actor = ScreenshotActor::get_actor_id(ctx.transport_mut()).map_err(|e| {
        AppError::User(format!(
            "screenshot --viewport: no screenshotActor on this Firefox ({e}); run `ff-rdp doctor`"
        ))
    })?;
    let data_url =
        ScreenshotActor::capture(ctx.transport_mut(), &actor, browsing_ctx_id, false, &prep)
            .map_err(|e| {
                AppError::User(format!(
                    "screenshot --viewport: screenshotActor.capture failed ({e})"
                ))
            })?;
    let b64 = data_url
        .strip_prefix("data:image/png;base64,")
        .ok_or_else(|| {
            AppError::User("screenshot --viewport: capture did not return a PNG".to_owned())
        })?;
    let png = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| AppError::User(format!("screenshot --viewport: bad base64 PNG: {e}")))?;
    let got = super::screenshot::png_dimensions(&png);
    if !png_matches(got, size, dppx) {
        return Err(AppError::User(format!(
            "screenshot --viewport: Firefox returned a {} image for a {}x{} capture at {dppx} \
             dppx — this Firefox does not honour the capture rect; use `screenshot \
             --window-size WxH` instead",
            got.map_or_else(|| "non-PNG".to_owned(), |(w, h)| format!("{w}x{h}")),
            size.0,
            size.1
        )));
    }
    Ok((png, dppx))
}

/// Whether PNG dimensions `got` are `size` CSS px at `dppx` (within a pixel
/// of rounding).
fn png_matches(got: Option<(u32, u32)>, size: (u32, u32), dppx: f64) -> bool {
    let near = |px: u32, css: u32| (f64::from(px) - f64::from(css) * dppx).abs() <= 1.0;
    got.is_some_and(|(w, h)| near(w, size.0) && near(h, size.1))
}

/// Evaluate `js` (which returns a JSON string) and parse the result.
fn eval_json(
    ctx: &mut ConnectedTab,
    console_actor: &ff_rdp_core::ActorId,
    js: &str,
    error_context: &str,
) -> Result<Value, AppError> {
    let result = eval_or_bail(ctx, console_actor, js, error_context)?;
    match result.result {
        Grip::Value(Value::String(s)) => serde_json::from_str(&s)
            .map_err(|e| AppError::User(format!("{error_context}: unexpected reply {s:?}: {e}"))),
        other => Err(AppError::User(format!(
            "{error_context}: unexpected reply {}",
            other.to_json()
        ))),
    }
}

/// A per-call id for the wrapper, so a poll never reads a stale one.
fn call_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or_default();
    format!("{}-{nanos}", std::process::id())
}

/// The JS templates, shared with the fixture recorder in
/// `crates/ff-rdp-core/tests/live_record_fixtures.rs`.
const INJECT_JS: &str = include_str!("../../js/screenshot_viewport/inject.js");
const POLL_JS: &str = include_str!("../../js/screenshot_viewport/poll.js");
const CLEANUP_JS: &str = include_str!("../../js/screenshot_viewport/cleanup.js");

/// JS string literal for `s`.
fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_owned())
}

/// JS that inserts the wrapper + iframe (see `js/screenshot_viewport/inject.js`)
/// and returns `{"left", "top", "window_clipped"}`.
///
/// The wrapper sits at the top-left of the visible viewport so the frame is
/// on screen (Firefox throttles off-screen frames and never starts their
/// lazy images), in the top layer when the Popover API exists (above an open
/// modal `<dialog>`). The frame is sandboxed without `allow-top-navigation`
/// and `allow-popups`. Its state lives on the wrapper element as `__ffrdp`,
/// read back by [`build_poll_js`].
fn build_inject_js(width: u32, height: u32, id: &str) -> String {
    INJECT_JS
        .replace("__FFRDP_W__", &width.to_string())
        .replace("__FFRDP_H__", &height.to_string())
        .replace("__FFRDP_ID__", &js_str(id))
}

/// JS returning the frame state as JSON (see [`FrameState`]).
fn build_poll_js(id: &str) -> String {
    POLL_JS.replace("__FFRDP_ID__", &js_str(id))
}

/// JS removing every wrapper (also leftovers of an interrupted run).
fn build_cleanup_js() -> &'static str {
    CLEANUP_JS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_viewport_accepts_wxh() {
        assert_eq!(parse_viewport("390x844").unwrap(), (390, 844));
        assert_eq!(parse_viewport("390X844").unwrap(), (390, 844));
    }

    #[test]
    fn parse_viewport_rejects_bad_forms() {
        for bad in ["", "x", "390", "390x", "x844", "390*844", "-1x10", "a x b"] {
            let err = parse_viewport(bad).unwrap_err().to_string();
            assert!(
                err.contains("--viewport") && err.contains("WxH"),
                "{bad}: {err}"
            );
        }
    }

    #[test]
    fn parse_viewport_rejects_out_of_range() {
        for bad in ["0x844", "390x0", "9000x100"] {
            let err = parse_viewport(bad).unwrap_err().to_string();
            assert!(err.contains("between 1 and"), "{bad}: {err}");
        }
    }

    #[test]
    fn inject_js_sizes_the_frame_and_blocks_top_navigation() {
        let js = build_inject_js(390, 844, "7-1");
        assert!(js.contains("var W = 390, H = 844, ID = \"7-1\""));
        assert!(!js.contains("__FFRDP_"), "every placeholder substituted");
        assert!(js.contains("f.src = location.href"));
        assert!(js.contains("'sandbox', 'allow-scripts allow-same-origin allow-forms'"));
        assert!(!js.contains("allow-top-navigation"));
        assert!(!js.contains("allow-popups"));
        assert!(js.contains("showPopover"));
        assert!(js.contains("img.loading = 'eager'"));
        assert!(js.contains("data-ffrdp-viewport"));
    }

    #[test]
    fn poll_and_cleanup_js_target_the_wrapper() {
        let poll = build_poll_js("a\"b");
        assert!(poll.contains(r#"var ID = "a\"b";"#), "{poll}");
        assert!(!poll.contains("__FFRDP_"));
        assert!(build_cleanup_js().contains("[data-ffrdp-viewport]"));
    }

    fn state(json: Value) -> FrameState {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn refused_error_names_x_frame_options() {
        let s = state(json!({"phase": "refused", "probed": true, "xfo": "DENY", "csp": null}));
        let msg = refused_error(&s).to_string();
        assert!(msg.contains("X-Frame-Options: DENY"), "{msg}");
        assert!(msg.contains("--window-size"), "{msg}");
    }

    #[test]
    fn refused_error_names_frame_ancestors() {
        let s = state(json!({
            "phase": "refused", "probed": true,
            "csp": "default-src 'self'; frame-ancestors 'none'"
        }));
        let msg = refused_error(&s).to_string();
        assert!(
            msg.contains("Content-Security-Policy: frame-ancestors 'none'"),
            "{msg}"
        );
    }

    #[test]
    fn refused_error_names_the_pages_own_frame_src() {
        let s = state(json!({"phase": "refused", "probed": true, "violation": "frame-src"}));
        let msg = refused_error(&s).to_string();
        assert!(
            msg.contains("own Content-Security-Policy (frame-src)"),
            "{msg}"
        );
    }

    #[test]
    fn refused_error_without_headers_explains_origin() {
        let s = state(json!({"phase": "refused", "probed": true, "csp": "default-src *"}));
        let msg = refused_error(&s).to_string();
        assert!(msg.contains("not same-origin"), "{msg}");
    }

    #[test]
    fn png_matches_allows_rounding_only() {
        assert!(png_matches(Some((390, 844)), (390, 844), 1.0));
        assert!(png_matches(Some((780, 1688)), (390, 844), 2.0));
        assert!(png_matches(Some((585, 1266)), (390, 844), 1.5));
        assert!(!png_matches(Some((1366, 683)), (390, 844), 1.0));
        assert!(!png_matches(None, (390, 844), 1.0));
    }

    #[test]
    fn readable_frame_requires_a_parsed_non_blank_document() {
        let ok = state(json!({"phase": "loading", "ready": "interactive", "href": "https://a/"}));
        assert_eq!(readable_frame(&ok).as_deref(), Some("https://a/"));
        let blank = state(json!({"phase": "loading", "ready": "complete", "href": "about:blank"}));
        assert_eq!(readable_frame(&blank), None);
        let parsing = state(json!({"phase": "loading", "ready": "loading", "href": "https://a/"}));
        assert_eq!(readable_frame(&parsing), None);
        let unreadable = state(json!({"phase": "loading"}));
        assert_eq!(readable_frame(&unreadable), None);
    }
}
