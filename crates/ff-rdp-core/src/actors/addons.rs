//! Operations on the Firefox `AddonsActor` — temporary WebExtension installs
//! and uninstalls.
//!
//! The same RDP path about:debugging and `web-ext run` use: the extension
//! needs no signing (`XPIDatabase.sys.mjs` `isCorrectlySigned` returns `true`
//! for the temporary install location) and is gone when Firefox exits.
//!
//! - Spec: `devtools/shared/specs/addon/addons.js:12-31`
//! - Server: `devtools/server/actors/addon/addons.js:23-89`
//! - Actor ID: `addonsActor` in the root `getRoot` reply (a global actor,
//!   `devtools/server/actors/utils/actor-registry.js:129-133`).
//!
//! See `kb/rdp/actors/addons.md`.

use serde_json::Value;

use crate::actors::root::RootActor;
use crate::error::ProtocolError;
use crate::specs::{addons as spec, call};
use crate::transport::RdpTransport;
use crate::types::ActorId;

pub use crate::specs::addons::response::InstalledAddon;

/// Operations on the Firefox Addons actor.
pub struct AddonsActor;

impl AddonsActor {
    /// The `addonsActor` ID from the root actor's `getRoot` reply.
    ///
    /// Present on every desktop Firefox build ff-rdp supports; an absent field
    /// is reported as [`ProtocolError::InvalidPacket`] rather than guessed.
    pub fn from_root(transport: &mut RdpTransport) -> Result<ActorId, ProtocolError> {
        let root = RootActor::get_root(transport)?;
        addons_actor_from_root_reply(&root)
    }

    /// Install an unpacked extension directory or `.xpi` as a temporary add-on.
    ///
    /// `addon_path` must be an absolute native path: the server passes it to
    /// `new FileUtils.File(addonPath)` (`addons.js:32`). `openDevTools` is sent
    /// as `false` — ff-rdp never opens a toolbox window.
    ///
    /// A rejected install (bad manifest, missing file) comes back from Firefox
    /// as an error packet whose message starts
    /// `Could not install add-on at '<path>': …` (`addons.js:35-41`), surfaced
    /// here as [`ProtocolError::ActorError`].
    pub fn install_temporary_addon(
        transport: &mut RdpTransport,
        addons_actor: &ActorId,
        addon_path: &str,
    ) -> Result<InstalledAddon, ProtocolError> {
        let args = spec::request::InstallTemporaryAddon {
            addon_path: addon_path.to_owned(),
            open_dev_tools: Some(false),
        };
        call::<spec::InstallTemporaryAddon>(transport, addons_actor, &args).map(|reply| reply.addon)
    }

    /// Uninstall the temporary add-on `addon_id` (Firefox 113+).
    ///
    /// Firefox refuses anything that is not temporarily installed — an unknown
    /// id included — with an `Error` packet whose message is
    /// `Could not uninstall add-on "<id>"` (`addons.js:78-88`), surfaced as
    /// [`ProtocolError::ActorError`]. Firefox before 113 answers
    /// `unrecognizedPacketType`.
    pub fn uninstall_addon(
        transport: &mut RdpTransport,
        addons_actor: &ActorId,
        addon_id: &str,
    ) -> Result<(), ProtocolError> {
        let args = spec::request::UninstallAddon {
            addon_id: addon_id.to_owned(),
        };
        call::<spec::UninstallAddon>(transport, addons_actor, &args).map(|_| ())
    }
}

fn addons_actor_from_root_reply(root: &Value) -> Result<ActorId, ProtocolError> {
    root.get("addonsActor")
        .and_then(Value::as_str)
        .map(ActorId::from)
        .ok_or_else(|| {
            ProtocolError::InvalidPacket("getRoot reply has no 'addonsActor' field".into())
        })
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    use serde_json::json;

    use super::*;
    use crate::error::ActorErrorKind;
    use crate::transport::{encode_frame, recv_from};

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let raw = std::fs::read_to_string(&path).unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    /// Serve `reply` to exactly one request and hand the request back.
    fn serve_one(reply: Value) -> (u16, std::thread::JoinHandle<Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let request = recv_from(&mut reader).unwrap();
            stream
                .write_all(encode_frame(&reply.to_string()).as_bytes())
                .unwrap();
            request
        });
        (port, handle)
    }

    #[test]
    fn addons_actor_is_read_from_get_root() {
        let root = json!({"from": "root", "addonsActor": "server1.conn0.addonsActor3"});
        assert_eq!(
            addons_actor_from_root_reply(&root).unwrap(),
            ActorId::from("server1.conn0.addonsActor3")
        );
        assert!(matches!(
            addons_actor_from_root_reply(&json!({"from": "root"})),
            Err(ProtocolError::InvalidPacket(_))
        ));
    }

    #[test]
    fn install_sends_spec_request_and_parses_recorded_success() {
        let reply = fixture("install_temporary_addon_response.json");
        let actor = ActorId::from(reply["from"].as_str().unwrap());
        let (port, server) = serve_one(reply);
        let mut transport =
            RdpTransport::connect_raw("127.0.0.1", port, Duration::from_secs(2)).unwrap();
        let addon =
            AddonsActor::install_temporary_addon(&mut transport, &actor, "/abs/ext").unwrap();
        let request = server.join().unwrap();
        assert_eq!(
            request,
            json!({
                "to": actor.as_ref(),
                "type": "installTemporaryAddon",
                "addonPath": "/abs/ext",
                "openDevTools": false,
            })
        );
        assert_eq!(addon.id, "temporary-addon-fixture@ff-rdp.test");
        assert_eq!(addon.actor, Value::Bool(false));
    }

    #[test]
    fn install_surfaces_recorded_error_reply_message() {
        let reply = fixture("install_temporary_addon_error_response.json");
        let actor = ActorId::from(reply["from"].as_str().unwrap());
        let (port, server) = serve_one(reply);
        let mut transport =
            RdpTransport::connect_raw("127.0.0.1", port, Duration::from_secs(2)).unwrap();
        let err = AddonsActor::install_temporary_addon(&mut transport, &actor, "/abs/bad")
            .expect_err("a rejected install is an error");
        server.join().unwrap();
        match err {
            ProtocolError::ActorError {
                kind,
                error,
                message,
                ..
            } => {
                assert_eq!(error, "Error");
                assert!(matches!(kind, ActorErrorKind::Other(_)), "{kind:?}");
                assert!(
                    message.starts_with("Could not install add-on at '"),
                    "{message}"
                );
            }
            other => panic!("expected ActorError, got {other:?}"),
        }
    }

    #[test]
    fn uninstall_sends_spec_request_and_accepts_recorded_success() {
        let reply = fixture("uninstall_addon_response.json");
        let actor = ActorId::from(reply["from"].as_str().unwrap());
        let (port, server) = serve_one(reply);
        let mut transport =
            RdpTransport::connect_raw("127.0.0.1", port, Duration::from_secs(2)).unwrap();
        AddonsActor::uninstall_addon(&mut transport, &actor, "a@b").unwrap();
        assert_eq!(
            server.join().unwrap(),
            json!({"to": actor.as_ref(), "type": "uninstallAddon", "addonId": "a@b"})
        );
    }

    #[test]
    fn uninstall_surfaces_recorded_unknown_id_error() {
        let reply = fixture("uninstall_addon_unknown_id_error_response.json");
        let actor = ActorId::from(reply["from"].as_str().unwrap());
        let (port, server) = serve_one(reply);
        let mut transport =
            RdpTransport::connect_raw("127.0.0.1", port, Duration::from_secs(2)).unwrap();
        let err = AddonsActor::uninstall_addon(&mut transport, &actor, "no-such-addon@ff-rdp.test")
            .expect_err("an unknown id is refused");
        server.join().unwrap();
        match err {
            ProtocolError::ActorError { error, message, .. } => {
                assert_eq!(error, "Error");
                assert_eq!(
                    message,
                    "Could not uninstall add-on \"no-such-addon@ff-rdp.test\""
                );
            }
            other => panic!("expected ActorError, got {other:?}"),
        }
    }
}
