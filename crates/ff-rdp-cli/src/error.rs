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
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User(msg) => write!(f, "{msg}"),
            Self::Internal(err) => write!(f, "{err:#}"),
            Self::Exit(code) => write!(f, "exit with code {code}"),
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
            ff_rdp_core::ProtocolError::ActorError { kind, .. } => match kind {
                ff_rdp_core::ActorErrorKind::UnknownActor => Self::User(format!(
                    "{err} — the tab may have been closed or navigated away; try again"
                )),
                ff_rdp_core::ActorErrorKind::WrongState => Self::User(format!(
                    "{err} — the target is in an unexpected state; try reloading the page"
                )),
                ff_rdp_core::ActorErrorKind::ThreadWouldRun => Self::User(format!(
                    "{err} — the page script is paused in the debugger; resume execution first"
                )),
                ff_rdp_core::ActorErrorKind::Other(_) => Self::Internal(anyhow::Error::new(err)),
            },
            _ => Self::Internal(anyhow::Error::new(err)),
        }
    }
}
