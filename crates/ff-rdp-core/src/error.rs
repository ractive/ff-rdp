use thiserror::Error;

use crate::types::ActorId;

// ---------------------------------------------------------------------------
// Top-level typed error + result alias
// ---------------------------------------------------------------------------

/// Root cause of a navigation failure, extracted from Firefox's `about:neterror` URL.
///
/// Maps the `e=` query-parameter values that Firefox encodes in `about:neterror`
/// to typed variants so callers (and the CLI exit-code mapper) can branch on them
/// without parsing raw strings.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NavCause {
    /// `e=dnsNotFound` — DNS resolution failed.
    #[error("DNS resolution failed")]
    DnsFail,
    /// `e=nssFailure2`, `e=sslv3Used`, `e=inadequateSecurityError` — TLS/cert error.
    #[error("TLS/certificate error")]
    CertError,
    /// `e=netReset`, `e=netInterrupt`, `e=connectionRefused` — connection reset or refused.
    #[error("connection reset or refused")]
    ConnReset,
    /// `e=netTimeout` — network timeout.
    #[error("network timeout")]
    Timeout,
    /// `e=cspBlocked`, `e=blockedByPolicy`, `e=remoteXUL` — content blocked by policy.
    #[error("content blocked by policy")]
    ContentBlocked,
    /// Any other `e=` value not covered above.
    #[error("navigation error: {0}")]
    Unknown(String),
}

impl NavCause {
    /// Classify a raw `e=` parameter value from an `about:neterror` URL.
    pub fn from_e_param(e: &str) -> Self {
        match e {
            "dnsNotFound" => Self::DnsFail,
            "nssFailure2" | "sslv3Used" | "inadequateSecurityError" => Self::CertError,
            "netReset" | "netInterrupt" | "connectionRefused" | "connectionFailure" => {
                Self::ConnReset
            }
            "netTimeout" => Self::Timeout,
            "cspBlocked" | "blockedByPolicy" | "remoteXUL" => Self::ContentBlocked,
            other => Self::Unknown(other.to_owned()),
        }
    }
}

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

    /// Firefox navigated to `about:neterror`, indicating a DNS/network failure.
    ///
    /// The `cause` discriminant lets callers (and the CLI) map to deterministic
    /// exit codes without parsing raw strings.
    #[error("navigation to '{url}' failed: {cause}")]
    Navigation { cause: NavCause, url: String },
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
    /// Required request parameter was missing (error code `"missingParameter"`).
    ///
    /// Terminal — a retry without fixing the request will hit the same error.
    MissingParameter,
    /// Request parameter had the wrong type (error code `"badParameterType"`).
    ///
    /// Terminal — a retry without fixing the request will hit the same error.
    BadParameterType,
    /// Method exists but is not implemented by this Firefox build
    /// (error code `"notImplemented"`).
    NotImplemented,
    /// Calls were issued in the wrong order — e.g. `evaluateJSAsync` before
    /// the console actor has finished `startListeners` (error code `"wrongOrder"`).
    WrongOrder,
    /// Generic protocol-level failure reported by Firefox
    /// (error code `"protocolError"`).
    ProtocolError,
    /// Firefox could not classify the failure (error code `"unknownError"`).
    UnknownError,
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
            "missingParameter" => Self::MissingParameter,
            "badParameterType" => Self::BadParameterType,
            "notImplemented" => Self::NotImplemented,
            "wrongOrder" => Self::WrongOrder,
            "protocolError" => Self::ProtocolError,
            "unknownError" => Self::UnknownError,
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
            Self::MissingParameter => write!(f, "missingParameter"),
            Self::BadParameterType => write!(f, "badParameterType"),
            Self::NotImplemented => write!(f, "notImplemented"),
            Self::WrongOrder => write!(f, "wrongOrder"),
            Self::ProtocolError => write!(f, "protocolError"),
            Self::UnknownError => write!(f, "unknownError"),
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
    /// [`max_frame_bytes`](crate::transport::max_frame_bytes) is either a
    /// malformed stream or a memory-exhaustion attempt and is rejected before
    /// any allocation is made.
    #[error("RDP frame too large: declared {declared} bytes, max {max} bytes")]
    FrameTooLarge { declared: usize, max: usize },

    /// A `bulk` frame declared a payload larger than the configured cap.
    ///
    /// The Firefox RDP transport.js receiver does **not** enforce an upper
    /// bound on the announced bulk-frame length, so a malicious or buggy peer
    /// could pin our memory by streaming a multi-GB body.  Detected before the
    /// body is read so the stream becomes unreadable but no allocation has
    /// been attempted.
    #[error("RDP bulk frame too large: announced {announced} bytes, max {max} bytes")]
    BulkFrameTooLarge { announced: u64, max: u64 },

    /// Firefox sent a `bulk` binary frame that this implementation cannot process.
    ///
    /// The frame has been consumed from the stream (all `length` bytes skipped)
    /// so the next `recv_from` call will see the following packet correctly.
    /// Callers should log this once and continue reading.
    #[error(
        "bulk packet unsupported: actor={actor} kind={kind} length={length} \
         (skipped {length} bytes)"
    )]
    BulkPacketUnsupported {
        actor: String,
        kind: String,
        length: u64,
    },

    /// The caller requested a bulk frame from a specific actor/kind, but the
    /// next frame on the wire was either a JSON packet or a bulk frame with a
    /// different actor or kind.
    ///
    /// When this is returned the stream position is undefined — the caller
    /// should treat the connection as unrecoverable.
    #[error("unexpected bulk packet: expected actor={actor} kind={kind}")]
    BulkPacketUnexpected { actor: String, kind: String },

    /// A per-actor demux channel is at capacity.
    ///
    /// Returned by [`DemuxReader::dispatch`] when the bounded channel for
    /// `actor` is full.  The packet is dropped; the caller (reader thread) logs
    /// a warning and continues reading so the Firefox connection stays healthy.
    #[error("actor channel full: actor={actor}")]
    ActorChannelFull { actor: String },

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
    /// - `ActorError { MissingParameter | BadParameterType }` — the request
    ///   itself is wrong; retrying the same payload will keep failing.
    /// - `InvalidPacket` / `FrameTooLarge` — protocol mismatch.
    /// - `ConnectionFailed` — Firefox is not listening; retry won't help until
    ///   the user starts Firefox.
    pub fn is_transient(&self) -> bool {
        // The terminal-ActorError arm below is exhaustive over `ActorErrorKind`
        // on purpose: when a new variant is added in `error.rs`, the compiler
        // forces a decision here rather than letting the new variant fall
        // through to a silent default.
        match self {
            Self::Timeout
            | Self::RecvFailed(_)
            | Self::SendFailed(_)
            | Self::ActorError {
                kind: ActorErrorKind::UnknownActor,
                ..
            } => true,
            Self::ActorError {
                kind:
                    ActorErrorKind::MissingParameter
                    | ActorErrorKind::BadParameterType
                    | ActorErrorKind::NotImplemented
                    | ActorErrorKind::WrongOrder
                    | ActorErrorKind::ProtocolError
                    | ActorErrorKind::UnknownError
                    | ActorErrorKind::UnrecognizedPacketType
                    | ActorErrorKind::WrongState
                    | ActorErrorKind::ThreadWouldRun
                    | ActorErrorKind::Other(_),
                ..
            }
            | Self::ConnectionFailed(_)
            | Self::InvalidPacket(_)
            | Self::EvalNavigatedDuringEval
            | Self::FrameTooLarge { .. }
            | Self::BulkFrameTooLarge { .. }
            | Self::BulkPacketUnsupported { .. }
            | Self::BulkPacketUnexpected { .. }
            | Self::ActorChannelFull { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── NavCause::from_e_param ───────────────────────────────────────────────

    #[test]
    fn classify_neterror_dns_not_found() {
        assert_eq!(NavCause::from_e_param("dnsNotFound"), NavCause::DnsFail);
    }

    #[test]
    fn classify_neterror_cert_errors() {
        assert_eq!(NavCause::from_e_param("nssFailure2"), NavCause::CertError);
        assert_eq!(NavCause::from_e_param("sslv3Used"), NavCause::CertError);
        assert_eq!(
            NavCause::from_e_param("inadequateSecurityError"),
            NavCause::CertError
        );
    }

    #[test]
    fn classify_neterror_conn_reset() {
        assert_eq!(NavCause::from_e_param("netReset"), NavCause::ConnReset);
        assert_eq!(
            NavCause::from_e_param("connectionRefused"),
            NavCause::ConnReset
        );
        assert_eq!(
            NavCause::from_e_param("connectionFailure"),
            NavCause::ConnReset
        );
    }

    #[test]
    fn classify_neterror_timeout() {
        assert_eq!(NavCause::from_e_param("netTimeout"), NavCause::Timeout);
    }

    #[test]
    fn classify_neterror_content_blocked() {
        assert_eq!(
            NavCause::from_e_param("cspBlocked"),
            NavCause::ContentBlocked
        );
        assert_eq!(
            NavCause::from_e_param("blockedByPolicy"),
            NavCause::ContentBlocked
        );
    }

    #[test]
    fn classify_neterror_unknown_passthrough() {
        assert_eq!(
            NavCause::from_e_param("someNewFirefoxCode"),
            NavCause::Unknown("someNewFirefoxCode".to_owned())
        );
    }

    // ── RdpError::Navigation ─────────────────────────────────────────────────

    #[test]
    fn rdp_error_navigation_display() {
        let err = RdpError::Navigation {
            cause: NavCause::DnsFail,
            url: "https://bad.invalid/".to_owned(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("https://bad.invalid/"),
            "must include url: {msg}"
        );
        assert!(msg.contains("DNS"), "must mention DNS: {msg}");
    }

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

    // ── New ActorErrorKind variants (iter-69) ───────────────────────────────

    #[test]
    fn actor_error_kind_from_code_new_variants() {
        assert_eq!(
            ActorErrorKind::from_code("missingParameter"),
            ActorErrorKind::MissingParameter
        );
        assert_eq!(
            ActorErrorKind::from_code("badParameterType"),
            ActorErrorKind::BadParameterType
        );
        assert_eq!(
            ActorErrorKind::from_code("notImplemented"),
            ActorErrorKind::NotImplemented
        );
        assert_eq!(
            ActorErrorKind::from_code("wrongOrder"),
            ActorErrorKind::WrongOrder
        );
        assert_eq!(
            ActorErrorKind::from_code("protocolError"),
            ActorErrorKind::ProtocolError
        );
        assert_eq!(
            ActorErrorKind::from_code("unknownError"),
            ActorErrorKind::UnknownError
        );
    }

    #[test]
    fn actor_error_kind_display_new_variants() {
        assert_eq!(
            ActorErrorKind::MissingParameter.to_string(),
            "missingParameter"
        );
        assert_eq!(
            ActorErrorKind::BadParameterType.to_string(),
            "badParameterType"
        );
        assert_eq!(ActorErrorKind::NotImplemented.to_string(), "notImplemented");
        assert_eq!(ActorErrorKind::WrongOrder.to_string(), "wrongOrder");
        assert_eq!(ActorErrorKind::ProtocolError.to_string(), "protocolError");
        assert_eq!(ActorErrorKind::UnknownError.to_string(), "unknownError");
    }

    /// AC: `actor_error_kind_terminal_for_param_errors`.
    #[test]
    fn actor_error_kind_terminal_for_param_errors() {
        for kind in [
            ActorErrorKind::MissingParameter,
            ActorErrorKind::BadParameterType,
            ActorErrorKind::NotImplemented,
            ActorErrorKind::WrongOrder,
            ActorErrorKind::ProtocolError,
            ActorErrorKind::UnknownError,
        ] {
            let err = ProtocolError::ActorError {
                actor: "conn0/actor1".to_owned(),
                kind: kind.clone(),
                error: kind.to_string(),
                message: String::new(),
            };
            assert!(
                !err.is_transient(),
                "{kind:?} must NOT be classified as transient"
            );
        }
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
