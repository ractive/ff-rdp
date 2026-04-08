use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::actors::screenshot_content::PrepareCapture;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::ActorId;

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
pub struct ScreenshotActor;

impl ScreenshotActor {
    /// Obtain the `screenshotActor` ID from the root actor's `getRoot` response.
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
        let snapshot_scale = prep.window_dpr * prep.window_zoom;

        let args = json!({
            "browsingContextID": browsing_context_id,
            "fullpage": full_page,
            "dpr": prep.window_dpr,
            "snapshotScale": snapshot_scale,
        });

        let response = actor_request(
            transport,
            screenshot_actor.as_ref(),
            "capture",
            Some(&json!({ "args": args })),
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
        };
        let data = ScreenshotActor::capture(&mut transport, &actor_id, 42, false, &prep).unwrap();
        assert_eq!(data, "data:image/png;base64,abc123");
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
        };
        let err =
            ScreenshotActor::capture(&mut transport, &actor_id, 42, false, &prep).unwrap_err();
        assert!(
            err.to_string().contains("'data'"),
            "error should mention missing field: {err}"
        );
        t.join().unwrap();
    }

    #[test]
    fn capture_snapshot_scale_is_dpr_times_zoom() {
        let (mut transport, server) = make_transport_pair();
        let actor_id = ActorId::from("server1.conn0.screenshotActor7");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            let args = &req["args"];
            // dpr=2.0, zoom=1.5 → snapshotScale=3.0
            assert_eq!(args["dpr"], 2.0);
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
        };
        ScreenshotActor::capture(&mut transport, &actor_id, 1, false, &prep).unwrap();
        t.join().unwrap();
    }
}
