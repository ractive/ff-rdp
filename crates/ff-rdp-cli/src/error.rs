use std::fmt;

#[derive(Debug)]
pub enum AppError {
    /// User-facing error (wrong arguments, tab not found, etc.)
    User(String),
    /// Internal/unexpected error
    Internal(anyhow::Error),
    /// Exit with specific code (reserved for commands that need a precise exit code)
    #[allow(dead_code)]
    Exit(i32),
    /// Connection failure (could not reach Firefox or daemon) — exit 3
    Connection(String),
    /// Operation timed out — exit 124
    Timeout(String),
    /// Assertion failure with a structured diagnostics payload.
    ///
    /// The `message` field is the human-readable failure description; `payload`
    /// is a `serde_json::Value` that the script runner surfaces as
    /// `"diagnostics"` in the NDJSON step output.  Using a typed variant avoids
    /// embedding diagnostics in the error string and parsing them back out.
    Diagnostics {
        message: String,
        payload: serde_json::Value,
    },
    // ── Typed RdpError variants — deterministic exit codes ─────────────────
    /// Firefox actor returned an error packet — exit 3.
    RdpProtocol {
        actor: String,
        name: String,
        message: String,
    },
    /// A received packet does not have the expected JSON shape — exit 4.
    RdpShape {
        path: String,
        expected: String,
        got: String,
    },
    /// RDP-level timeout (phase/after_ms context) — exit 5.
    RdpTimeout { phase: String, after_ms: u64 },
    /// Low-level transport I/O failure — exit 6.
    RdpTransport(String),
    /// Remote peer closed the connection — exit 6.
    RdpRemoteClosed(String),
}

impl AppError {
    /// Return the machine-readable discriminant string for JSON error output.
    pub fn error_type(&self) -> &'static str {
        match self {
            Self::User(_) | Self::Diagnostics { .. } => "User",
            Self::Internal(_) => "Internal",
            Self::Exit(_) => "Exit",
            Self::Connection(_) => "Connection",
            Self::Timeout(_) | Self::RdpTimeout { .. } => "Timeout",
            Self::RdpProtocol { .. } => "Protocol",
            Self::RdpShape { .. } => "Shape",
            Self::RdpTransport(_) => "Transport",
            Self::RdpRemoteClosed(_) => "RemoteClosed",
        }
    }

    /// Collect context chain from an anyhow error into a Vec of strings.
    fn context_chain(err: &anyhow::Error) -> Vec<String> {
        err.chain()
            .skip(1) // Skip the root error itself (already in "error" field).
            .map(std::string::ToString::to_string)
            .collect()
    }

    /// Render this error as a JSON value suitable for `meta.error_type` in the
    /// CLI output envelope.  Used by the output pipeline to attach error
    /// metadata when a command fails.
    pub fn to_error_json(&self) -> serde_json::Value {
        let error_type = self.error_type();
        let message = self.to_string();

        let context: Vec<String> = if let Self::Internal(err) = self {
            Self::context_chain(err)
        } else {
            Vec::new()
        };

        if context.is_empty() {
            serde_json::json!({
                "error": message,
                "error_type": error_type,
            })
        } else {
            serde_json::json!({
                "error": message,
                "error_type": error_type,
                "context": context,
            })
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Internal(err) => write!(f, "{err:#}"),
            Self::Exit(code) => write!(f, "exit with code {code}"),
            Self::User(msg)
            | Self::Connection(msg)
            | Self::Timeout(msg)
            | Self::RdpTransport(msg)
            | Self::RdpRemoteClosed(msg) => write!(f, "{msg}"),
            Self::Diagnostics { message, .. } => write!(f, "{message}"),
            Self::RdpProtocol {
                actor,
                name,
                message,
            } => {
                write!(f, "actor error from {actor}: {name} — {message}")
            }
            Self::RdpShape {
                path,
                expected,
                got,
            } => {
                write!(
                    f,
                    "unexpected packet shape at {path}: expected {expected}, got {got}"
                )
            }
            Self::RdpTimeout { phase, after_ms } => {
                write!(f, "operation timed out after {after_ms}ms (phase: {phase})")
            }
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        Self::Internal(err)
    }
}

impl From<ff_rdp_core::RdpError> for AppError {
    fn from(err: ff_rdp_core::RdpError) -> Self {
        match err {
            ff_rdp_core::RdpError::Protocol {
                actor,
                name,
                message,
            } => Self::RdpProtocol {
                actor,
                name,
                message,
            },
            ff_rdp_core::RdpError::Shape {
                path,
                expected,
                got,
            } => Self::RdpShape {
                path,
                expected,
                got,
            },
            ff_rdp_core::RdpError::Timeout { phase, after_ms } => {
                Self::RdpTimeout { phase, after_ms }
            }
            ff_rdp_core::RdpError::Transport(io_err) => Self::RdpTransport(io_err.to_string()),
            ff_rdp_core::RdpError::RemoteClosed => {
                Self::RdpRemoteClosed("remote connection closed unexpectedly".to_owned())
            }
        }
    }
}

impl From<ff_rdp_core::ProtocolError> for AppError {
    fn from(err: ff_rdp_core::ProtocolError) -> Self {
        match &err {
            ff_rdp_core::ProtocolError::ConnectionFailed(_) => Self::Connection(format!(
                "{err}\nhint: run `ff-rdp doctor` for a full diagnostic, or `ff-rdp launch` to start Firefox."
            )),
            ff_rdp_core::ProtocolError::Timeout => Self::RdpTimeout {
                phase: "recv".to_owned(),
                after_ms: 0,
            },
            ff_rdp_core::ProtocolError::ActorError {
                kind,
                actor,
                error,
                message,
            } => match kind {
                ff_rdp_core::ActorErrorKind::UnknownActor => Self::User(format!(
                    "{err} — the tab may have been closed or navigated away; try again.\n\
                     hint: run `ff-rdp doctor` if this keeps happening — the connection may be stale."
                )),
                ff_rdp_core::ActorErrorKind::WrongState => Self::User(format!(
                    "{err} — the target is in an unexpected state; try reloading the page.\n\
                     hint: run `ff-rdp doctor` to inspect connection state."
                )),
                ff_rdp_core::ActorErrorKind::ThreadWouldRun => Self::User(format!(
                    "{err} — the page script is paused in the debugger; resume execution first.\n\
                     hint: run `ff-rdp eval 'debugger; void 0'` then continue in DevTools, or close DevTools."
                )),
                ff_rdp_core::ActorErrorKind::UnrecognizedPacketType => Self::User(format!(
                    "{err} — the method is not supported by this Firefox version.\n\
                     hint: run `ff-rdp doctor` to check Firefox version compatibility."
                )),
                ff_rdp_core::ActorErrorKind::Other(_) => {
                    // Map to typed Protocol variant so callers get a deterministic exit code.
                    Self::RdpProtocol {
                        actor: actor.clone(),
                        name: error.clone(),
                        message: message.clone(),
                    }
                }
            },
            // I/O errors on the established connection map to Transport (exit 6).
            ff_rdp_core::ProtocolError::RecvFailed(_)
            | ff_rdp_core::ProtocolError::SendFailed(_) => Self::RdpTransport(format!("{err}")),
            // Wire-framing errors map to RdpShape (exit 4).
            ff_rdp_core::ProtocolError::InvalidPacket(detail) => Self::RdpShape {
                path: "frame".to_owned(),
                expected: "valid RDP frame".to_owned(),
                got: detail.clone(),
            },
            ff_rdp_core::ProtocolError::FrameTooLarge { declared, max } => Self::RdpShape {
                path: "frame.length".to_owned(),
                expected: format!("<= {max} bytes"),
                got: format!("{declared} bytes"),
            },
            // EvalNavigatedDuringEval remains Internal.
            ff_rdp_core::ProtocolError::EvalNavigatedDuringEval => {
                Self::Internal(anyhow::Error::new(err))
            }
        }
    }
}
