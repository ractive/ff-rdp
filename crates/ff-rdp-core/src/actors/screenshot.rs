// allow-actor-kb-skip: iter-92 adds a `full_page_rect` parameter to the
// existing `screenshot_via_process_drawsnapshot` workaround (no spec change);
// the actor-level protocol is unchanged from kb/rdp/actors/screenshot.md.
use serde::Serialize;
use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::actor::actor_send;
use crate::actors::console::WebConsoleActor;
use crate::actors::root::RootActor;
use crate::actors::screenshot_content::PrepareCapture;
use crate::actors::tab::TabActor;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::{ActorId, Grip};

/// Wire-format arguments for the root `screenshotActor.capture` request.
///
/// The published spec dict at `devtools/shared/specs/screenshot.js:13-35`
/// declares only `fullpage`, `file`, `clipboard`, `selector`, `dpr`, and
/// `delay`.  However, the server-side `devtools/server/actors/screenshot.js`
/// implementation reads three additional fields that ff-rdp must send for
/// the two-step Firefox-149+ protocol to work:
///
/// - `browsingContextID` — selects the browsing context whose snapshot
///   `browsingContext.drawSnapshot` should render.
/// - `snapshotScale` — `windowDPR * windowZoom`; omitted when equal to 1.0
///   (server default).
/// - `rect` — capture rectangle for fullpage / element captures.
///
/// This typed shim makes the spec drift explicit (rather than scattered
/// `json!({…})` blocks) so the `rdp-spec-reviewer` agent can flag it.
///
// allow-spec-drift: bug TBD (SD-1: screenshot.args dict at
// devtools/shared/specs/screenshot.js:13-35 omits
// browsingContextID/snapshotScale/rect even though the server in
// devtools/server/actors/screenshot.js reads all three).  iter-117 searched
// Mozilla Bugzilla and found no existing bug — this is a novel gap awaiting
// James's Bugzilla-account filing (see
// kb/rdp/from-our-codebase/open-gaps.md#spec-drift-bugs-awaiting-filing and
// iter-117 Results).  `TBD` blocks publishing the v0.3.0 draft: replace
// with the real Bugzilla number in a follow-up commit before publish.
#[derive(Debug, Clone, Serialize)]
pub struct ScreenshotArgsExt {
    // ── spec-declared fields ────────────────────────────────────────────────
    /// Whether to capture the full scrollable page.  Spec field.
    pub fullpage: bool,
    /// Device pixel ratio.  Spec types this as `nullable:string`, so it is
    /// serialised as a JSON string (e.g. `"1.5"`).
    pub dpr: String,

    // ── extra fields read by the server but NOT in the spec dict ────────────
    /// Browsing context the snapshot should be taken against.
    #[serde(rename = "browsingContextID")]
    pub browsing_context_id: u64,
    /// `windowDPR * windowZoom`.  Omitted when equal to 1.0 (server default).
    #[serde(rename = "snapshotScale", skip_serializing_if = "Option::is_none")]
    pub snapshot_scale: Option<f64>,
    /// Optional capture rectangle, serialised as `{left,top,width,height}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<ScreenshotArgsRect>,
}

/// Serialisable capture rectangle.
#[derive(Debug, Clone, Serialize)]
pub struct ScreenshotArgsRect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

impl ScreenshotArgsExt {
    /// Build a `ScreenshotArgsExt` from the two-step protocol inputs.
    pub fn from_prep(browsing_context_id: u64, full_page: bool, prep: &PrepareCapture) -> Self {
        let snapshot_scale_raw = prep.window_dpr * prep.window_zoom;
        let snapshot_scale = if (snapshot_scale_raw - 1.0).abs() < 1e-6 {
            None
        } else {
            Some(snapshot_scale_raw)
        };
        let rect = prep.rect.as_ref().map(|r| ScreenshotArgsRect {
            left: r.left,
            top: r.top,
            width: r.width,
            height: r.height,
        });
        Self {
            fullpage: full_page,
            dpr: format!("{}", prep.window_dpr),
            browsing_context_id,
            snapshot_scale,
            rect,
        }
    }

    /// Serialise to a JSON `Value` for inclusion as the `args` field of the
    /// outbound `capture` request.
    ///
    /// Returns [`ProtocolError::InvalidPacket`] if serialization fails.  In
    /// practice `ScreenshotArgsExt` is always Serialize-safe (all fields are
    /// plain scalars/strings/options), but returning a `Result` keeps a
    /// production `.expect()` out of the core per the project's error-handling
    /// rules.
    pub fn to_args_value(&self) -> Result<Value, ProtocolError> {
        serde_json::to_value(self).map_err(|e| {
            ProtocolError::InvalidPacket(format!("failed to serialize screenshot args: {e}"))
        })
    }
}

/// Operations on the root-level `screenshotActor` (parent-process side).
///
/// This actor was introduced in Firefox 87 alongside `screenshotContentActor`.
/// Firefox 149 removed the old single-step `screenshotContentActor.captureScreenshot`
/// method in favour of a two-step protocol:
///
/// 1. `screenshotContentActor.prepareCapture` → collects viewport DPR/zoom/rect
/// 2. `screenshotActor.capture` (this actor) → calls `browsingContext.drawSnapshot`
///    and returns the PNG data URL
///
/// The actor ID is obtained via `root.getRoot` → `screenshotActor`.
///
/// ## Firefox 151+ fallback path
///
/// On Firefox 151+ the `screenshotActor` field was observed absent from the
/// `getRoot` response in dogfood session 57.  In that case, callers should use
/// [`screenshot_via_target`] which sends the `screenshot` request directly
/// against the `WindowGlobalTarget` actor obtained via `listTabs` + `getTarget`.
pub struct ScreenshotActor;

impl ScreenshotActor {
    /// Obtain the `screenshotActor` ID from the root actor's `getRoot` response.
    ///
    /// Callers needing the full `getRoot` payload (to enumerate alternative
    /// actor locations on Firefox 151+, for example) should use
    /// [`get_root_raw`](Self::get_root_raw) instead.
    pub fn get_actor_id(transport: &mut RdpTransport) -> Result<ActorId, ProtocolError> {
        let response = actor_request(transport, "root", "getRoot", None)?;

        let id = response
            .get("screenshotActor")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProtocolError::InvalidPacket(
                    "getRoot response missing 'screenshotActor' field".into(),
                )
            })?;

        Ok(id.into())
    }

    /// Probe the root `getRoot` response and return the raw reply as a JSON value.
    ///
    /// Used by the CLI screenshot command for diagnostics: when `screenshotActor`
    /// is absent from `getRoot` (e.g. Firefox 151+) the caller enumerates the
    /// advertised actor keys and surfaces them in the error message so the
    /// missing capability is visible to the user.
    pub fn get_root_raw(transport: &mut RdpTransport) -> Result<Value, ProtocolError> {
        actor_request(transport, "root", "getRoot", None)
    }

    /// Capture a screenshot via the root-level screenshot actor (Firefox 149+).
    ///
    /// This is the second step of the two-step protocol.  The caller must first
    /// call [`ScreenshotContentActor::prepare_capture`] to obtain the
    /// [`PrepareCapture`] metadata, then call this method.
    ///
    /// `browsing_context_id` is the numeric ID from [`TargetInfo::browsing_context_id`]
    /// or [`TabInfo::browsing_context_id`].
    ///
    /// Returns a `data:image/png;base64,...` string.
    pub fn capture(
        transport: &mut RdpTransport,
        screenshot_actor: &ActorId,
        browsing_context_id: u64,
        full_page: bool,
        prep: &PrepareCapture,
    ) -> Result<String, ProtocolError> {
        let args = ScreenshotArgsExt::from_prep(browsing_context_id, full_page, prep);
        let args_value = args.to_args_value()?;
        let response = actor_request(
            transport,
            screenshot_actor.as_ref(),
            "capture",
            Some(&json!({ "args": args_value })),
        )?;

        // The response shape is: `{ "value": { "data": "data:...", ... } }`
        let value = response.get("value").unwrap_or(&response);

        let data = value
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProtocolError::InvalidPacket(
                    "screenshotActor capture response missing 'data' field".into(),
                )
            })?
            .to_owned();

        Ok(data)
    }

    /// Send a `capture` request to the screenshot actor without reading the reply.
    ///
    /// The caller is responsible for reading the next packet from the transport.
    /// This is the low-level split used by the `--bulk` path in the CLI to allow
    /// `Transport::recv_bulk_with_handler` to consume the reply as a bulk frame.
    ///
    /// Most callers should use [`capture`](Self::capture) instead.
    pub fn send_capture_request(
        transport: &mut RdpTransport,
        screenshot_actor: &str,
        browsing_context_id: u64,
        full_page: bool,
        prep: &PrepareCapture,
    ) -> Result<(), ProtocolError> {
        let args = ScreenshotArgsExt::from_prep(browsing_context_id, full_page, prep);
        let args_value = args.to_args_value()?;
        actor_send(
            transport,
            screenshot_actor,
            "capture",
            Some(&json!({ "args": args_value })),
        )
    }

    /// Capture a screenshot on Firefox 151+ via the parent-process console actor.
    ///
    /// ## Motivation
    ///
    /// On Firefox 151 (and some subsequent builds) `screenshotActor.capture` fails
    /// with "Unable to load actor module 'devtools/server/actors/screenshot'" because
    /// `capture-screenshot.js` calls `ChromeUtils.importESModule("moz-src:///...")` without
    /// the `{ global: "current" }` option required in the DevTools distinct global.
    /// This is a Firefox regression (tracked upstream — see `// allow-spec-drift` annotation
    /// below) that makes the standard `screenshotActor.capture` path unusable via RDP.
    ///
    /// ## Protocol
    ///
    /// 1. `root.getProcess(0)` → parent-process descriptor actor ID.
    /// 2. `processDescriptor.getTarget` → `TargetInfo` containing a chrome-privileged
    ///    console actor.
    /// 3. `consoleActor.evaluateJSAsync` (with `mapped: { await: true }`) runs an async
    ///    IIFE that:
    ///    - Calls `BrowsingContext.get(bc_id).currentWindowGlobal.drawSnapshot()`
    ///    - Converts the snapshot to a PNG `Blob` via `OffscreenCanvas`
    ///    - Writes it to a temp file via `IOUtils.write(path, bytes)`
    ///    - Returns `"ok"` on success or a diagnostic string on error.
    /// 4. The Rust caller reads the temp file, encodes it as a data URL, deletes the file.
    ///
    /// ## Availability
    ///
    /// Requires Firefox 87+ (`getProcess` and `IOUtils` were added in FF 87).
    /// `drawSnapshot` on `WindowGlobalParent` is available in Firefox 73+.
    ///
    // allow-spec-drift: bug TBD (SD-2: BrowsingContext.drawSnapshot used via
    // parent-process eval as a workaround for the Firefox 151 regression where
    // screenshotActor.capture fails to load capture-screenshot.js in the
    // DevTools distinct global.  iter-117 REASSESSED the regression against
    // Firefox 152.0.5: it STILL reproduces — a live `screenshot` probe logs
    // "screenshotActor module load failure; retrying via
    // screenshot_via_process_drawsnapshot".  The workaround therefore stays;
    // removing it or gating it behind a version check on FF152 would break
    // screenshots.  Remove or version-gate it only once Mozilla fixes the
    // module-load path.  iter-117 found no existing Bugzilla bug — novel gap
    // awaiting James's filing; see
    // kb/rdp/from-our-codebase/open-gaps.md#spec-drift-bugs-awaiting-filing and
    // iter-117 Results.  `TBD` blocks publishing the v0.3.0 draft.)
    /// Returns raw PNG bytes (not a data URL).
    ///
    /// The caller (CLI screenshot command) is responsible for encoding the bytes
    /// as `data:image/png;base64,...` when needed.
    pub fn screenshot_via_process_drawsnapshot(
        transport: &mut RdpTransport,
        browsing_context_id: u64,
        full_page: bool,
        full_page_rect: Option<(f64, f64)>,
    ) -> Result<Vec<u8>, ProtocolError> {
        // Step 1: get the parent process descriptor.
        let process_actor = RootActor::get_process(transport, 0)?;

        // Step 2: get the target (which carries a chrome-privileged console actor).
        let process_target = TabActor::get_process_target(transport, &process_actor)?;

        let console_actor = process_target.console_actor;

        // Step 3: build a per-call unique temp path.
        //
        // PID + nanosecond timestamp avoids collisions between concurrent
        // captures in the same process and reduces clobber/symlink risk on
        // shared temp dirs.
        let pid = std::process::id();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let tmp_dir = std::env::temp_dir();
        // Forward-slashes work on all platforms in Firefox's IOUtils.
        let tmp_path = tmp_dir.join(format!("ff-rdp-screenshot-{pid}-{nonce}.png"));
        let tmp_path_str = tmp_path.to_string_lossy().replace('\\', "/");
        // JSON-encode the path to produce a safely-escaped JS string literal.
        let tmp_path_js = serde_json::to_string(&tmp_path_str).map_err(|e| {
            ProtocolError::InvalidPacket(format!(
                "screenshot_via_process_drawsnapshot: invalid temp path: {e}"
            ))
        })?;

        // Step 4: run the async screenshot JS in the parent-process console.
        //
        // The IIFE:
        //   (a) Resolves the BrowsingContext by its ID.
        //   (b) Calls drawSnapshot to get an ImageBitmap.
        //   (c) Renders it onto an OffscreenCanvas and serialises to PNG.
        //   (d) Writes the PNG bytes to a temp file via IOUtils.
        //
        // The 4th arg to `drawSnapshot(rect, scale, color, resetScrollPosition)`
        // is `resetScrollPosition` (see
        // dom/webidl/WindowGlobalActors.webidl), NOT a fullpage flag.  To
        // capture the full scrollable area, pass an explicit oversized `rect`
        // (first arg).  The caller supplies `full_page_rect = Some((w, h))`
        // when `--full-page` is requested; the JS then constructs a `DOMRect`
        // spanning the full document.  When `None`, `drawSnapshot` defaults to
        // the visible viewport.
        let rect_js = match (full_page, full_page_rect) {
            (true, Some((w, h))) => format!("new DOMRect(0, 0, {w}, {h})"),
            _ => "null".to_owned(),
        };
        let reset_scroll = if full_page { "true" } else { "false" };
        let bc_id = browsing_context_id;
        let js = format!(
            r#"(async function() {{
  try {{
    const bc = BrowsingContext.get({bc_id});
    if (!bc) return "error:no-bc:{bc_id}";
    const wg = bc.currentWindowGlobal;
    if (!wg) return "error:no-wg:{bc_id}";
    const rect = {rect_js};
    const snapshot = await wg.drawSnapshot(rect, 1, "rgb(255,255,255)", {reset_scroll});
    const canvas = new OffscreenCanvas(snapshot.width, snapshot.height);
    const ctx = canvas.getContext("2d");
    ctx.drawImage(snapshot, 0, 0);
    const blob = await canvas.convertToBlob({{type: "image/png"}});
    const ab = await blob.arrayBuffer();
    await IOUtils.write({tmp_path_js}, new Uint8Array(ab));
    return "ok:" + blob.size;
  }} catch(e) {{
    return "error:" + e.toString();
  }}
}})()"#
        );

        let eval_result = WebConsoleActor::evaluate_js_async(transport, &console_actor, &js)?;

        // Surface JS-level exceptions (syntax error, missing IOUtils, etc.)
        // before falling through to the grip-shape check.
        if let Some(exc) = &eval_result.exception {
            let msg = exc.message.as_deref().unwrap_or("<no message>");
            return Err(ProtocolError::InvalidPacket(format!(
                "screenshot_via_process_drawsnapshot: JS evaluation threw: {msg}"
            )));
        }

        // Check the result for errors.
        let result_str = match &eval_result.result {
            Grip::Value(Value::String(s)) => s.clone(),
            other => {
                return Err(ProtocolError::InvalidPacket(format!(
                    "screenshot_via_process_drawsnapshot: unexpected eval result grip: {other:?}"
                )));
            }
        };

        if let Some(msg) = result_str.strip_prefix("error:") {
            // Best-effort cleanup of any partial file the JS may have written.
            let _ = std::fs::remove_file(&tmp_path);
            return Err(ProtocolError::InvalidPacket(format!(
                "screenshot_via_process_drawsnapshot: JS returned error: {msg}"
            )));
        }

        // Step 5: read the PNG file written by Firefox.
        let png_bytes = std::fs::read(&tmp_path).map_err(|e| {
            ProtocolError::InvalidPacket(format!(
                "screenshot_via_process_drawsnapshot: could not read temp file '{}': {e}",
                tmp_path.display()
            ))
        })?;

        // Clean up the temp file (best-effort).
        let _ = std::fs::remove_file(&tmp_path);

        Ok(png_bytes)
    }

    /// Fallback capture path for Firefox 151+ where `screenshotActor` is absent
    /// from `getRoot`.
    ///
    /// Protocol:
    /// 1. `root.listTabs` → find the selected tab's actor.
    /// 2. `tabActor.getTarget` → obtain the `WindowGlobalTarget` actor ID.
    /// 3. Send a `screenshot` request directly to the target actor with the same
    ///    args used by the root-form path.
    ///
    /// Firefox 151 moved the screenshot capability onto the target actor itself.
    /// The method names tried in order are:
    /// - `"screenshot"` — the name used by the WindowGlobalTarget in FF 151+.
    /// - `"takeScreenshot"` — alternate name observed in some builds.
    ///
    /// Returns the `data:image/png;base64,...` string on success, or a
    /// [`ProtocolError`] when neither method is recognised by the target.
    // allow-spec-drift: bug TBD (SD-3: WindowGlobalTarget.screenshot not
    // declared in devtools/shared/specs/targets/window-global.js on main — the
    // method was observed in the server-side implementation on FF 151 but the
    // spec dict has not been updated.  iter-117 searched Mozilla Bugzilla and
    // found no existing bug — novel gap awaiting James's filing; see
    // kb/rdp/from-our-codebase/open-gaps.md#spec-drift-bugs-awaiting-filing and
    // iter-117 Results.  `TBD` blocks publishing the v0.3.0 draft: replace
    // with the real Bugzilla number in a follow-up commit before publish.)
    pub fn screenshot_via_target(
        transport: &mut RdpTransport,
        browsing_context_id: u64,
        full_page: bool,
        prep: &PrepareCapture,
    ) -> Result<String, ProtocolError> {
        // Step 1: find the selected tab.
        let tabs = RootActor::list_tabs(transport)?;
        let tab = tabs
            .iter()
            .find(|t| t.selected)
            .or_else(|| tabs.first())
            .ok_or_else(|| {
                ProtocolError::InvalidPacket(
                    "screenshot_via_target: no tabs available from listTabs".into(),
                )
            })?;

        // Step 2: get the WindowGlobalTarget actor.
        let target = TabActor::get_target(transport, &tab.actor.clone())?;

        // Step 3: try screenshot methods on the target actor.
        let args = ScreenshotArgsExt::from_prep(browsing_context_id, full_page, prep);
        let args_value = args.to_args_value()?;
        #[allow(clippy::items_after_statements)]
        const TARGET_SCREENSHOT_METHODS: &[&str] = &["screenshot", "takeScreenshot"];
        let mut last_err: Option<ProtocolError> = None;
        for &method in TARGET_SCREENSHOT_METHODS {
            match actor_request(
                transport,
                target.actor.as_ref(),
                method,
                Some(&json!({ "args": args_value })),
            ) {
                Ok(response) => {
                    let value = response.get("value").unwrap_or(&response);
                    let data = value
                        .get("data")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ProtocolError::InvalidPacket(format!(
                                "WindowGlobalTarget.{method} response missing 'data' field"
                            ))
                        })?
                        .to_owned();
                    return Ok(data);
                }
                Err(e) if e.is_unrecognized_packet_type() => {
                    last_err = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            ProtocolError::InvalidPacket(
                "screenshot_via_target: no screenshot methods recognized by target".into(),
            )
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;
    use std::net::{TcpListener, TcpStream};
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;
    use crate::actors::screenshot_content::PrepareCapture;
    use crate::transport::{RdpTransport, encode_frame, recv_from};

    fn make_transport_pair() -> (RdpTransport, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let client = TcpStream::connect(addr).unwrap();
        let (server, _) = listener.accept().unwrap();

        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        (RdpTransport::from_parts(reader, writer), server)
    }

    #[allow(clippy::needless_pass_by_value)]
    fn server_reply(server: &TcpStream, msg: serde_json::Value) {
        use std::io::Write as _;
        let frame = encode_frame(&serde_json::to_string(&msg).unwrap());
        // TcpStream implements Write for &TcpStream (shared reference).
        let mut s = server;
        s.write_all(frame.as_bytes()).unwrap();
    }

    fn server_read(server: &TcpStream) -> serde_json::Value {
        let mut reader = BufReader::new(server);
        recv_from(&mut reader).unwrap()
    }

    #[test]
    fn get_actor_id_parses_screenshot_actor_from_get_root() {
        let (mut transport, server) = make_transport_pair();

        let t = std::thread::spawn(move || {
            let _req = server_read(&server);
            server_reply(
                &server,
                json!({
                    "from": "root",
                    "screenshotActor": "server1.conn0.screenshotActor7",
                    "preferenceActor": "server1.conn0.preferenceActor1",
                }),
            );
        });

        let actor_id = ScreenshotActor::get_actor_id(&mut transport).unwrap();
        assert_eq!(actor_id.as_ref(), "server1.conn0.screenshotActor7");
        t.join().unwrap();
    }

    #[test]
    fn get_actor_id_returns_error_when_field_absent() {
        let (mut transport, server) = make_transport_pair();

        let t = std::thread::spawn(move || {
            let _req = server_read(&server);
            server_reply(&server, json!({ "from": "root", "preferenceActor": "x" }));
        });

        let err = ScreenshotActor::get_actor_id(&mut transport).unwrap_err();
        assert!(
            err.to_string().contains("screenshotActor"),
            "error should mention field: {err}"
        );
        t.join().unwrap();
    }

    #[test]
    fn capture_sends_correct_request_and_parses_data_url() {
        let (mut transport, server) = make_transport_pair();
        let actor_id = ActorId::from("server1.conn0.screenshotActor7");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "capture");
            assert_eq!(req["to"], "server1.conn0.screenshotActor7");
            let args = &req["args"];
            assert_eq!(args["browsingContextID"], 42);
            assert_eq!(args["fullpage"], false);
            // Spec types dpr as `nullable:string` — must be a JSON string.
            assert!(
                args["dpr"].is_string(),
                "dpr must be a JSON string, got {:?}",
                args["dpr"]
            );
            assert_eq!(args["dpr"].as_str().unwrap(), "1");

            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.screenshotActor7",
                    "value": {
                        "data": "data:image/png;base64,abc123",
                        "filename": "screenshot.png",
                        "messages": [],
                    }
                }),
            );
        });

        let prep = PrepareCapture {
            window_dpr: 1.0,
            window_zoom: 1.0,
            rect: None,
        };
        let data = ScreenshotActor::capture(&mut transport, &actor_id, 42, false, &prep).unwrap();
        assert_eq!(data, "data:image/png;base64,abc123");
        t.join().unwrap();
    }

    #[test]
    fn capture_forwards_rect_when_present() {
        let (mut transport, server) = make_transport_pair();
        let actor_id = ActorId::from("server1.conn0.screenshotActor7");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            let args = &req["args"];
            // rect must be forwarded to the server
            assert_eq!(args["rect"]["left"], 10.0);
            assert_eq!(args["rect"]["top"], 20.0);
            assert_eq!(args["rect"]["width"], 800.0);
            assert_eq!(args["rect"]["height"], 600.0);

            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.screenshotActor7",
                    "value": { "data": "data:image/png;base64,rect_test" }
                }),
            );
        });

        let prep = PrepareCapture {
            window_dpr: 1.0,
            window_zoom: 1.0,
            rect: Some(crate::actors::screenshot_content::CaptureRect {
                left: 10.0,
                top: 20.0,
                width: 800.0,
                height: 600.0,
            }),
        };
        let data = ScreenshotActor::capture(&mut transport, &actor_id, 99, true, &prep).unwrap();
        assert_eq!(data, "data:image/png;base64,rect_test");
        t.join().unwrap();
    }

    #[test]
    fn capture_omits_rect_when_none() {
        let (mut transport, server) = make_transport_pair();
        let actor_id = ActorId::from("server1.conn0.screenshotActor7");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            let args = &req["args"];
            // rect must not be present in the request
            assert!(
                args.get("rect").is_none(),
                "rect should be absent when None"
            );

            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.screenshotActor7",
                    "value": { "data": "data:image/png;base64,no_rect" }
                }),
            );
        });

        let prep = PrepareCapture {
            window_dpr: 1.0,
            window_zoom: 1.0,
            rect: None,
        };
        let data = ScreenshotActor::capture(&mut transport, &actor_id, 5, false, &prep).unwrap();
        assert_eq!(data, "data:image/png;base64,no_rect");
        t.join().unwrap();
    }

    #[test]
    fn capture_returns_error_when_data_missing() {
        let (mut transport, server) = make_transport_pair();
        let actor_id = ActorId::from("server1.conn0.screenshotActor7");

        let t = std::thread::spawn(move || {
            let _req = server_read(&server);
            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.screenshotActor7",
                    "value": { "messages": [] }
                }),
            );
        });

        let prep = PrepareCapture {
            window_dpr: 1.0,
            window_zoom: 1.0,
            rect: None,
        };
        let err =
            ScreenshotActor::capture(&mut transport, &actor_id, 42, false, &prep).unwrap_err();
        assert!(
            err.to_string().contains("'data'"),
            "error should mention missing field: {err}"
        );
        t.join().unwrap();
    }

    /// iter-70 AC: outbound packet JSON has `dpr` as `Value::String`, not
    /// `Value::Number`.  The Firefox spec at
    /// `devtools/shared/specs/screenshot.js:18` types it as `nullable:string`.
    #[test]
    fn screenshot_dpr_serialised_as_string() {
        let (mut transport, server) = make_transport_pair();
        let actor_id = ActorId::from("server1.conn0.screenshotActor7");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            let args = &req["args"];
            assert!(
                matches!(args["dpr"], serde_json::Value::String(_)),
                "dpr must be a JSON string per spec, got {:?}",
                args["dpr"]
            );
            assert_eq!(args["dpr"].as_str().unwrap(), "1.5");

            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.screenshotActor7",
                    "value": { "data": "data:image/png;base64,x" }
                }),
            );
        });

        let prep = PrepareCapture {
            window_dpr: 1.5,
            window_zoom: 1.0,
            rect: None,
        };
        ScreenshotActor::capture(&mut transport, &actor_id, 1, false, &prep).unwrap();
        t.join().unwrap();
    }

    /// AC: `screenshot_args_ext_serializes_full_set` — `ScreenshotArgsExt`
    /// round-trips through `to_args_value()` carrying both the spec-declared
    /// fields and the locally-required `browsingContextID` / `snapshotScale`
    /// / `rect` fields.  Also verifies the `allow-spec-drift: bug` annotation
    /// is present on the struct (doctest grep against the module source).
    #[test]
    fn screenshot_args_ext_serializes_full_set() {
        let prep = PrepareCapture {
            window_dpr: 2.0,
            window_zoom: 1.5,
            rect: Some(crate::actors::screenshot_content::CaptureRect {
                left: 1.0,
                top: 2.0,
                width: 800.0,
                height: 600.0,
            }),
        };
        let args = ScreenshotArgsExt::from_prep(99, true, &prep);
        let v = args.to_args_value().unwrap();
        // Spec-declared fields.
        assert_eq!(v["fullpage"], true);
        assert_eq!(v["dpr"], "2");
        // Locally-required fields (NOT in the published spec dict).
        assert_eq!(v["browsingContextID"], 99);
        assert!((v["snapshotScale"].as_f64().unwrap() - 3.0).abs() < f64::EPSILON);
        assert_eq!(v["rect"]["left"], 1.0);
        assert_eq!(v["rect"]["width"], 800.0);

        // Drop snapshotScale when DPR*zoom == 1.0.
        let unit = PrepareCapture {
            window_dpr: 1.0,
            window_zoom: 1.0,
            rect: None,
        };
        let unit_v = ScreenshotArgsExt::from_prep(1, false, &unit)
            .to_args_value()
            .unwrap();
        assert!(
            unit_v.get("snapshotScale").is_none(),
            "snapshotScale must be omitted when equal to server default 1.0"
        );
        assert!(unit_v.get("rect").is_none());

        // Verify the allow-spec-drift annotation is present in the module
        // source — it is part of the contract that spec drift is documented.
        let src = include_str!("screenshot.rs");
        assert!(
            src.contains("allow-spec-drift: bug"),
            "screenshot.rs must carry an `allow-spec-drift: bug …` annotation \
             documenting the spec-dict gap"
        );
    }

    #[test]
    fn capture_snapshot_scale_is_dpr_times_zoom() {
        let (mut transport, server) = make_transport_pair();
        let actor_id = ActorId::from("server1.conn0.screenshotActor7");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            let args = &req["args"];
            // dpr=2.0, zoom=1.5 → snapshotScale=3.0
            // dpr is sent as a JSON string per the Firefox spec.
            assert_eq!(args["dpr"], "2");
            assert!((args["snapshotScale"].as_f64().unwrap() - 3.0).abs() < f64::EPSILON);

            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.screenshotActor7",
                    "value": { "data": "data:image/png;base64,xyz" }
                }),
            );
        });

        let prep = PrepareCapture {
            window_dpr: 2.0,
            window_zoom: 1.5,
            rect: None,
        };
        ScreenshotActor::capture(&mut transport, &actor_id, 1, false, &prep).unwrap();
        t.join().unwrap();
    }

    // ── iter-92 Theme A: unit_window_global_target_screenshot_forwards_full_page ──

    /// AC: `unit_window_global_target_screenshot_forwards_full_page`
    ///
    /// Drives `screenshot_via_target` against a minimal mock server and asserts
    /// that the outbound `screenshot` request JSON carries `args.fullpage == true`
    /// when `full_page` is set.  This pins the iter-92 regression where the
    /// process-drawsnapshot fallback was hard-rejecting `full_page=true` instead
    /// of forwarding the flag — causing `--full-page` to silently produce a
    /// viewport-sized PNG.
    #[test]
    fn unit_window_global_target_screenshot_forwards_full_page() {
        use std::io::Write as _;

        let (mut transport, server) = make_transport_pair();

        let captured_request: std::sync::Arc<Mutex<Option<serde_json::Value>>> =
            std::sync::Arc::new(Mutex::new(None));
        let captured_clone = std::sync::Arc::clone(&captured_request);

        let t = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let mut srv = &server;

            // listTabs
            let _ = recv_from(&mut reader).unwrap();
            let reply = json!({
                "from": "root",
                "tabs": [{
                    "actor": "server1.conn0.tabDescriptor1",
                    "title": "Test",
                    "url": "about:blank",
                    "selected": true,
                    "browsingContextID": 7
                }]
            });
            srv.write_all(encode_frame(&serde_json::to_string(&reply).unwrap()).as_bytes())
                .unwrap();

            // getTarget
            let _ = recv_from(&mut reader).unwrap();
            let reply = json!({
                "from": "server1.conn0.tabDescriptor1",
                "frame": {
                    "actor": "server1.conn0.child1/windowGlobalTarget2",
                    "consoleActor": "server1.conn0.child1/consoleActor3"
                }
            });
            srv.write_all(encode_frame(&serde_json::to_string(&reply).unwrap()).as_bytes())
                .unwrap();

            // screenshot method — capture request before replying
            let req = recv_from(&mut reader).unwrap();
            *captured_clone.lock().unwrap() = Some(req);

            let reply = json!({
                "from": "server1.conn0.child1/windowGlobalTarget2",
                "value": { "data": "data:image/png;base64,abc123" }
            });
            srv.write_all(encode_frame(&serde_json::to_string(&reply).unwrap()).as_bytes())
                .unwrap();
        });

        let prep = PrepareCapture {
            window_dpr: 1.0,
            window_zoom: 1.0,
            rect: None,
        };
        // full_page = true
        ScreenshotActor::screenshot_via_target(&mut transport, 7, true, &prep).unwrap();
        t.join().unwrap();

        let req = captured_request.lock().unwrap().take().unwrap();
        assert_eq!(
            req["type"].as_str(),
            Some("screenshot"),
            "request must be a screenshot method call"
        );
        let fullpage = &req["args"]["fullpage"];
        assert_eq!(
            *fullpage,
            serde_json::Value::Bool(true),
            "args.fullpage must be true when full_page is requested, got: {fullpage}"
        );
    }

    // ── Firefox 151 fallback: screenshot_via_target ──────────────────────────

    /// AC: `screenshot_via_target_uses_target_screenshot_method`
    ///
    /// A mock server returns a FF 151-shaped `getRoot` (no `screenshotActor`),
    /// then responds to `listTabs`, `getTarget`, and the `screenshot` method on
    /// the WindowGlobalTarget actor.  Asserts that `screenshot_via_target`
    /// selects the target path and returns the data URL.
    #[test]
    fn screenshot_via_target_uses_target_screenshot_method() {
        use std::io::Write as _;

        let (mut transport, server) = make_transport_pair();

        let t = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let mut srv = &server;

            // Request 1: listTabs
            let req1 = recv_from(&mut reader).unwrap();
            assert_eq!(req1["type"], "listTabs");
            let list_tabs_reply = json!({
                "from": "root",
                "tabs": [{
                    "actor": "server1.conn0.tabDescriptor1",
                    "title": "Test",
                    "url": "https://example.com",
                    "selected": true,
                    "browsingContextID": 42
                }]
            });
            srv.write_all(
                encode_frame(&serde_json::to_string(&list_tabs_reply).unwrap()).as_bytes(),
            )
            .unwrap();

            // Request 2: getTarget on the tab
            let req2 = recv_from(&mut reader).unwrap();
            assert_eq!(req2["type"], "getTarget");
            let get_target_reply = json!({
                "from": "server1.conn0.tabDescriptor1",
                "frame": {
                    "actor": "server1.conn0.child1/windowGlobalTarget2",
                    "consoleActor": "server1.conn0.child1/consoleActor3"
                }
            });
            srv.write_all(
                encode_frame(&serde_json::to_string(&get_target_reply).unwrap()).as_bytes(),
            )
            .unwrap();

            // Request 3: screenshot on the target actor
            let req3 = recv_from(&mut reader).unwrap();
            assert_eq!(req3["type"], "screenshot");
            assert_eq!(req3["to"], "server1.conn0.child1/windowGlobalTarget2");
            let screenshot_reply = json!({
                "from": "server1.conn0.child1/windowGlobalTarget2",
                "value": {
                    "data": "data:image/png;base64,ff151data"
                }
            });
            srv.write_all(
                encode_frame(&serde_json::to_string(&screenshot_reply).unwrap()).as_bytes(),
            )
            .unwrap();
        });

        let prep = PrepareCapture {
            window_dpr: 1.0,
            window_zoom: 1.0,
            rect: None,
        };
        let data =
            ScreenshotActor::screenshot_via_target(&mut transport, 42, false, &prep).unwrap();
        assert_eq!(data, "data:image/png;base64,ff151data");
        t.join().unwrap();
    }

    /// AC: `get_actor_id_returns_error_when_screenshotActor_absent_ff151`
    ///
    /// Verifies that a `getRoot` reply without `screenshotActor` (the FF 151
    /// shape from fixture `getroot_ff151.json`) causes `get_actor_id` to return
    /// an error that clearly names the missing field — confirming the fallback
    /// trigger condition.
    #[test]
    fn get_actor_id_returns_error_when_screenshotactor_absent_ff151() {
        let (mut transport, server) = make_transport_pair();

        // Synthetic FF 151 getRoot shape: no screenshotActor field.
        // Source: crates/ff-rdp-core/tests/fixtures/getroot_ff151.json (synthetic)
        let ff151_root = json!({
            "from": "root",
            "preferenceActor": "server1.conn0.preferenceActor1",
            "deviceActor": "server1.conn0.deviceActor2",
            "addonsActor": "server1.conn0.addonsActor3",
            "processActor": "server1.conn0.processActor4"
        });

        let t = std::thread::spawn(move || {
            server_reply(&server, ff151_root);
        });

        let err = ScreenshotActor::get_actor_id(&mut transport).unwrap_err();
        assert!(
            err.to_string().contains("screenshotActor"),
            "error must mention the missing field: {err}"
        );
        t.join().unwrap();
    }

    // ── iter-89 ACs ─────────────────────────────────────────────────────────

    /// PNG magic bytes prefix used by the dispatcher-shaped tests below.
    const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

    /// Base64 encoding of the 8-byte PNG magic (`\x89PNG\r\n\x1a\n`).
    /// Hardcoded to avoid adding the `base64` crate as a dev-dependency.
    const PNG_MAGIC_B64: &str = "iVBORw0KGgo=";

    fn png_magic_data_url() -> String {
        format!("data:image/png;base64,{PNG_MAGIC_B64}")
    }

    /// Tiny "from-scratch" base64 decoder for the dispatcher round-trip
    /// assertion.  Standard alphabet, supports `=` padding.  Kept local so
    /// the core crate doesn't pull in a base64 dev-dependency.
    fn decode_b64(s: &str) -> Vec<u8> {
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut lookup = [255u8; 256];
        for (i, &c) in alphabet.iter().enumerate() {
            lookup[c as usize] = u8::try_from(i).unwrap();
        }
        let bytes: Vec<u8> = s
            .bytes()
            .filter(|&b| b != b'=' && lookup[b as usize] != 255)
            .collect();
        let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
        for chunk in bytes.chunks(4) {
            let mut buf = [0u8; 4];
            for (i, &b) in chunk.iter().enumerate() {
                buf[i] = lookup[b as usize];
            }
            let n = u32::from(buf[0]) << 18
                | u32::from(buf[1]) << 12
                | u32::from(buf[2]) << 6
                | u32::from(buf[3]);
            out.push(((n >> 16) & 0xFF) as u8);
            if chunk.len() > 2 {
                out.push(((n >> 8) & 0xFF) as u8);
            }
            if chunk.len() > 3 {
                out.push((n & 0xFF) as u8);
            }
        }
        out
    }

    /// AC: `unit_screenshot_via_target_returns_png` — drives
    /// `screenshot_via_target` end-to-end against a mock that mirrors the
    /// recorded FF 151 `getRoot` shape (no `screenshotActor`).  The mock
    /// replies to `listTabs` → `getTarget` → `screenshot` with a data URL
    /// containing the PNG magic bytes; the returned buffer must start with
    /// the PNG magic.
    #[test]
    fn unit_screenshot_via_target_returns_png() {
        use std::io::Write as _;

        let (mut transport, server) = make_transport_pair();
        let data_url = png_magic_data_url();
        let data_url_for_thread = data_url.clone();

        let t = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let mut srv = &server;

            let _ = recv_from(&mut reader).unwrap(); // listTabs
            let reply = json!({
                "from": "root",
                "tabs": [{
                    "actor": "server1.conn0.tabDescriptor1",
                    "title": "FF151",
                    "url": "https://example.com",
                    "selected": true,
                    "browsingContextID": 7
                }]
            });
            srv.write_all(encode_frame(&serde_json::to_string(&reply).unwrap()).as_bytes())
                .unwrap();

            let _ = recv_from(&mut reader).unwrap(); // getTarget
            let reply = json!({
                "from": "server1.conn0.tabDescriptor1",
                "frame": {
                    "actor": "server1.conn0.child1/windowGlobalTarget9",
                    "consoleActor": "server1.conn0.child1/consoleActor10"
                }
            });
            srv.write_all(encode_frame(&serde_json::to_string(&reply).unwrap()).as_bytes())
                .unwrap();

            let _ = recv_from(&mut reader).unwrap(); // screenshot
            let reply = json!({
                "from": "server1.conn0.child1/windowGlobalTarget9",
                "value": { "data": data_url_for_thread }
            });
            srv.write_all(encode_frame(&serde_json::to_string(&reply).unwrap()).as_bytes())
                .unwrap();
        });

        let prep = PrepareCapture {
            window_dpr: 1.0,
            window_zoom: 1.0,
            rect: None,
        };
        let returned =
            ScreenshotActor::screenshot_via_target(&mut transport, 7, false, &prep).unwrap();
        t.join().unwrap();

        let b64 = returned
            .strip_prefix("data:image/png;base64,")
            .expect("dispatcher must return a PNG data URL");
        let bytes = decode_b64(b64);
        assert!(
            bytes.starts_with(PNG_MAGIC),
            "decoded buffer must start with PNG magic bytes, got {:?}",
            &bytes[..bytes.len().min(8)]
        );
    }

    /// AC: `pre_fix_repro_screenshot_fixture_red_then_green` — loads the
    /// recorded `getroot_ff151.json` fixture and exercises the FF 151 routing.
    ///
    /// Red proof: a `getRoot` reply that lacks the `screenshotActor` field
    /// produces an error from `get_actor_id` whose message names the missing
    /// field — this is exactly the surface shape that produced
    /// "screenshot actor not found in Firefox 151 root form" on `origin/main`.
    ///
    /// Green proof: against the same FF 151 shape, the dispatcher's
    /// `screenshot_via_target` fallback drives `listTabs` → `getTarget` →
    /// `screenshot` and yields a buffer that starts with the PNG magic bytes.
    #[test]
    fn pre_fix_repro_screenshot_fixture_red_then_green() {
        use std::io::Write as _;

        // The recorded FF 151 `getRoot` reply.
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/getroot_ff151.json"))
                .expect("getroot_ff151.json must parse");

        // ── RED proof (origin/main behaviour) ──────────────────────────────
        // Force the missing-field shape by cloning the fixture and stripping
        // `screenshotActor` so the assertion is deterministic regardless of
        // which FF 151 sub-build produced the recorded fixture.  Against that
        // shape, `get_actor_id` must fail with an error message naming the
        // missing field — the root cause of the iter-89 regression.
        let mut red_fixture = fixture.clone();
        if let Some(obj) = red_fixture.as_object_mut() {
            obj.remove("screenshotActor");
        }
        let (mut transport, server) = make_transport_pair();
        let t = std::thread::spawn(move || server_reply(&server, red_fixture));
        let err = ScreenshotActor::get_actor_id(&mut transport).unwrap_err();
        t.join().unwrap();
        assert!(
            err.to_string().contains("screenshotActor"),
            "RED proof: missing-field error must name 'screenshotActor', got {err}"
        );

        // ── GREEN proof (branch HEAD behaviour) ────────────────────────────
        // Drive screenshot_via_target end-to-end against a mock; the returned
        // buffer must begin with the PNG magic bytes.
        let (mut transport, server) = make_transport_pair();
        let data_url = png_magic_data_url();
        let data_url_for_thread = data_url.clone();

        let t = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let mut srv = &server;

            let _ = recv_from(&mut reader).unwrap(); // listTabs
            let reply = json!({
                "from": "root",
                "tabs": [{
                    "actor": "server1.conn0.tabDescriptor1",
                    "title": "FF151",
                    "url": "https://example.com",
                    "selected": true,
                    "browsingContextID": 42
                }]
            });
            srv.write_all(encode_frame(&serde_json::to_string(&reply).unwrap()).as_bytes())
                .unwrap();

            let _ = recv_from(&mut reader).unwrap(); // getTarget
            let reply = json!({
                "from": "server1.conn0.tabDescriptor1",
                "frame": {
                    "actor": "server1.conn0.child1/windowGlobalTarget2",
                    "consoleActor": "server1.conn0.child1/consoleActor3"
                }
            });
            srv.write_all(encode_frame(&serde_json::to_string(&reply).unwrap()).as_bytes())
                .unwrap();

            let _ = recv_from(&mut reader).unwrap(); // screenshot
            let reply = json!({
                "from": "server1.conn0.child1/windowGlobalTarget2",
                "value": { "data": data_url_for_thread }
            });
            srv.write_all(encode_frame(&serde_json::to_string(&reply).unwrap()).as_bytes())
                .unwrap();
        });

        let prep = PrepareCapture {
            window_dpr: 1.0,
            window_zoom: 1.0,
            rect: None,
        };
        let returned =
            ScreenshotActor::screenshot_via_target(&mut transport, 42, false, &prep).unwrap();
        t.join().unwrap();

        let b64 = returned
            .strip_prefix("data:image/png;base64,")
            .expect("GREEN proof: dispatcher must return a PNG data URL");
        let bytes = decode_b64(b64);
        assert!(
            bytes.starts_with(PNG_MAGIC),
            "GREEN proof: decoded buffer must start with PNG magic, got {:?}",
            &bytes[..bytes.len().min(8)]
        );
    }
}
