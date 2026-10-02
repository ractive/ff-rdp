use serde_json::Value;

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use serde_json::json;

/// Information about a loaded JavaScript/WASM source.
#[derive(Debug, Clone)]
pub struct SourceInfo {
    /// The source actor ID.
    pub actor: String,
    /// URL of the source (may be empty for eval'd code).
    pub url: String,
    /// Whether the source is black-boxed (skipped during debugging).
    pub is_black_boxed: bool,
}

/// Source enumeration and explicit debugger operations on a ThreadActor.
///
/// On current Firefox, attaching enables debugging without pausing execution.
/// Enumerating sources must not resume a pause owned by another debugger.
pub struct ThreadActor;

impl ThreadActor {
    /// Enable the thread and consume its ordinary method reply.
    ///
    /// Firefox requires an `options` object, even when no options are requested.
    /// A fresh thread becomes running; an already attached thread keeps its
    /// state, including an independently established pause. A `paused` event is
    /// not attach completion. Interleaved events go to the transport's existing
    /// event sink, which callers consuming events must install before this call.
    pub fn attach(
        transport: &mut RdpTransport,
        thread_actor: &str,
    ) -> Result<Value, ProtocolError> {
        actor_request(
            transport,
            thread_actor,
            "attach",
            Some(&json!({"options": {}})),
        )
    }

    /// List all sources loaded in the thread.
    ///
    /// The thread must be attached (call [`Self::attach`] first). Both running
    /// and paused threads can enumerate sources; this method changes neither state.
    pub fn sources(
        transport: &mut RdpTransport,
        thread_actor: &str,
    ) -> Result<Vec<SourceInfo>, ProtocolError> {
        let response = actor_request(transport, thread_actor, "sources", None)?;
        let sources = response
            .get("sources")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(parse_source_info).collect())
            .unwrap_or_default();
        Ok(sources)
    }

    /// Resume the thread, transitioning from Paused to Running.
    ///
    /// Only call this when the caller owns the pause. Attaching or enumerating
    /// sources does not create a pause that needs resuming.
    pub fn resume(
        transport: &mut RdpTransport,
        thread_actor: &str,
    ) -> Result<Value, ProtocolError> {
        actor_request(transport, thread_actor, "resume", None)
    }

    /// Legacy detach request, unsupported by current Firefox.
    ///
    /// Source enumeration does not call this method. Close an owned direct
    /// connection to release its actors instead of sending unsupported detach.
    #[deprecated(note = "current Firefox has no thread detach method; close the owned connection")]
    pub fn detach(
        transport: &mut RdpTransport,
        thread_actor: &str,
    ) -> Result<Value, ProtocolError> {
        actor_request(transport, thread_actor, "detach", None)
    }

    /// Attach and enumerate sources without changing debugger pause ownership.
    ///
    /// Neither success nor failure resumes the thread or sends unsupported
    /// detach. The caller owns the transport and its connection lifetime.
    pub fn list_sources(
        transport: &mut RdpTransport,
        thread_actor: &str,
    ) -> Result<Vec<SourceInfo>, ProtocolError> {
        Self::attach(transport, thread_actor)?;
        Self::sources(transport, thread_actor)
    }
}

/// Parse a single source entry from the `sources` response array.
///
/// Returns `None` for null entries and non-object values, which some
/// Firefox builds may include in the sources array.
fn parse_source_info(value: &Value) -> Option<SourceInfo> {
    // Skip null or non-object entries that may appear in some Firefox versions.
    let _ = value.as_object()?;
    let actor = value.get("actor")?.as_str()?.to_owned();
    let url = value
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let is_black_boxed = value
        .get("isBlackBoxed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(SourceInfo {
        actor,
        url,
        is_black_boxed,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // Synthetic protocol peer, not a recorded Firefox fixture. Replies are
    // preloaded so restoring the old paused-event wait fails within the socket
    // bound; packet assertions independently detect resume/detach restoration.
    type Exchange = (
        Vec<Result<Vec<SourceInfo>, ProtocolError>>,
        Vec<Value>,
        Vec<Value>,
    );

    fn exchange(replies: &[Value], calls: usize) -> Exchange {
        use std::io::{BufReader, Write};
        use std::net::{TcpListener, TcpStream};
        use std::time::Duration;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let mut transport =
            RdpTransport::from_parts(BufReader::new(client.try_clone().unwrap()), client);
        let (tx, rx) = std::sync::mpsc::channel();
        transport.set_event_sink(Some(tx));
        for reply in replies {
            peer.write_all(crate::transport::encode_frame(&reply.to_string()).as_bytes())
                .unwrap();
        }
        let results = (0..calls)
            .map(|_| ThreadActor::list_sources(&mut transport, "thread1"))
            .collect();
        drop(transport);
        let mut requests = Vec::new();
        let mut reader = BufReader::new(peer);
        loop {
            match crate::transport::recv_from(&mut reader) {
                Ok(packet) => requests.push(packet),
                Err(ProtocolError::RecvFailed(err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(err) => panic!("unexpected peer receive error: {err}"),
            }
        }
        (results, requests, rx.try_iter().collect())
    }

    fn assert_native_requests(requests: &[Value]) {
        assert_eq!(
            requests,
            &[
                json!({"to": "thread1", "type": "attach", "options": {}}),
                json!({"to": "thread1", "type": "sources"}),
            ]
        );
    }

    #[test]
    fn list_sources_ordinary_attach_without_paused_or_cleanup() {
        let (results, requests, events) = exchange(
            &[
                json!({"from": "thread1"}),
                json!({"from": "thread1", "sources": [
                    {"actor": "source1", "url": "http://fixture/iteration286.js"}
                ]}),
            ],
            1,
        );
        let sources = results[0].as_ref().unwrap();
        assert_eq!(sources[0].actor, "source1");
        assert_eq!(sources[0].url, "http://fixture/iteration286.js");
        assert_native_requests(&requests);
        assert!(events.is_empty());
    }

    #[test]
    fn list_sources_already_attached_running_is_repeatable() {
        let (results, requests, events) = exchange(
            &[
                json!({"from": "thread1"}),
                json!({"from": "thread1", "sources": []}),
                json!({"from": "thread1"}),
                json!({"from": "thread1", "sources": []}),
            ],
            2,
        );
        assert!(results.iter().all(Result::is_ok));
        assert_eq!(requests.len(), 4);
        assert_native_requests(&requests[..2]);
        assert_native_requests(&requests[2..]);
        assert!(events.is_empty());
    }

    #[test]
    fn list_sources_preserves_external_pause_and_interleaved_events() {
        let paused =
            json!({"from": "thread1", "type": "paused", "why": {"type": "debuggerStatement"}});
        let sibling = json!({"from": "watcher1", "type": "resources-available-array", "array": []});
        let source =
            json!({"from": "thread1", "type": "newSource", "source": {"actor": "source1"}});
        let (results, requests, events) = exchange(
            &[
                paused.clone(),
                sibling.clone(),
                json!({"from": "thread1"}),
                source.clone(),
                json!({"from": "thread1", "sources": []}),
            ],
            1,
        );
        assert!(results[0].is_ok());
        assert_native_requests(&requests);
        assert_eq!(events, vec![paused, sibling, source]);
    }

    #[test]
    fn list_sources_attach_error_is_returned_without_sources_or_cleanup() {
        let (results, requests, events) = exchange(
            &[json!({"from": "thread1", "error": "wrongState", "message": "exited"})],
            1,
        );
        assert!(
            matches!(&results[0], Err(ProtocolError::ActorError { error, .. }) if error == "wrongState")
        );
        assert_eq!(
            requests,
            vec![json!({"to": "thread1", "type": "attach", "options": {}})]
        );
        assert!(events.is_empty());
    }

    #[test]
    fn list_sources_source_error_preserves_pause_without_cleanup() {
        let paused = json!({"from": "thread1", "type": "paused", "why": {"type": "breakpoint"}});
        let (results, requests, events) = exchange(
            &[
                json!({"from": "thread1"}),
                paused.clone(),
                json!({"from": "thread1", "error": "sourceUnavailable", "message": "fixture failure"}),
            ],
            1,
        );
        assert!(
            matches!(&results[0], Err(ProtocolError::ActorError { error, message, .. }) if error == "sourceUnavailable" && message == "fixture failure")
        );
        assert_native_requests(&requests);
        assert_eq!(events, vec![paused]);
    }

    #[test]
    fn parse_source_info_valid() {
        let value = json!({
            "actor": "server1.conn0.child1/sourceActor42",
            "url": "https://example.com/app.js",
            "isBlackBoxed": false
        });
        let info = parse_source_info(&value).unwrap();
        assert_eq!(info.actor, "server1.conn0.child1/sourceActor42");
        assert_eq!(info.url, "https://example.com/app.js");
        assert!(!info.is_black_boxed);
    }

    #[test]
    fn parse_source_info_black_boxed() {
        let value = json!({
            "actor": "server1.conn0.child1/sourceActor10",
            "url": "https://example.com/vendor.min.js",
            "isBlackBoxed": true
        });
        let info = parse_source_info(&value).unwrap();
        assert!(info.is_black_boxed);
    }

    #[test]
    fn parse_source_info_missing_url_defaults_to_empty() {
        // Eval'd code may have no URL field at all.
        let value = json!({
            "actor": "server1.conn0.child1/sourceActor99"
        });
        let info = parse_source_info(&value).unwrap();
        assert_eq!(info.url, "");
        assert!(!info.is_black_boxed);
    }

    #[test]
    fn parse_source_info_null_url_defaults_to_empty() {
        let value = json!({
            "actor": "server1.conn0.child1/sourceActor99",
            "url": null
        });
        let info = parse_source_info(&value).unwrap();
        assert_eq!(info.url, "");
    }

    #[test]
    fn parse_source_info_missing_actor_returns_none() {
        // actor is required; without it we cannot address the source.
        let value = json!({
            "url": "https://example.com/app.js",
            "isBlackBoxed": false
        });
        assert!(parse_source_info(&value).is_none());
    }

    #[test]
    fn parse_source_info_null_entry() {
        assert!(parse_source_info(&Value::Null).is_none());
    }

    #[test]
    fn parse_source_info_non_object_entry() {
        assert!(parse_source_info(&serde_json::json!("string")).is_none());
    }

    #[test]
    fn parse_source_info_missing_is_black_boxed_defaults_to_false() {
        let value = json!({
            "actor": "server1.conn0.child1/sourceActor1",
            "url": "https://example.com/main.js"
        });
        let info = parse_source_info(&value).unwrap();
        assert!(!info.is_black_boxed);
    }

    #[test]
    fn sources_response_empty_array() {
        // Verify that a "sources": [] response yields an empty Vec without error.
        // We use parse_source_info directly since wiring up a full mock transport
        // is done in actor.rs tests.
        let empty: Vec<SourceInfo> = json!([])
            .as_array()
            .unwrap()
            .iter()
            .filter_map(parse_source_info)
            .collect();
        assert!(empty.is_empty());
    }

    #[test]
    fn sources_response_multiple() {
        let arr = json!([
            {"actor": "server1.conn0.child1/src1", "url": "https://a.com/a.js", "isBlackBoxed": false},
            {"actor": "server1.conn0.child1/src2", "url": "https://a.com/b.js", "isBlackBoxed": true},
            {"actor": "server1.conn0.child1/src3", "url": "",                   "isBlackBoxed": false},
        ]);
        let infos: Vec<SourceInfo> = arr
            .as_array()
            .unwrap()
            .iter()
            .filter_map(parse_source_info)
            .collect();
        assert_eq!(infos.len(), 3);
        assert_eq!(infos[1].actor, "server1.conn0.child1/src2");
        assert!(infos[1].is_black_boxed);
        assert_eq!(infos[2].url, "");
    }
}
