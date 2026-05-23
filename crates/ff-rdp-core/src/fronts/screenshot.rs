use crate::actors::screenshot_content::PrepareCapture;
use crate::error::ProtocolError;
use crate::registry::{Front, FrontKind, Registry};
use crate::specs::{call, screenshot as spec};
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// A typed handle to a Firefox `Screenshot` or `ScreenshotContent` actor.
///
/// Screenshot actors provide `capture` operations for taking page screenshots.
///
/// Creating a `ScreenshotFront` is O(1) and does not touch the network.
pub struct ScreenshotFront {
    id: ActorId,
    registry: Registry,
}

impl ScreenshotFront {
    /// Wrap an actor ID as a `ScreenshotFront` and register it in the registry.
    ///
    /// `target_root` should be the `WindowGlobalTarget` actor that owns this front.
    pub fn new(id: ActorId, registry: Registry, target_root: ActorId) -> Self {
        registry.register(id.clone(), FrontKind::Screenshot, Some(target_root));
        Self { id, registry }
    }

    /// Capture a screenshot via the root-level screenshot actor (Firefox 149+).
    ///
    /// This is the second step of the two-step protocol — the caller must first
    /// call `ScreenshotContentActor::prepare_capture` to obtain the `PrepareCapture`
    /// metadata, then call this method.
    ///
    /// Returns a `data:image/png;base64,...` string.
    pub fn capture(
        &self,
        transport: &mut RdpTransport,
        browsing_context_id: u64,
        full_page: bool,
        prep: &PrepareCapture,
    ) -> Result<String, ProtocolError> {
        let snapshot_scale = prep.window_dpr * prep.window_zoom;
        let rect = prep.rect.as_ref().map(|r| spec::request::CaptureRect {
            left: r.left,
            top: r.top,
            width: r.width,
            height: r.height,
        });
        // Encode DPR as a string per the Firefox spec (dpr is declared as `string`
        // in devtools/shared/specs/screenshot.js).  Skip 1.0 (the default) to
        // keep the packet minimal; Firefox defaults to 1.0 when the field is absent.
        let dpr_str = if (prep.window_dpr - 1.0).abs() < 1e-6 {
            None
        } else {
            Some(format!("{}", prep.window_dpr))
        };
        // Only send snapshotScale when it differs from the server default (1.0).
        let snapshot_scale_opt = if (snapshot_scale - 1.0).abs() < 1e-6 {
            None
        } else {
            Some(snapshot_scale)
        };
        let args = spec::request::Capture {
            args: spec::request::CaptureArgs {
                browsing_context_id,
                fullpage: full_page,
                dpr: dpr_str,
                snapshot_scale: snapshot_scale_opt,
                delay: None,
                rect,
            },
        };
        let reply = call::<spec::Capture>(transport, &self.id, &args)?;
        reply
            .value
            .map(|v| v.data)
            .filter(|d| !d.is_empty())
            .ok_or_else(|| {
                ProtocolError::InvalidPacket(
                    "screenshotActor capture response missing 'data' field".into(),
                )
            })
    }
}

impl Front for ScreenshotFront {
    fn id(&self) -> &ActorId {
        &self.id
    }

    fn registry(&self) -> &Registry {
        &self.registry
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;
    use std::net::{TcpListener, TcpStream};

    use serde_json::json;

    use super::*;
    use crate::actors::screenshot_content::CaptureRect;
    use crate::registry::Registry;
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
        let mut s = server;
        s.write_all(frame.as_bytes()).unwrap();
    }

    fn server_read(server: &TcpStream) -> serde_json::Value {
        let mut reader = BufReader::new(server);
        recv_from(&mut reader).unwrap()
    }

    #[test]
    fn capture_sends_correct_request_and_returns_data_url() {
        let (mut transport, server) = make_transport_pair();
        let front = ScreenshotFront::new(
            ActorId::from("server1.conn0.screenshotActor7"),
            Registry::default(),
            ActorId::from("server1.conn0.child1/windowGlobalTarget1"),
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "capture");
            assert_eq!(req["args"]["browsingContextID"], 42);
            assert_eq!(req["args"]["fullpage"], false);
            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.screenshotActor7",
                    "value": {"data": "data:image/png;base64,abc123", "filename": "screenshot.png"}
                }),
            );
        });

        let prep = PrepareCapture {
            window_dpr: 1.0,
            window_zoom: 1.0,
            rect: None,
        };
        let data = front.capture(&mut transport, 42, false, &prep).unwrap();
        assert_eq!(data, "data:image/png;base64,abc123");
        t.join().unwrap();
    }

    #[test]
    fn capture_forwards_rect_when_present() {
        let (mut transport, server) = make_transport_pair();
        let front = ScreenshotFront::new(
            ActorId::from("server1.conn0.screenshotActor7"),
            Registry::default(),
            ActorId::from("server1.conn0.child1/windowGlobalTarget1"),
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["args"]["rect"]["width"], 800.0);
            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.screenshotActor7",
                    "value": {"data": "data:image/png;base64,xyz"}
                }),
            );
        });

        let prep = PrepareCapture {
            window_dpr: 1.0,
            window_zoom: 1.0,
            rect: Some(CaptureRect {
                left: 0.0,
                top: 0.0,
                width: 800.0,
                height: 600.0,
            }),
        };
        front.capture(&mut transport, 1, true, &prep).unwrap();
        t.join().unwrap();
    }
}
