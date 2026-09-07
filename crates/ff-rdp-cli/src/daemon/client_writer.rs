//! The one writer per CLI client socket (iteration 240).
//!
//! # Why this type exists
//!
//! Before it, a client's socket was written by whichever thread happened to
//! have a clone of it. `handle_client` minted a fresh
//! `FramedWriter::from_stream(reader.try_clone_stream()?)` for the greeting,
//! for every daemon-local response, for every `daemon_busy` refusal and for
//! every queue heartbeat; `handle_daemon_message` stored *another* clone in the
//! [`StreamSubscriber`](super::server) list; and the RPC slot held a third that
//! the **event-dispatcher thread** wrote Firefox replies through. Nothing
//! serialised them. Two `write_all`s racing on one socket interleave as soon as
//! either frame is large enough to be split across kernel writes — which a
//! `--with-page` page view always is — so the CLI's framer could resume reading
//! inside a payload.
//!
//! Every daemon→client byte now goes through one `ClientWriter`, and every
//! holder (greeting, daemon responses, heartbeats, the RPC slot, the
//! stream-subscriber list) holds a *clone of the same* `Arc<Mutex<…>>`. The
//! hazard is gone by construction rather than by timing.
//!
//! # The write deadline
//!
//! Client sockets had a 30 s read timeout and **no** write timeout, so
//! `forward_to_rpc_client` — running on the single event-dispatcher thread —
//! blocked in `write_all` forever against a client that had stopped reading
//! with a full receive window. Nothing else routes Firefox traffic, so every
//! other client, including brand-new ones, then timed out too, silently and
//! permanently, until the daemon was restarted. That is the wedge iteration 241
//! recorded ("~25 hops, then every hop times out, and the daemon log says
//! nothing at all").
//!
//! Every write here is bounded by [`CLIENT_WRITE_DEADLINE`]. A client that
//! misses it is marked broken and its socket is shut down, so the dispatcher
//! moves on and the next write to that client fails immediately rather than
//! blocking again.

use std::io;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ff_rdp_core::{FramedWriter, ProtocolError};
use serde_json::Value;

/// How long a single daemon→client frame write may take before the client is
/// treated as gone.
///
/// Matched to the CLI's own default socket deadline (`--timeout`, 10 s): a
/// client that has not drained a byte for that long has already given up on
/// this request, so nothing is lost by dropping it — whereas the dispatcher
/// blocking on it costs every other client the whole daemon.
pub(crate) const CLIENT_WRITE_DEADLINE: Duration = Duration::from_secs(10);

/// Why a [`ClientWriter`] stopped accepting frames.
///
/// Kept as a static string so the reason can be reported in the
/// `daemon_client_closed` frame and in the daemon log without allocating on the
/// error path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteFailure {
    /// The frame was cut in half — the client's stream is desynchronised and
    /// the socket has been shut down.
    Desynchronised,
    /// The write deadline expired with nothing written: the client is alive but
    /// not reading.
    DeadlineExpired,
    /// The socket errored (reset, closed, …).
    SocketError,
}

impl WriteFailure {
    /// Stable machine-readable reason string (used as the `error` field of the
    /// `daemon_client_closed` frame and in the daemon log).
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::Desynchronised => "client_frame_write_desynchronised",
            Self::DeadlineExpired => "client_write_deadline_expired",
            Self::SocketError => "client_write_failed",
        }
    }
}

struct Inner {
    writer: FramedWriter,
    /// Set on the first failure; every later write fails fast with it rather
    /// than blocking on a socket that is already known to be gone.
    failed: Option<(WriteFailure, String)>,
}

/// The single, shared, deadline-bounded write half of one CLI client's socket.
///
/// Cloning is cheap and yields another handle to the *same* writer — that is
/// the whole point: there is exactly one place where bytes enter this socket.
#[derive(Clone)]
pub(crate) struct ClientWriter {
    inner: Arc<Mutex<Inner>>,
}

impl ClientWriter {
    /// Wrap a client socket, installing [`CLIENT_WRITE_DEADLINE`] on it.
    ///
    /// The deadline is best-effort: a platform that refuses `SO_SNDTIMEO`
    /// leaves the writer working exactly as before rather than failing the
    /// connection outright.
    pub(crate) fn new(stream: TcpStream) -> Self {
        Self::with_deadline(stream, CLIENT_WRITE_DEADLINE)
    }

    /// [`ClientWriter::new`] with an explicit deadline.
    ///
    /// Production always uses [`CLIENT_WRITE_DEADLINE`]; tests that need to
    /// observe the drop-on-deadline behaviour pass a short one rather than
    /// spending ten seconds proving it.
    pub(crate) fn with_deadline(stream: TcpStream, deadline: Duration) -> Self {
        let writer = FramedWriter::from_stream(stream);
        let _ = writer.set_write_timeout(Some(deadline));
        Self {
            inner: Arc::new(Mutex::new(Inner {
                writer,
                failed: None,
            })),
        }
    }

    /// Send a pre-serialised JSON string as one frame.
    ///
    /// Returns the typed failure so the caller can name it in the daemon log
    /// and in the client's goodbye frame.
    pub(crate) fn send_raw(&self, json: &str) -> Result<(), WriteFailure> {
        // Poison recovery mirrors `lock_or_recover!` in `server.rs`: the
        // protected data stays structurally valid across a panic, and refusing
        // to write for the rest of the daemon's life would be a worse outcome
        // than continuing.
        let mut guard = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

        if let Some((failure, _)) = guard.failed {
            return Err(failure);
        }

        match guard.writer.send_raw(json) {
            Ok(()) => Ok(()),
            Err(e) => {
                let failure = classify(&e);
                if failure == WriteFailure::DeadlineExpired {
                    // Nothing of this frame reached the socket, so the stream
                    // is still aligned — but the client is not draining, and a
                    // second attempt would block for another full deadline on
                    // the dispatcher thread. Shut the socket down so the client
                    // learns immediately instead of waiting out its own read
                    // timeout.
                    let _ = guard.writer.shutdown();
                }
                guard.failed = Some((failure, e.to_string()));
                Err(failure)
            }
        }
    }

    /// Send a JSON value as one frame.
    pub(crate) fn send(&self, message: &Value) -> Result<(), WriteFailure> {
        match serde_json::to_string(message) {
            Ok(json) => self.send_raw(&json),
            // An unserialisable value is a daemon bug, not a client failure;
            // report it as a socket error so the caller drops the client rather
            // than looping on it.
            Err(_) => Err(WriteFailure::SocketError),
        }
    }

    /// The failure and its underlying I/O description, if any.
    pub(crate) fn failure(&self) -> Option<(WriteFailure, String)> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .failed
            .clone()
    }
}

/// Map a transport-level send error onto the daemon's client-drop taxonomy.
fn classify(e: &ProtocolError) -> WriteFailure {
    match e {
        ProtocolError::FrameWriteDesynchronised { .. } => WriteFailure::Desynchronised,
        ProtocolError::Timeout => WriteFailure::DeadlineExpired,
        ProtocolError::SendFailed(io_err) if io_err.kind() == io::ErrorKind::WouldBlock => {
            WriteFailure::DeadlineExpired
        }
        _ => WriteFailure::SocketError,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;
    use std::net::{TcpListener, TcpStream as StdTcpStream};

    /// A connected loopback pair: `(daemon side, client side)`.
    fn socket_pair() -> (StdTcpStream, StdTcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let client = StdTcpStream::connect(addr).expect("connect");
        let (server, _) = listener.accept().expect("accept");
        (server, client)
    }

    /// The central guarantee: concurrent writers on one `ClientWriter` produce
    /// a stream that decodes frame-for-frame.
    ///
    /// Before iteration 240 each writer was its own `FramedWriter` over its own
    /// `try_clone` of the socket, so two `write_all`s could interleave and the
    /// reader would resume mid-payload.
    /// Payloads big enough that the kernel splits them across writes, which is
    /// what made the old racing writers observable.
    const THREADS: usize = 4;
    const PER_THREAD: usize = 25;

    #[test]
    fn concurrent_writes_decode_frame_for_frame() {
        let (server, client) = socket_pair();
        let writer = ClientWriter::new(server);

        let bodies: Vec<String> = (0..THREADS)
            .map(|t| format!(r#"{{"t":{t},"pad":"{}"}}"#, "x".repeat(40_000)))
            .collect();

        let mut handles = Vec::new();
        for body in bodies.clone() {
            let w = writer.clone();
            handles.push(std::thread::spawn(move || {
                for _ in 0..PER_THREAD {
                    w.send_raw(&body).expect("send");
                }
            }));
        }

        // Read everything the four threads wrote and re-frame it.
        let reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut client = client;
            client
                .set_read_timeout(Some(Duration::from_secs(30)))
                .expect("read timeout");
            let mut chunk = [0u8; 8192];
            loop {
                match client.read(&mut chunk) {
                    Ok(n) if n > 0 => buf.extend_from_slice(&chunk[..n]),
                    // `Ok(0)` is EOF (the writer dropped); an error is the read
                    // deadline or a dead socket. Either way there is no more to
                    // collect.
                    Ok(_) | Err(_) => break,
                }
            }
            buf
        });

        for h in handles {
            h.join().expect("writer thread");
        }
        // Dropping the writer closes the socket, ending the reader loop.
        drop(writer);
        let buf = reader.join().expect("reader thread");

        // Decode `{len}:{json}` frames strictly: any interleaving shows up
        // either as a non-digit where a length is expected or as a payload that
        // is not one of the four bodies.
        let mut pos = 0usize;
        let mut decoded = 0usize;
        while pos < buf.len() {
            let colon = buf[pos..]
                .iter()
                .position(|b| *b == b':')
                .unwrap_or_else(|| panic!("no length prefix at offset {pos}"));
            let len_str = std::str::from_utf8(&buf[pos..pos + colon]).expect("utf8 length prefix");
            let len: usize = len_str
                .parse()
                .unwrap_or_else(|_| panic!("bad length prefix {len_str:?} at offset {pos}"));
            let start = pos + colon + 1;
            let body = std::str::from_utf8(&buf[start..start + len]).expect("utf8 payload");
            assert!(
                bodies.iter().any(|b| b == body),
                "frame {decoded} is not one of the written payloads"
            );
            pos = start + len;
            decoded += 1;
        }
        assert_eq!(
            decoded,
            THREADS * PER_THREAD,
            "every frame must be decodable, in one piece"
        );
    }

    /// A client that never reads must not block a writer past the deadline, and
    /// must be marked failed so later writes fail fast.
    #[test]
    fn non_reading_client_fails_within_the_deadline() {
        let (server, client) = socket_pair();
        // Shrink the deadline for the test — the production constant is 10 s.
        let writer = ClientWriter::with_deadline(server, Duration::from_millis(250));

        // Never read from `client`; keep it alive so the socket is not reset.
        let body = format!(r#"{{"pad":"{}"}}"#, "x".repeat(200_000));
        let started = std::time::Instant::now();
        let mut outcome = Ok(());
        // Fill the receive window: the first writes succeed into the buffers,
        // then one blocks and hits the deadline.
        for _ in 0..200 {
            outcome = writer.send_raw(&body);
            if outcome.is_err() {
                break;
            }
        }
        let failure = outcome.expect_err("a non-reading client must eventually fail the write");
        assert!(
            matches!(
                failure,
                WriteFailure::DeadlineExpired | WriteFailure::Desynchronised
            ),
            "unexpected failure: {failure:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the write must be bounded by the deadline, not block indefinitely"
        );

        // Fail-fast on every later write, without touching the socket again.
        assert!(writer.failure().is_some());
        let second = std::time::Instant::now();
        assert_eq!(writer.send_raw("{}").expect_err("still failed"), failure);
        assert!(
            second.elapsed() < Duration::from_millis(100),
            "a failed writer must not block again"
        );

        drop(client);
    }

    #[test]
    fn failure_reasons_are_stable() {
        assert_eq!(
            WriteFailure::Desynchronised.reason(),
            "client_frame_write_desynchronised"
        );
        assert_eq!(
            WriteFailure::DeadlineExpired.reason(),
            "client_write_deadline_expired"
        );
        assert_eq!(WriteFailure::SocketError.reason(), "client_write_failed");
    }
}
