//! Operations on a Firefox `WebExtensionDescriptorActor` — one per installed
//! add-on, handed out by the root actor's `listAddons`.
//!
//! - Spec: `devtools/shared/specs/descriptors/webextension.js:12-45`
//! - Server: `devtools/server/actors/descriptors/webextension.js:78-220`
//!
//! See `kb/rdp/actors/descriptors/webextension-descriptor.md`.

use crate::error::ProtocolError;
use crate::specs::{NoArgs, call, webextension_descriptor as spec};
use crate::transport::RdpTransport;
use crate::types::ActorId;

pub use crate::specs::webextension_descriptor::response::AddonDescriptor;

/// Operations on a WebExtension descriptor actor.
pub struct WebExtensionDescriptorActor;

impl WebExtensionDescriptorActor {
    /// Reload the add-on in place — about:debugging's Reload button
    /// (`devtools/client/aboutdebugging/src/actions/debug-targets.js:161-175`).
    ///
    /// For a temporary add-on Firefox re-runs `installTemporaryAddon` on the
    /// path it was installed from (`XPIDatabase.sys.mjs:1505-1519`), which goes
    /// through `BootstrapScope.update` — an upgrade, not an uninstall — so open
    /// tabs stay open. A manifest that no longer loads comes back as
    /// [`ProtocolError::ActorError`]; the manifest is read before the running
    /// version is touched (`XPIInstall.sys.mjs:4833`), so that stays installed.
    pub fn reload(transport: &mut RdpTransport, descriptor: &ActorId) -> Result<(), ProtocolError> {
        call::<spec::Reload>(transport, descriptor, &NoArgs::default()).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    use serde_json::{Value, json};

    use super::*;
    use crate::transport::{encode_frame, recv_from};

    #[test]
    fn reload_sends_bare_request_and_accepts_recorded_reply() {
        let path = format!(
            "{}/tests/fixtures/webextension_reload_response.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let reply: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let actor = ActorId::from(reply["from"].as_str().unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let request = recv_from(&mut reader).unwrap();
            stream
                .write_all(encode_frame(&reply.to_string()).as_bytes())
                .unwrap();
            request
        });
        let mut transport =
            RdpTransport::connect_raw("127.0.0.1", port, Duration::from_secs(2)).unwrap();
        WebExtensionDescriptorActor::reload(&mut transport, &actor).unwrap();
        assert_eq!(
            server.join().unwrap(),
            json!({"to": actor.as_ref(), "type": "reload"})
        );
    }
}
