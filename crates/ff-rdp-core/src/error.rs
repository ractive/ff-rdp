use thiserror::Error;

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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
