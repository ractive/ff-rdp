use thiserror::Error;

use crate::types::ActorId;

// ---------------------------------------------------------------------------
// Top-level typed error + result alias
// ---------------------------------------------------------------------------

/// Typed error taxonomy for the Firefox RDP core library.
///
/// New typed-error variants for wire-level failures; existing actors still
/// return [`ProtocolError`] for now.  Callers (e.g. the CLI) can map these
/// discriminants to deterministic exit codes.
#[derive(Debug, Error)]
pub enum RdpError {
    /// Low-level I/O or framing failure on the TCP transport.
    #[error("transport error: {0}")]
    Transport(#[from] std::io::Error),

    /// Firefox returned an error packet from an actor.
    #[error("actor error from {actor}: {name} — {message}")]
    Protocol {
        actor: String,
        name: String,
        message: String,
    },

    /// A received packet does not have the expected JSON shape.
    #[error("unexpected packet shape at {path}: expected {expected}, got {got}")]
    Shape {
        path: String,
        expected: String,
        got: String,
    },

    /// An operation exceeded its deadline.
    #[error("operation timed out after {after_ms}ms (phase: {phase})")]
    Timeout { phase: String, after_ms: u64 },

    /// The Firefox RDP peer closed the connection.
    #[error("remote connection closed unexpectedly")]
    RemoteClosed,

    /// The actor has been destroyed (e.g. after a cross-origin navigation or tab close).
    ///
    /// Surfaced as `error_type: "actor_destroyed"` in CLI JSON output.
    #[error("actor {actor} has been destroyed — target navigated or closed")]
    ActorDestroyed { actor: ActorId },
}

/// Convenience alias used throughout `ff-rdp-core`.
pub type RdpResult<T> = Result<T, RdpError>;

/// Well-known Firefox RDP actor error codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorErrorKind {
    /// The actor ID does not exist (expired connection, wrong tab, etc.).
    UnknownActor,
    /// The actor is in a wrong state for the requested operation.
    WrongState,
    /// The thread would deadlock if the operation were performed.
    ThreadWouldRun,
    /// The actor does not recognise the requested packet type / method name.
    ///
    /// This typically means the method was renamed or removed in a newer
    /// Firefox version (error code `"unrecognizedPacketType"`).
    UnrecognizedPacketType,
    /// An unrecognised error code — the raw string is preserved.
    Other(String),
}

impl ActorErrorKind {
    /// Classify a raw error code string from Firefox.
    pub fn from_code(code: &str) -> Self {
        match code {
            "unknownActor" | "noSuchActor" => Self::UnknownActor,
            "wrongState" => Self::WrongState,
            "threadWouldRun" => Self::ThreadWouldRun,
            "unrecognizedPacketType" => Self::UnrecognizedPacketType,
            _ => Self::Other(code.to_owned()),
        }
    }
}

impl std::fmt::Display for ActorErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownActor => write!(f, "unknownActor"),
            Self::WrongState => write!(f, "wrongState"),
            Self::ThreadWouldRun => write!(f, "threadWouldRun"),
            Self::UnrecognizedPacketType => write!(f, "unrecognizedPacketType"),
            Self::Other(code) => write!(f, "{code}"),
        }
    }
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("connection failed: {0}")]
    ConnectionFailed(#[source] std::io::Error),

    #[error("send failed: {0}")]
    SendFailed(#[source] std::io::Error),

    #[error("recv failed: {0}")]
    RecvFailed(#[source] std::io::Error),

    #[error("invalid packet: {0}")]
    InvalidPacket(String),

    #[error("operation timed out")]
    Timeout,

    /// The page navigated while waiting for a JavaScript evaluation result.
    ///
    /// Firefox sends `tabNavigated` or `willNavigate` push events when the
    /// page URL changes mid-eval.  The `evaluationResult` will never arrive
    /// in that case, so we surface this typed error immediately instead of
    /// hanging until the socket read timeout fires.
    #[error("page navigated during JS evaluation — result will not arrive")]
    EvalNavigatedDuringEval,

    /// Frame declared too large to be a valid Firefox RDP packet.
    ///
    /// Firefox frames are length-prefixed JSON.  A declared length exceeding
    /// [`MAX_FRAME_BYTES`](crate::transport::MAX_FRAME_BYTES) is either a
    /// malformed stream or a memory-exhaustion attempt and is rejected before
    /// any allocation is made.
    #[error("RDP frame too large: declared {declared} bytes, max {max} bytes")]
    FrameTooLarge { declared: usize, max: usize },

    #[error("actor error from {actor}: {error} ({kind}) — {message}")]
    ActorError {
        actor: String,
        kind: ActorErrorKind,
        /// The raw error code string from Firefox.
        error: String,
        message: String,
    },
}

impl ProtocolError {
    /// Returns true if this is an `ActorError` with `UnknownActor` kind.
    pub fn is_unknown_actor(&self) -> bool {
        matches!(
            self,
            Self::ActorError {
                kind: ActorErrorKind::UnknownActor,
                ..
            }
        )
    }

    /// Returns true if this is an `ActorError` with `UnrecognizedPacketType` kind.
    ///
    /// This indicates the method name was not recognised by the actor — the
    /// method may have been renamed or removed in a newer Firefox version.
    pub fn is_unrecognized_packet_type(&self) -> bool {
        matches!(
            self,
            Self::ActorError {
                kind: ActorErrorKind::UnrecognizedPacketType,
                ..
            }
        )
    }

    /// Returns `true` if the error is transient and the operation can safely be
    /// retried after a short backoff.
    ///
    /// Transient errors:
    /// - `Timeout` — the socket read timed out; Firefox may respond on retry.
    /// - `ConnectionClosed` (expressed as a recv/send I/O error) — the daemon or
    ///   Firefox closed the connection mid-stream; a fresh connection may work.
    /// - `ActorError { UnknownActor }` — the actor was garbage-collected (e.g.
    ///   after a soft navigation); retrying after reconnect may resolve this.
    ///
    /// Terminal errors (retry would not help):
    /// - `ActorError { UnrecognizedPacketType }` — method does not exist.
    /// - `ActorError { WrongState | ThreadWouldRun }` — page or debugger state.
    /// - `InvalidPacket` / `FrameTooLarge` — protocol mismatch.
    /// - `ConnectionFailed` — Firefox is not listening; retry won't help until
    ///   the user starts Firefox.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Timeout
                | Self::RecvFailed(_)
                | Self::SendFailed(_)
                | Self::ActorError {
                    kind: ActorErrorKind::UnknownActor,
                    ..
                }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── RdpError::ActorDestroyed ─────────────────────────────────────────────

    #[test]
    fn actor_destroyed_display_contains_actor_and_phrase() {
        let actor = ActorId::from("conn0/tab1");
        let err = RdpError::ActorDestroyed {
            actor: actor.clone(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("conn0/tab1"),
            "display must include the actor ID; got: {msg}"
        );
        assert!(
            msg.contains("has been destroyed"),
            "display must contain 'has been destroyed'; got: {msg}"
        );
    }

    #[test]
    fn actor_error_kind_from_code_known_codes() {
        assert_eq!(
            ActorErrorKind::from_code("unknownActor"),
            ActorErrorKind::UnknownActor
        );
        assert_eq!(
            ActorErrorKind::from_code("noSuchActor"),
            ActorErrorKind::UnknownActor
        );
        assert_eq!(
            ActorErrorKind::from_code("wrongState"),
            ActorErrorKind::WrongState
        );
        assert_eq!(
            ActorErrorKind::from_code("threadWouldRun"),
            ActorErrorKind::ThreadWouldRun
        );
        assert_eq!(
            ActorErrorKind::from_code("unrecognizedPacketType"),
            ActorErrorKind::UnrecognizedPacketType
        );
    }

    #[test]
    fn actor_error_kind_from_code_unknown() {
        assert_eq!(
            ActorErrorKind::from_code("someWeirdError"),
            ActorErrorKind::Other("someWeirdError".to_owned())
        );
        assert_eq!(
            ActorErrorKind::from_code(""),
            ActorErrorKind::Other(String::new())
        );
    }

    #[test]
    fn actor_error_kind_display() {
        assert_eq!(ActorErrorKind::UnknownActor.to_string(), "unknownActor");
        assert_eq!(ActorErrorKind::WrongState.to_string(), "wrongState");
        assert_eq!(ActorErrorKind::ThreadWouldRun.to_string(), "threadWouldRun");
        assert_eq!(
            ActorErrorKind::UnrecognizedPacketType.to_string(),
            "unrecognizedPacketType"
        );
        assert_eq!(
            ActorErrorKind::Other("customError".to_owned()).to_string(),
            "customError"
        );
    }

    #[test]
    fn is_unknown_actor_returns_true_for_unknown_actor_kind() {
        let err = ProtocolError::ActorError {
            actor: "conn0/actor1".to_owned(),
            kind: ActorErrorKind::UnknownActor,
            error: "unknownActor".to_owned(),
            message: String::new(),
        };
        assert!(err.is_unknown_actor());
    }

    #[test]
    fn is_unknown_actor_returns_false_for_other_kinds() {
        let wrong_state = ProtocolError::ActorError {
            actor: "conn0/actor1".to_owned(),
            kind: ActorErrorKind::WrongState,
            error: "wrongState".to_owned(),
            message: String::new(),
        };
        assert!(!wrong_state.is_unknown_actor());

        let other = ProtocolError::ActorError {
            actor: "conn0/actor1".to_owned(),
            kind: ActorErrorKind::Other("customError".to_owned()),
            error: "customError".to_owned(),
            message: String::new(),
        };
        assert!(!other.is_unknown_actor());

        assert!(!ProtocolError::Timeout.is_unknown_actor());
    }

    #[test]
    fn is_unrecognized_packet_type_returns_true() {
        let err = ProtocolError::ActorError {
            actor: "conn0/walker1".to_owned(),
            kind: ActorErrorKind::UnrecognizedPacketType,
            error: "unrecognizedPacketType".to_owned(),
            message: "getRootNode".to_owned(),
        };
        assert!(err.is_unrecognized_packet_type());
    }

    #[test]
    fn is_unrecognized_packet_type_returns_false_for_other_kinds() {
        assert!(
            !ProtocolError::ActorError {
                actor: "conn0/actor1".to_owned(),
                kind: ActorErrorKind::UnknownActor,
                error: "unknownActor".to_owned(),
                message: String::new(),
            }
            .is_unrecognized_packet_type()
        );

        assert!(!ProtocolError::Timeout.is_unrecognized_packet_type());
    }

    // ── is_transient tests ──────────────────────────────────────────────────

    fn io_err(kind: std::io::ErrorKind) -> std::io::Error {
        std::io::Error::new(kind, "test io error")
    }

    #[test]
    fn is_transient_timeout_is_true() {
        assert!(ProtocolError::Timeout.is_transient());
    }

    #[test]
    fn is_transient_recv_failed_io_timed_out_is_true() {
        assert!(ProtocolError::RecvFailed(io_err(std::io::ErrorKind::TimedOut)).is_transient());
    }

    #[test]
    fn is_transient_recv_failed_would_block_is_true() {
        assert!(ProtocolError::RecvFailed(io_err(std::io::ErrorKind::WouldBlock)).is_transient());
    }

    #[test]
    fn is_transient_recv_failed_connection_reset_is_true() {
        // RecvFailed is transient regardless of the I/O kind — a new connection may work.
        assert!(
            ProtocolError::RecvFailed(io_err(std::io::ErrorKind::ConnectionReset)).is_transient()
        );
    }

    #[test]
    fn is_transient_send_failed_is_true() {
        assert!(ProtocolError::SendFailed(io_err(std::io::ErrorKind::TimedOut)).is_transient());
        assert!(ProtocolError::SendFailed(io_err(std::io::ErrorKind::BrokenPipe)).is_transient());
    }

    #[test]
    fn is_transient_actor_error_unknown_actor_is_true() {
        let err = ProtocolError::ActorError {
            actor: "conn0/actor1".to_owned(),
            kind: ActorErrorKind::UnknownActor,
            error: "unknownActor".to_owned(),
            message: String::new(),
        };
        assert!(err.is_transient());
    }

    #[test]
    fn is_transient_terminal_errors_are_false() {
        // InvalidPacket is terminal.
        assert!(!ProtocolError::InvalidPacket("bad".to_owned()).is_transient());

        // FrameTooLarge is terminal.
        assert!(
            !ProtocolError::FrameTooLarge {
                declared: 99_999_999,
                max: 10_000_000
            }
            .is_transient()
        );

        // ConnectionFailed is terminal (Firefox isn't running).
        assert!(
            !ProtocolError::ConnectionFailed(io_err(std::io::ErrorKind::ConnectionRefused))
                .is_transient()
        );
    }

    #[test]
    fn is_transient_actor_error_wrong_state_is_false() {
        let err = ProtocolError::ActorError {
            actor: "conn0/actor1".to_owned(),
            kind: ActorErrorKind::WrongState,
            error: "wrongState".to_owned(),
            message: String::new(),
        };
        assert!(!err.is_transient());
    }

    #[test]
    fn is_transient_actor_error_unrecognized_packet_type_is_false() {
        let err = ProtocolError::ActorError {
            actor: "conn0/actor1".to_owned(),
            kind: ActorErrorKind::UnrecognizedPacketType,
            error: "unrecognizedPacketType".to_owned(),
            message: String::new(),
        };
        assert!(!err.is_transient());
    }
}
