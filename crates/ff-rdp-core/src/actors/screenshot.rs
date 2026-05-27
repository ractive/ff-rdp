use serde::Serialize;
use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::actor::actor_send;
use crate::actors::root::RootActor;
use crate::actors::screenshot_content::PrepareCapture;
use crate::actors::tab::TabActor;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::ActorId;

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
// allow-spec-drift: bug TBD (Mozilla Bugzilla entry to be filed in a follow-up
// iter — screenshot.args dict at devtools/shared/specs/screenshot.js:13-35
// omits browsingContextID/snapshotScale/rect even though the server in
// devtools/server/actors/screenshot.js reads all three).  Per the `TBD`
// rule in CLAUDE.md, this annotation MUST be replaced with the real
// Bugzilla number before the next release cut; tracked via iter-78.
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
    pub fn to_args_value(&self) -> Value {
        serde_json::to_value(self).expect("ScreenshotArgsExt is Serialize-safe")
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
        let response = actor_request(
            transport,
            screenshot_actor.as_ref(),
            "capture",
            Some(&json!({ "args": args.to_args_value() })),
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
        actor_send(
            transport,
            screenshot_actor,
            "capture",
            Some(&json!({ "args": args.to_args_value() })),
        )
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
    ///
    // allow-spec-drift: bug TBD (WindowGlobalTarget.screenshot not declared in
    // devtools/shared/specs/targets/window-global.js on main — the method was
    // observed in the server-side implementation on FF 151 but the spec dict
    // has not been updated.  This annotation must be replaced once the Bugzilla
    // number is available.)
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
        #[allow(clippy::items_after_statements)]
        const TARGET_SCREENSHOT_METHODS: &[&str] = &["screenshot", "takeScreenshot"];
        let mut last_err: Option<ProtocolError> = None;
        for &method in TARGET_SCREENSHOT_METHODS {
            match actor_request(
                transport,
                target.actor.as_ref(),
                method,
                Some(&json!({ "args": args.to_args_value() })),
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
        Err(last_err.expect("TARGET_SCREENSHOT_METHODS is non-empty"))
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;
    use std::net::{TcpListener, TcpStream};

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
        let v = args.to_args_value();
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
        let unit_v = ScreenshotArgsExt::from_prep(1, false, &unit).to_args_value();
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
}
