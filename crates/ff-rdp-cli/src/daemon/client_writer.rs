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
//! stream-subscriber list) holds a clone of the same private writer lease slot. Cancellation can
//! close the slot and interrupt its socket without waiting for that lease.
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
//! Every write, including lease acquisition and partial syscalls, shares one
//! absolute budget bounded by [`CLIENT_WRITE_DEADLINE`]. A client that
//! misses it is marked broken and its socket is shut down, so the dispatcher
//! moves on and the next write to that client fails immediately rather than
//! blocking again.

use std::io;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::lifecycle::{Cancellation, LeaseError, Registration, WakePhase, WriterSlot};

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
    /// The daemon cancelled this connection before another frame could begin.
    DaemonStopping,
}

impl WriteFailure {
    /// Stable machine-readable reason string (used as the `error` field of the
    /// `daemon_client_closed` frame and in the daemon log).
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::Desynchronised => "client_frame_write_desynchronised",
            Self::DeadlineExpired => "client_write_deadline_expired",
            Self::SocketError => "client_write_failed",
            Self::DaemonStopping => "daemon_shutting_down",
        }
    }
}

struct Inner {
    writer: WriterSlot<FramedWriter>,
    interrupt: TcpStream,
    deadline: Duration,
    failed: Mutex<Option<(WriteFailure, String)>>,
}

/// Every writer clone leases the same socket writer. Metadata and cancellation
/// never wait behind a lease doing I/O.
#[derive(Clone)]
pub(crate) struct ClientWriter {
    inner: Arc<Inner>,
}

impl ClientWriter {
    /// Production acquisition fails if its independent shutdown handle cannot
    /// be retained. No handler proceeds with uncancellable socket ownership.
    pub(crate) fn try_new(stream: TcpStream) -> io::Result<Self> {
        let interrupt = stream.try_clone()?;
        Ok(Self::from_parts(stream, interrupt, CLIENT_WRITE_DEADLINE))
    }

    fn from_parts(stream: TcpStream, interrupt: TcpStream, deadline: Duration) -> Self {
        let writer = FramedWriter::from_stream(stream);
        let _ = writer.set_write_timeout(Some(deadline));
        Self {
            inner: Arc::new(Inner {
                writer: WriterSlot::new(writer),
                interrupt,
                deadline,
                failed: Mutex::new(None),
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn new(stream: TcpStream) -> Self {
        Self::try_new(stream).expect("test client writer shutdown clone")
    }

    #[cfg(test)]
    pub(crate) fn with_deadline(stream: TcpStream, deadline: Duration) -> Self {
        let interrupt = stream
            .try_clone()
            .expect("test client writer shutdown clone");
        Self::from_parts(stream, interrupt, deadline)
    }

    pub(super) fn register_cancellation(&self, cancellation: &Arc<Cancellation>) -> Registration {
        let inner = Arc::downgrade(&self.inner);
        cancellation.register(WakePhase::SocketAndWriter, move || {
            if let Some(inner) = inner.upgrade() {
                let _ = inner.interrupt.shutdown(std::net::Shutdown::Both);
                inner.writer.close();
            }
        })
    }

    fn fail(&self, failure: WriteFailure, detail: String) -> WriteFailure {
        let mut failed = self
            .inner
            .failed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let first = failed.get_or_insert((failure, detail)).0;
        drop(failed);
        let _ = self.inner.interrupt.shutdown(std::net::Shutdown::Both);
        self.inner.writer.close();
        first
    }

    pub(crate) fn send_raw(&self, json: &str) -> Result<(), WriteFailure> {
        self.send_raw_until(json, Instant::now() + self.inner.deadline)
    }

    /// The caller starts this deadline before serialization and lease acquisition.
    /// Core transport consumes the same deadline through every write attempt.
    pub(super) fn send_raw_until(&self, json: &str, deadline: Instant) -> Result<(), WriteFailure> {
        if let Some((failure, _)) = self.failure() {
            return Err(failure);
        }
        let mut writer = match self.inner.writer.acquire(deadline) {
            Ok(writer) => writer,
            Err(LeaseError::Cancelled) => {
                return Err(self.failure().map_or(WriteFailure::DaemonStopping, |f| f.0));
            }
            Err(LeaseError::Deadline) => {
                return Err(self.fail(
                    WriteFailure::DeadlineExpired,
                    "writer lease deadline expired".to_owned(),
                ));
            }
        };
        if let Some((failure, _)) = self.failure() {
            return Err(failure);
        }
        match writer.send_raw_until(json, deadline) {
            Ok(()) => Ok(()),
            Err(error) => Err(self.fail(classify(&error), error.to_string())),
        }
    }

    pub(crate) fn send_bounded(
        &self,
        message: &Value,
        budget: Duration,
    ) -> Result<(), WriteFailure> {
        let deadline = Instant::now() + budget.min(self.inner.deadline);
        let json = serde_json::to_string(message).map_err(|_| WriteFailure::SocketError)?;
        self.send_raw_until(&json, deadline)
    }

    pub(crate) fn send(&self, message: &Value) -> Result<(), WriteFailure> {
        self.send_bounded(message, self.inner.deadline)
    }

    pub(crate) fn is_failed(&self) -> bool {
        self.inner
            .failed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }

    pub(crate) fn failure(&self) -> Option<(WriteFailure, String)> {
        self.inner
            .failed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    #[cfg(test)]
    pub(super) fn wait_for_blocked(&self, deadline: Instant) -> bool {
        self.inner.writer.wait_for_blocked(deadline)
    }

    #[cfg(test)]
    pub(super) fn hold_for_test(
        &self,
        deadline: Instant,
    ) -> Result<super::lifecycle::WriterLease<'_, FramedWriter>, LeaseError> {
        self.inner.writer.acquire(deadline)
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
