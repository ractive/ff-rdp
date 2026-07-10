//! Live AC for iter-75 M-1: a malicious/buggy peer cannot OOM ff-rdp by
//! announcing a multi-GB bulk frame.  We stand up a tiny TCP server that
//! sends the Firefox greeting, then a bulk header whose declared length is
//! way above our configured cap, and we assert that the receive side returns
//! `BulkFrameTooLarge` promptly without trying to read or allocate the body.
//!
//! Gated behind `FF_RDP_LIVE_TESTS=1` so it doesn't run in the default
//! workspace tests.
//!
//! ```sh
//! FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live live_bulk_cap
//! ```

use std::io::Write;
use std::net::TcpListener;
use std::time::{Duration, Instant};

use ff_rdp_core::ProtocolError;
use ff_rdp_core::transport::{RdpTransport, max_frame_bytes, set_max_frame_bytes};

/// Restores the process-global transport frame cap on drop (panic-safe), so
/// this test's 1 KiB cap can't leak into later tests in the same binary.
struct FrameCapGuard(usize);

impl Drop for FrameCapGuard {
    fn drop(&mut self) {
        set_max_frame_bytes(self.0);
    }
}

fn live_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_TESTS").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// AC: `live_bulk_frame_oversize_rejected` — connect to a local mock that
/// announces `bulk … length:<2*max_frame>`, assert `BulkFrameTooLarge`
/// returned within 50ms, no buffer allocation observed (no body bytes sent
/// — if we tried to read them we'd block on the read timeout instead).
///
// allow-ungated-live: no real Firefox — this connects to an in-process mock TCP
// server and is fast, so it runs by default under FF_RDP_LIVE_TESTS=1 (no-op
// pass when unset). #[ignore] would hide a cheap, Firefox-free probe behind the
// live-Firefox gate for no benefit. See iter-75 / iter-113 Theme B.
#[test]
fn live_bulk_frame_oversize_rejected() {
    if !live_tests_enabled() {
        eprintln!("FF_RDP_LIVE_TESTS not set — skipping");
        return;
    }

    // Pick a small cap to keep the announcement modest. The cap is a
    // process-global knob shared with every other test in this binary, so
    // restore it on exit — including panic unwind — or later in-process
    // transport users fail with FrameTooLarge on ordinary Firefox packets
    // (observed as live_console_no_double_delivery red in the iter-114 sweep).
    let _cap_guard = FrameCapGuard(max_frame_bytes());
    set_max_frame_bytes(1024);
    let cap = 1024u64;
    let announced = cap * 2;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
    let port = listener.local_addr().unwrap().port();

    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().expect("accept");
        // Firefox greeting — minimal valid JSON frame.
        let greeting = r#"{"from":"root","applicationType":"browser","traits":{}}"#;
        let frame = format!("{}:{}", greeting.len(), greeting);
        s.write_all(frame.as_bytes()).unwrap();
        // Then a bulk frame header with an oversize length.  We do NOT
        // send any body bytes — if the client attempts to read the body
        // it will block until the socket read timeout (2s) fires and we
        // would observe `Timeout`/`RecvFailed` rather than
        // `BulkFrameTooLarge`.
        let bulk_header = format!("bulk mock/actor heap {announced}:");
        s.write_all(bulk_header.as_bytes()).unwrap();
        // Keep the socket alive so the read side actually parses the header.
        std::thread::sleep(Duration::from_secs(2));
        drop(s);
    });

    let mut t =
        RdpTransport::connect("127.0.0.1", port, Duration::from_secs(2)).expect("connect mock");

    let started = Instant::now();
    let err = t.recv().expect_err("oversize bulk frame must be rejected");
    let elapsed = started.elapsed();
    server.join().ok();

    match &err {
        ProtocolError::BulkFrameTooLarge {
            announced: a,
            max: m,
        } => {
            assert_eq!(*a, announced, "announced length must round-trip");
            assert_eq!(*m, cap, "max must equal our cap");
        }
        other => panic!("expected BulkFrameTooLarge, got {other:?}"),
    }
    assert!(
        elapsed < Duration::from_millis(500),
        "rejection must be prompt (no body read), took {elapsed:?}"
    );
}
