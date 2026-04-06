---
name: ff-rdp-core implementation status
description: Key design decisions and constraints for the ff-rdp-core crate transport layer
type: project
---

ff-rdp-core is fully implemented at `crates/ff-rdp-core/` with error, types, transport, and lib modules.

**Why:** Core library for Firefox Remote Debugging Protocol over TCP. Firefox uses length-prefixed JSON framing: `{len}:{json}` on send, same format on recv.

**How to apply:** The framing logic is in two internal helpers (`encode_frame`, `recv_from`) that accept `impl AsyncReadExt + Unpin` — this is what allows testing via `Cursor<&[u8]>` instead of live sockets, avoiding test hangs that occur with `read_to_end` on an open TCP writer half. When adding features, keep the framing helpers generic over I/O traits.

Note: ff-rdp-cli has a pre-existing compile error (`jaq_json::Val` missing `serde::Serialize`) unrelated to ff-rdp-core.
