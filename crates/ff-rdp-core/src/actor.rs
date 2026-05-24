use serde_json::{Value, json};

use crate::error::ProtocolError;
use crate::transport::{RdpTransport, recv_reply_from};

/// Send a fire-and-forget request to a named actor without reading any reply.
///
/// Used for methods marked `oneway: true` in the Firefox spec (e.g. `unwatchTargets`,
/// `clearResources`).  Builds the packet, sends it, and returns immediately.
pub fn actor_send(
    transport: &mut RdpTransport,
    to: &str,
    method: &str,
    params: Option<&Value>,
) -> Result<(), ProtocolError> {
    let mut request = params.cloned().unwrap_or_else(|| json!({}));
    let obj = request.as_object_mut().ok_or_else(|| {
        ProtocolError::InvalidPacket("actor request params must be a JSON object".into())
    })?;
    obj.insert("to".into(), json!(to));
    obj.insert("type".into(), json!(method));
    transport.send(&request)?;
    Ok(())
}

/// Send a request to a named actor and return the response.
///
/// Builds a message with the required `to` and `type` fields, merges any
/// extra `params`, sends it, reads the reply, and checks for actor-level
/// errors.
pub fn actor_request(
    transport: &mut RdpTransport,
    to: &str,
    method: &str,
    params: Option<&Value>,
) -> Result<Value, ProtocolError> {
    let mut request = params.cloned().unwrap_or_else(|| json!({}));

    // Ensure the request is an object and set required fields.
    let obj = request.as_object_mut().ok_or_else(|| {
        ProtocolError::InvalidPacket("actor request params must be a JSON object".into())
    })?;
    obj.insert("to".into(), json!(to));
    obj.insert("type".into(), json!(method));

    transport.send(&request)?;

    // Per `kb/rdp/protocol/message-format.md`: replies have no `type` field;
    // any `from`+`type` packet is an event. `recv_reply_from` enforces that
    // rule and routes stray events through the transport's event sink (see
    // `RdpTransport::set_event_sink`) instead of mis-classifying them.
    recv_reply_from(transport, to)
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Write};
    use std::net::{TcpListener, TcpStream};

    use super::*;
    use crate::error::ActorErrorKind;
    use crate::transport::{encode_frame, recv_from};

    #[test]
    fn actor_request_builds_correct_message() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let client = TcpStream::connect(addr).unwrap();
        let (accept, _) = listener.accept().unwrap();

        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        let mut transport = RdpTransport::from_parts(reader, writer);

        // Server: read request, send response
        let server = std::thread::spawn(move || {
            let mut srv_reader = BufReader::new(&accept);
            let request = recv_from(&mut srv_reader).unwrap();

            // Verify the request shape
            assert_eq!(request["to"], "root");
            assert_eq!(request["type"], "listTabs");

            let resp = json!({"from": "root", "tabs": []});
            let frame = encode_frame(&serde_json::to_string(&resp).unwrap());
            (&accept).write_all(frame.as_bytes()).unwrap();
        });

        let response = actor_request(&mut transport, "root", "listTabs", None).unwrap();
        assert_eq!(response["from"], "root");
        assert!(response["tabs"].is_array());

        server.join().unwrap();
    }

    #[test]
    fn actor_request_detects_error_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let client = TcpStream::connect(addr).unwrap();
        let (accept, _) = listener.accept().unwrap();

        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        let mut transport = RdpTransport::from_parts(reader, writer);

        let server = std::thread::spawn(move || {
            let mut srv_reader = BufReader::new(&accept);
            let _request = recv_from(&mut srv_reader).unwrap();

            let resp = json!({
                "from": "root",
                "error": "unknownError",
                "message": "something went wrong"
            });
            let frame = encode_frame(&serde_json::to_string(&resp).unwrap());
            (&accept).write_all(frame.as_bytes()).unwrap();
        });

        let err = actor_request(&mut transport, "root", "badMethod", None).unwrap_err();

        match err {
            ProtocolError::ActorError {
                actor,
                kind,
                error,
                message,
            } => {
                assert_eq!(actor, "root");
                assert_eq!(kind, ActorErrorKind::UnknownError);
                assert_eq!(error, "unknownError");
                assert_eq!(message, "something went wrong");
            }
            other => panic!("expected ActorError, got {other:?}"),
        }

        server.join().unwrap();
    }

    #[test]
    fn actor_request_skips_unsolicited_events() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let client = TcpStream::connect(addr).unwrap();
        let (accept, _) = listener.accept().unwrap();

        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        let mut transport = RdpTransport::from_parts(reader, writer);

        // Server: read request, send two unsolicited events, then the real response
        let server = std::thread::spawn(move || {
            let mut srv_reader = BufReader::new(&accept);
            let _request = recv_from(&mut srv_reader).unwrap();

            // Unsolicited event from a different actor
            let event1 = json!({"from": "server1.conn0.child1/tab1", "type": "tabNavigated"});
            let frame1 = encode_frame(&serde_json::to_string(&event1).unwrap());
            (&accept).write_all(frame1.as_bytes()).unwrap();

            // Another event with no "from" at all
            let event2 = json!({"type": "tabListChanged"});
            let frame2 = encode_frame(&serde_json::to_string(&event2).unwrap());
            (&accept).write_all(frame2.as_bytes()).unwrap();

            // The actual response
            let resp = json!({"from": "root", "tabs": [{"url": "about:blank"}]});
            let frame3 = encode_frame(&serde_json::to_string(&resp).unwrap());
            (&accept).write_all(frame3.as_bytes()).unwrap();
        });

        let response = actor_request(&mut transport, "root", "listTabs", None).unwrap();
        assert_eq!(response["from"], "root");
        assert_eq!(response["tabs"][0]["url"], "about:blank");

        server.join().unwrap();
    }
}
