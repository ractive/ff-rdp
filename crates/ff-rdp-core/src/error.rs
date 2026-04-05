use thiserror::Error;

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

    #[error("actor error from {actor}: {error} — {message}")]
    ActorError {
        actor: String,
        error: String,
        message: String,
    },
}
