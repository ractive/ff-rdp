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
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Internal(err) => write!(f, "{err:#}"),
            Self::Exit(code) => write!(f, "exit with code {code}"),
            Self::User(msg) | Self::Connection(msg) | Self::Timeout(msg) => write!(f, "{msg}"),
            Self::Diagnostics { message, .. } => write!(f, "{message}"),
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        Self::Internal(err)
    }
}

impl From<ff_rdp_core::ProtocolError> for AppError {
    fn from(err: ff_rdp_core::ProtocolError) -> Self {
        match &err {
            ff_rdp_core::ProtocolError::ConnectionFailed(_) => Self::Connection(format!(
                "{err}\nhint: run `ff-rdp doctor` for a full diagnostic, or `ff-rdp launch` to start Firefox."
            )),
            ff_rdp_core::ProtocolError::Timeout => {
                Self::Timeout(format!("{err} — try increasing --timeout"))
            }
            ff_rdp_core::ProtocolError::ActorError { kind, .. } => match kind {
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
                ff_rdp_core::ActorErrorKind::Other(_) => Self::Internal(anyhow::Error::new(err)),
            },
            _ => Self::Internal(anyhow::Error::new(err)),
        }
    }
}
