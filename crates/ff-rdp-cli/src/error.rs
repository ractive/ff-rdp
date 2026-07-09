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
    /// Daemon protocol version does not match CLI.
    DaemonVersionMismatch { daemon: u32, cli: u32 },
    /// An actor has been destroyed (target navigated or closed) — exit 3.
    RdpActorDestroyed { actor: String },
    /// Navigation failed with a typed DNS/network cause — deterministic exit codes.
    ///
    /// Exit codes:
    /// - `DnsFail`       → 7
    /// - `CertError`     → 8
    /// - `ConnReset`     → 9
    /// - `Timeout`       → 10
    /// - `ContentBlocked`→ 11
    /// - `Unknown`       → 12
    Navigation {
        cause: ff_rdp_core::NavCause,
        url: String,
    },
    /// Bulk-frame announcement exceeded the configured `--max-frame-mb` cap —
    /// exit 78 (`EX_CONFIG` in BSD sysexits, "configuration error" — the
    /// remote announced a frame larger than ff-rdp is willing to accept).
    RdpBulkOversize { announced: u64, max: u64 },
    /// A requested feature is well-formed but cannot be honored in the current
    /// mode, and the CLI refuses to silently do the wrong thing (iter-101
    /// Theme D) — exit 1 (runtime/user error).
    ///
    /// `error_type` is a stable machine-readable discriminant (e.g.
    /// `"since_requires_daemon"`) so scripts and parity tests can branch on it
    /// without matching on the human-readable `message`.  Exit code 1 keeps it
    /// in the documented "runtime / user error" bucket and avoids colliding
    /// with clap's usage-error exit code 2.
    Unsupported {
        error_type: &'static str,
        message: String,
    },
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
            Self::DaemonVersionMismatch { .. } => "daemon_version_mismatch",
            Self::RdpActorDestroyed { .. } => "actor_destroyed",
            Self::Navigation { cause, .. } => match cause {
                ff_rdp_core::NavCause::DnsFail => "nav_dns_fail",
                ff_rdp_core::NavCause::CertError => "nav_cert_error",
                ff_rdp_core::NavCause::ConnReset => "nav_conn_reset",
                ff_rdp_core::NavCause::Timeout => "nav_timeout",
                ff_rdp_core::NavCause::ContentBlocked => "nav_content_blocked",
                ff_rdp_core::NavCause::Unknown(_) => "nav_unknown",
            },
            Self::RdpBulkOversize { .. } => "rdp_bulk_oversize",
            Self::Unsupported { error_type, .. } => error_type,
        }
    }

    /// Return the process exit code for this error.
    ///
    /// Navigation errors use dedicated exit codes (7–12) so callers can branch
    /// on them without parsing `error_type` strings.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Navigation { cause, .. } => match cause {
                ff_rdp_core::NavCause::DnsFail => 7,
                ff_rdp_core::NavCause::CertError => 8,
                ff_rdp_core::NavCause::ConnReset => 9,
                ff_rdp_core::NavCause::Timeout => 10,
                ff_rdp_core::NavCause::ContentBlocked => 11,
                ff_rdp_core::NavCause::Unknown(_) => 12,
            },
            Self::RdpBulkOversize { .. } => 78,
            // Everything else — including `Unsupported` (well-formed but not
            // honorable here) — falls in the runtime/user-error bucket (exit 1),
            // never clap's usage exit code 2.
            _ => 1,
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
            Self::Diagnostics { message, .. } | Self::Unsupported { message, .. } => {
                write!(f, "{message}")
            }
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
            Self::DaemonVersionMismatch { daemon, cli } => {
                write!(
                    f,
                    "daemon protocol version mismatch: daemon={daemon}, cli={cli}.\n\
                     Stop the running daemon (`ff-rdp daemon stop`) so a fresh one is started."
                )
            }
            Self::RdpActorDestroyed { actor } => {
                write!(
                    f,
                    "actor {actor} has been destroyed — the target navigated or closed.\n\
                     hint: retry the command; ff-rdp will reconnect to the new target."
                )
            }
            Self::Navigation { cause, url } => {
                write!(
                    f,
                    "navigate: navigation to '{url}' failed: {cause}\n\
                     hint: check the URL, DNS, or network connectivity"
                )
            }
            Self::RdpBulkOversize { announced, max } => {
                write!(
                    f,
                    "RDP bulk frame too large: announced {announced} bytes, cap {max} bytes.\n\
                     hint: raise --max-frame-mb if the peer is trusted and the transfer is legitimate (e.g. a large heap-snapshot)."
                )
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
            ff_rdp_core::RdpError::ActorDestroyed { actor } => Self::RdpActorDestroyed {
                actor: actor.to_string(),
            },
            ff_rdp_core::RdpError::Navigation { cause, url } => Self::Navigation { cause, url },
            ff_rdp_core::RdpError::Spec { reason } => {
                Self::User(format!("spec violation: {reason}"))
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
                ff_rdp_core::ActorErrorKind::NotImplemented => Self::User(format!(
                    "{err} — Firefox accepts this method name but has not implemented it.\n\
                     hint: try a newer Firefox build, or report this as a missing feature."
                )),
                ff_rdp_core::ActorErrorKind::MissingParameter
                | ff_rdp_core::ActorErrorKind::BadParameterType
                | ff_rdp_core::ActorErrorKind::WrongOrder
                | ff_rdp_core::ActorErrorKind::ProtocolError
                | ff_rdp_core::ActorErrorKind::UnknownError => Self::RdpProtocol {
                    actor: actor.clone(),
                    name: error.clone(),
                    message: message.clone(),
                },
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
            // iter-75 M-1: oversize bulk-frame announcements map to a
            // dedicated variant so the CLI exits with EX_CONFIG (78) and a
            // hint pointing at --max-frame-mb.
            ff_rdp_core::ProtocolError::BulkFrameTooLarge { announced, max } => {
                Self::RdpBulkOversize {
                    announced: *announced,
                    max: *max,
                }
            }
            // EvalNavigatedDuringEval, BulkPacketUnsupported, BulkPacketUnexpected,
            // ActorChannelFull, and InvalidState remain Internal.
            // Bulk frames are not something the CLI handles; they are skipped
            // by the daemon and surfaced as Internal for direct-connect callers.
            // ActorChannelFull is a daemon-internal back-pressure signal; it
            // should not escape to end-user error paths.
            // InvalidState is a programming error (misuse of the API); surface
            // it as Internal so engineers see it in traces.
            ff_rdp_core::ProtocolError::EvalNavigatedDuringEval
            | ff_rdp_core::ProtocolError::BulkPacketUnsupported { .. }
            | ff_rdp_core::ProtocolError::BulkPacketUnexpected { .. }
            | ff_rdp_core::ProtocolError::ActorChannelFull { .. }
            | ff_rdp_core::ProtocolError::InvalidState(_) => {
                Self::Internal(anyhow::Error::new(err))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_version_mismatch_error_type() {
        let err = AppError::DaemonVersionMismatch { daemon: 0, cli: 1 };
        assert_eq!(err.error_type(), "daemon_version_mismatch");
    }

    #[test]
    fn daemon_version_mismatch_display_contains_versions() {
        let err = AppError::DaemonVersionMismatch { daemon: 0, cli: 1 };
        let msg = err.to_string();
        assert!(
            msg.contains("daemon=0") && msg.contains("cli=1"),
            "message should mention both versions: {msg}"
        );
    }

    #[test]
    fn daemon_version_mismatch_json_has_correct_error_type() {
        let err = AppError::DaemonVersionMismatch { daemon: 0, cli: 1 };
        let json = err.to_error_json();
        assert_eq!(
            json["error_type"].as_str(),
            Some("daemon_version_mismatch"),
            "JSON error_type must be 'daemon_version_mismatch'"
        );
        assert!(
            json["error"].as_str().unwrap_or("").contains("daemon=0"),
            "JSON error message should mention daemon version"
        );
    }

    // ── RdpActorDestroyed ────────────────────────────────────────────────────

    #[test]
    fn rdp_actor_destroyed_error_type() {
        let err = AppError::RdpActorDestroyed {
            actor: "conn0/tab1".to_owned(),
        };
        assert_eq!(err.error_type(), "actor_destroyed");
    }

    #[test]
    fn rdp_actor_destroyed_display_contains_actor_id() {
        let err = AppError::RdpActorDestroyed {
            actor: "conn0/tab1".to_owned(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("conn0/tab1"),
            "display must include the actor ID; got: {msg}"
        );
    }

    #[test]
    fn rdp_actor_destroyed_json_has_correct_error_type_and_actor() {
        let err = AppError::RdpActorDestroyed {
            actor: "conn0/tab1".to_owned(),
        };
        let json = err.to_error_json();
        assert_eq!(
            json["error_type"].as_str(),
            Some("actor_destroyed"),
            "JSON error_type must be 'actor_destroyed'"
        );
        assert!(
            json["error"].as_str().unwrap_or("").contains("conn0/tab1"),
            "JSON error message must include the actor ID"
        );
    }

    #[test]
    fn rdp_error_actor_destroyed_converts_to_app_error_rdp_actor_destroyed() {
        let actor = ff_rdp_core::ActorId::from("conn0/tab1");
        let rdp_err = ff_rdp_core::RdpError::ActorDestroyed {
            actor: actor.clone(),
        };
        let app_err = AppError::from(rdp_err);
        match app_err {
            AppError::RdpActorDestroyed { actor: ref a } => {
                assert_eq!(
                    a, "conn0/tab1",
                    "converted AppError must carry the same actor string"
                );
            }
            other => panic!("expected RdpActorDestroyed, got {other:?}"),
        }
    }
}
