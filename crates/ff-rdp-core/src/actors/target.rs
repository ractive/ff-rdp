use serde_json::json;

use crate::actor::{actor_request, actor_send};
use crate::error::ProtocolError;
use crate::transport::{RdpTransport, recv_reply_from};
use crate::types::ActorId;

/// Operations on a WindowGlobalTarget actor (navigation, reload, etc.).
pub struct WindowGlobalTarget;

impl WindowGlobalTarget {
    /// Read the live top document URL from this WindowGlobal actor. Unlike its
    /// availability form, listFrames reflects fragment/history changes. The
    /// reply is actor-scoped and child frames must not supply the top URL.
    pub fn current_url(
        transport: &mut RdpTransport,
        target_actor: &ActorId,
    ) -> Result<Option<String>, ProtocolError> {
        actor_send(transport, target_actor.as_ref(), "listFrames", None)?;
        let reply = match recv_reply_from(transport, target_actor.as_ref()) {
            Err(error @ (ProtocolError::Timeout | ProtocolError::EvalTargetDestroyed { .. })) => {
                // listFrames has no correlation ID. A late reply to this
                // abandoned request cannot become a later request's metadata.
                transport.abandon_reply(target_actor.as_ref());
                return Err(error);
            }
            result => result?,
        };
        Ok(reply
            .get("frames")
            .and_then(serde_json::Value::as_array)
            .and_then(|frames| {
                frames.iter().find(|frame| {
                    frame["isTopLevel"] == true
                        && frame.get("parentID").is_none_or(serde_json::Value::is_null)
                })
            })
            .and_then(|frame| frame["url"].as_str())
            .map(str::to_owned))
    }

    /// Navigate to the given URL.
    pub fn navigate_to(
        transport: &mut RdpTransport,
        target_actor: &ActorId,
        url: &str,
    ) -> Result<(), ProtocolError> {
        let params = json!({"url": url});
        actor_request(
            transport,
            target_actor.as_ref(),
            "navigateTo",
            Some(&params),
        )?;
        Ok(())
    }

    /// Reload the current page.
    ///
    /// When `force` is true, sends `{options: {force: true}}` so Firefox
    /// bypasses the HTTP cache (equivalent to a hard reload / Cmd-Shift-R).
    pub fn reload(
        transport: &mut RdpTransport,
        target_actor: &ActorId,
        force: bool,
    ) -> Result<(), ProtocolError> {
        if force {
            let params = json!({"options": {"force": true}});
            actor_request(transport, target_actor.as_ref(), "reload", Some(&params))?;
        } else {
            actor_request(transport, target_actor.as_ref(), "reload", None)?;
        }
        Ok(())
    }

    /// Go back in browser history.
    pub fn go_back(
        transport: &mut RdpTransport,
        target_actor: &ActorId,
    ) -> Result<(), ProtocolError> {
        actor_request(transport, target_actor.as_ref(), "goBack", None)?;
        Ok(())
    }

    /// Go forward in browser history.
    pub fn go_forward(
        transport: &mut RdpTransport,
        target_actor: &ActorId,
    ) -> Result<(), ProtocolError> {
        actor_request(transport, target_actor.as_ref(), "goForward", None)?;
        Ok(())
    }
}

#[cfg(test)]
mod current_url_tests {
    use super::*;
    use crate::transport::{encode_frame, recv_from};
    use std::io::{BufReader, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    #[test]
    fn interrupted_current_url_retires_late_reply_before_same_actor_request() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut send = |packet: serde_json::Value| {
                socket
                    .write_all(encode_frame(&packet.to_string()).as_bytes())
                    .unwrap();
            };
            assert_eq!(recv_from(&mut reader).unwrap()["type"], "listFrames");
            send(
                json!({"from":"watcher","type":"target-destroyed-form","target":{"innerWindowId":7}}),
            );
            assert_eq!(recv_from(&mut reader).unwrap()["type"], "listFrames");
            send(
                json!({"from":"target","frames":[{"id":1,"isTopLevel":true,"url":"https://WRONG/"}]}),
            );
            send(
                json!({"from":"target","frames":[{"id":1,"isTopLevel":true,"url":"https://right/"}]}),
            );
        });
        let mut transport =
            RdpTransport::connect_raw("127.0.0.1", port, Duration::from_secs(2)).unwrap();
        transport.set_target_guard(Some(7));
        assert!(matches!(
            WindowGlobalTarget::current_url(&mut transport, &"target".into()),
            Err(ProtocolError::EvalTargetDestroyed { inner_window_id: 7 })
        ));
        transport.set_target_guard(None);
        let next = WindowGlobalTarget::current_url(&mut transport, &"target".into()).unwrap();
        peer.join().unwrap();
        assert_eq!(next.as_deref(), Some("https://right/"));
    }

    #[test]
    fn current_url_uses_actor_scoped_top_frame() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            assert_eq!(
                recv_from(&mut reader).unwrap(),
                json!({"to":"target-a","type":"listFrames"})
            );
            for reply in [
                json!({"from":"target-b","frames":[{"isTopLevel":true,"url":"https://wrong/"}]}),
                json!({"from":"target-a","frames":[
                    {"id":9,"parentID":1,"isTopLevel":false,"url":"https://child/"},
                    {"id":1,"isTopLevel":true,"url":"https://a/#here"}]}),
            ] {
                stream
                    .write_all(encode_frame(&reply.to_string()).as_bytes())
                    .unwrap();
            }
        });
        let mut transport =
            RdpTransport::connect_raw("127.0.0.1", port, Duration::from_secs(2)).unwrap();
        assert_eq!(
            WindowGlobalTarget::current_url(&mut transport, &"target-a".into())
                .unwrap()
                .as_deref(),
            Some("https://a/#here")
        );
        server.join().unwrap();
    }
}
