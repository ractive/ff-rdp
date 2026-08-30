use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// The socket read deadline (ms) this invocation configured, published by
/// `main` from `--timeout` (iter-137 Theme B).
///
/// `0` means "not yet recorded" — only reachable from unit tests and library
/// consumers that never ran `main`.
static SOCKET_TIMEOUT_MS: AtomicU64 = AtomicU64::new(0);

/// Record the socket read deadline for this process (iter-137 Theme B).
///
/// Called once from `main` before any RDP connection is opened.  See
/// [`socket_timeout_ms`] for why a global is the right shape here.
pub(crate) fn remember_socket_timeout_ms(ms: u64) {
    SOCKET_TIMEOUT_MS.store(ms, Ordering::Relaxed);
}

/// The socket read deadline in ms, or `None` if `main` never recorded one.
///
/// `ff_rdp_core::ProtocolError::Timeout` is a **unit** variant: the transport
/// maps `ErrorKind::TimedOut` from the socket without carrying how long the
/// read waited, and every `recv()` in the codebase would have to be rewritten
/// to thread a duration through.  The waited duration is not unknown, though —
/// it is exactly the read timeout the CLI set on the socket, which is a
/// per-process constant taken from `--timeout`.  Publishing it here lets
/// `From<ProtocolError>` report the real number instead of the fabricated
/// `after_ms: 0` that rendered as "operation timed out after 0ms (phase: recv)"
/// — an error that told a dogfooding user nothing about what to change.
fn socket_timeout_ms() -> Option<u64> {
    match SOCKET_TIMEOUT_MS.load(Ordering::Relaxed) {
        0 => None,
        ms => Some(ms),
    }
}

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
    ///
    /// `after_ms` is the deadline that actually elapsed.  It is never 0 on any
    /// path the CLI constructs (iter-137 Theme B): a zero would claim the
    /// operation timed out instantly, which is never true and is exactly the
    /// nonsense the daemon-contention bug surfaced.
    RdpTimeout {
        phase: String,
        after_ms: u64,
        /// Extra context from the call site about *why* the reply never came
        /// (iter-220).
        ///
        /// The bare "operation timed out after 10000ms (phase: recv)" is true
        /// and useless: it names the socket, not the cause. A caller that knows
        /// what it was doing — collecting a page view immediately after an
        /// action that navigated, say — attaches that here with
        /// [`AppError::with_timeout_hint`], and it is rendered after the base
        /// message. `None` renders exactly as before.
        hint: Option<String>,
    },
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
    ///
    /// `details`, when `Some(object)`, is merged **into** the error envelope
    /// next to `error`/`error_type` rather than nested under a key (iter-160
    /// Theme A: an obscured click reports `matched`/`reachable`/`obscured_by`
    /// there, so the failing caller reads the covering element out of the same
    /// flat object it already parses `error_type` from). A non-object value is
    /// ignored — the envelope's top level is a map, and silently producing a
    /// different shape would be the exact dishonesty this iteration removes.
    Unsupported {
        error_type: &'static str,
        message: String,
        details: Option<serde_json::Value>,
    },
}

impl AppError {
    /// Attach call-site context to a timeout, explaining what the CLI was doing
    /// when the reply failed to arrive (iter-220 Theme C).
    ///
    /// A bare `operation timed out after 10000ms (phase: recv)` names the
    /// socket and nothing else — a user reading it cannot tell a slow page from
    /// a request Firefox silently dropped, and the second is what actually
    /// happens when a navigation destroys the docshell mid-request. A caller
    /// that knows which of the two it was in adds that here.
    ///
    /// A no-op on every other variant, so callers can apply it to a whole
    /// `Result` without matching first.
    #[must_use]
    pub fn with_timeout_hint(self, hint: impl Into<String>) -> Self {
        match self {
            Self::RdpTimeout {
                phase,
                after_ms,
                hint: None,
            } => Self::RdpTimeout {
                phase,
                after_ms,
                hint: Some(hint.into()),
            },
            Self::Timeout(msg) => Self::Timeout(format!("{msg}\n{}", hint.into())),
            other => other,
        }
    }

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
                // `NavCause` is `#[non_exhaustive]`; `Unknown` plus any future
                // cause map to the catch-all `nav_unknown` discriminant.
                _ => "nav_unknown",
            },
            Self::RdpBulkOversize { .. } => "rdp_bulk_oversize",
            Self::Unsupported { error_type, .. } => error_type,
        }
    }

    /// Return the process exit code for this error.
    ///
    /// This is the **single** exit-code authority (iter-105 Theme C): the former
    /// shadow `error_exit_code()` in `main.rs` — which returned 3/4/5/6/124 for
    /// variants this method used to collapse to 1 — has been folded in and
    /// deleted, so the documented table below is now the only source of truth.
    ///
    /// | Variant                                  | Exit code |
    /// |------------------------------------------|-----------|
    /// | `RdpProtocol` / `Connection` / `RdpActorDestroyed` | 3 |
    /// | `RdpShape`                               | 4         |
    /// | `RdpTimeout`                             | 5         |
    /// | `RdpTransport` / `RdpRemoteClosed`       | 6         |
    /// | `Navigation` (`DnsFail`…`Unknown`)       | 7–12      |
    /// | `RdpBulkOversize`                        | 78 (`EX_CONFIG`) |
    /// | `Timeout` (operation-level)              | 124       |
    /// | `Exit(code)`                             | `code`    |
    /// | `User` / `Internal` / `Diagnostics` / `DaemonVersionMismatch` / `Unsupported` | 1 |
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::RdpProtocol { .. } | Self::Connection(_) | Self::RdpActorDestroyed { .. } => 3,
            Self::RdpShape { .. } => 4,
            Self::RdpTimeout { .. } => 5,
            Self::RdpTransport(_) | Self::RdpRemoteClosed(_) => 6,
            Self::Navigation { cause, .. } => match cause {
                ff_rdp_core::NavCause::DnsFail => 7,
                ff_rdp_core::NavCause::CertError => 8,
                ff_rdp_core::NavCause::ConnReset => 9,
                ff_rdp_core::NavCause::Timeout => 10,
                ff_rdp_core::NavCause::ContentBlocked => 11,
                // `NavCause` is `#[non_exhaustive]`; `Unknown` plus any future
                // cause map to the catch-all navigation exit code 12.
                _ => 12,
            },
            Self::RdpBulkOversize { .. } => 78,
            Self::Timeout(_) => 124,
            Self::Exit(code) => *code,
            // Everything else — `User`, `Internal`, `Diagnostics`,
            // `DaemonVersionMismatch`, and `Unsupported` (well-formed but not
            // honorable here) — falls in the runtime/user-error bucket (exit 1),
            // never clap's usage exit code 2.
            Self::User(_)
            | Self::Internal(_)
            | Self::Diagnostics { .. }
            | Self::DaemonVersionMismatch { .. }
            | Self::Unsupported { .. } => 1,
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

        let mut json = if context.is_empty() {
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
        };

        // iter-160 Theme A: merge `Unsupported`'s structured details in flat,
        // never overwriting `error`/`error_type`/`context`.
        if let Self::Unsupported {
            details: Some(details),
            ..
        } = self
            && let (Some(obj), Some(extra)) = (json.as_object_mut(), details.as_object())
        {
            for (k, v) in extra {
                if k != "error" && k != "error_type" && k != "context" {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }

        json
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
            Self::RdpTimeout {
                phase,
                after_ms,
                hint,
            } if *after_ms == 0 => {
                // Defensive: the CLI no longer builds this shape (iter-137
                // Theme B), but a `0` must never render as a duration claim.
                write!(
                    f,
                    "operation timed out (phase: {phase}) — no reply arrived before the \
                     socket read deadline.\n\
                     hint: raise --timeout, or use --no-daemon for a private connection to Firefox.{}",
                    hint.as_ref().map_or_else(String::new, |h| format!("\n{h}"))
                )
            }
            Self::RdpTimeout {
                phase,
                after_ms,
                hint,
            } => {
                write!(
                    f,
                    "operation timed out after {after_ms}ms (phase: {phase}){}",
                    hint.as_ref().map_or_else(String::new, |h| format!("\n{h}"))
                )
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
            // iter-105 Theme A: `Protocol` now carries the full `ProtocolError`
            // losslessly.  Delegate to the `From<ProtocolError>` impl so the
            // `ActorErrorKind` discriminant, timeout phase/duration, and source
            // chains all reach the deterministic CLI mapping — no more
            // fabricated `after_ms: 0` or dropped `noSuchActor`/`wrongState`.
            ff_rdp_core::RdpError::Protocol(protocol_err) => Self::from(protocol_err),
            ff_rdp_core::RdpError::Shape {
                path,
                expected,
                got,
            } => Self::RdpShape {
                path,
                expected,
                got,
            },
            ff_rdp_core::RdpError::Timeout { phase, after_ms } => Self::RdpTimeout {
                phase,
                after_ms,
                hint: None,
            },
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
            // `RdpError` is `#[non_exhaustive]` (iter-105 Theme B): a variant
            // added upstream without a CLI mapping here falls back to a generic
            // internal error rather than failing to compile downstream.
            other => Self::Internal(anyhow::anyhow!("{other}")),
        }
    }
}

impl From<ff_rdp_core::ProtocolError> for AppError {
    fn from(err: ff_rdp_core::ProtocolError) -> Self {
        match &err {
            ff_rdp_core::ProtocolError::ConnectionFailed(_) => Self::Connection(format!(
                "{err}\nhint: run `ff-rdp doctor` for a full diagnostic, or `ff-rdp launch` to start Firefox."
            )),
            // iter-137 Theme B: report the socket read deadline that actually
            // elapsed instead of a fabricated `0`.  `--timeout` is what the
            // transport set on the socket, so it *is* the waited duration; the
            // fallback only applies to in-process callers that never ran
            // `main` (unit tests), and uses the same default `--timeout`
            // carries so the number is still the truth for a default run.
            ff_rdp_core::ProtocolError::Timeout => Self::RdpTimeout {
                phase: "recv".to_owned(),
                after_ms: socket_timeout_ms().unwrap_or(crate::cli::args::DEFAULT_TIMEOUT_MS),
                hint: None,
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
                // Parameter/order/protocol errors, `Other(_)`, and — since
                // `ActorErrorKind` is `#[non_exhaustive]` (iter-105 Theme B) —
                // any future kind map to the typed `RdpProtocol` variant so
                // callers get a deterministic exit code (3).  The explicit
                // wildcard satisfies the non-exhaustive requirement while
                // keeping the fallback behaviour unchanged.
                _ => Self::RdpProtocol {
                    actor: actor.clone(),
                    name: error.clone(),
                    message: message.clone(),
                },
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
            // iter-220: a target destroyed mid-request is not an internal
            // fault — it is the same "the actor you were talking to is gone"
            // condition `noSuchActor` reports, and callers that can re-resolve
            // the target (`page_view::attach`) branch on this variant.
            ff_rdp_core::ProtocolError::EvalTargetDestroyed { inner_window_id } => {
                Self::RdpActorDestroyed {
                    actor: format!("target(innerWindowId {inner_window_id})"),
                }
            }
            ff_rdp_core::ProtocolError::EvalNavigatedDuringEval
            | ff_rdp_core::ProtocolError::BulkPacketUnsupported { .. }
            | ff_rdp_core::ProtocolError::BulkPacketUnexpected { .. }
            | ff_rdp_core::ProtocolError::ActorChannelFull { .. }
            | ff_rdp_core::ProtocolError::InvalidState(_) => {
                Self::Internal(anyhow::Error::new(err))
            }
            // `ProtocolError` is `#[non_exhaustive]` (iter-105 Theme B): a
            // variant added upstream without an explicit mapping surfaces as an
            // internal error rather than breaking this match.
            _ => Self::Internal(anyhow::Error::new(err)),
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

    // ── with_timeout_hint (iter-220 Theme C) ────────────────────────────────

    #[test]
    fn with_timeout_hint_appends_to_an_rdp_timeout() {
        let rendered = AppError::RdpTimeout {
            phase: "recv".to_owned(),
            after_ms: 10_000,
            hint: None,
        }
        .with_timeout_hint("the page navigated mid-collection")
        .to_string();
        assert!(
            rendered.contains("operation timed out after 10000ms (phase: recv)"),
            "the base message must survive: {rendered}"
        );
        assert!(
            rendered.contains("the page navigated mid-collection"),
            "the hint must be rendered: {rendered}"
        );
    }

    #[test]
    fn with_timeout_hint_does_not_overwrite_an_existing_hint() {
        // An inner call site's diagnosis is closer to the failure than an
        // outer one's; the outer must not clobber it.
        let rendered = AppError::RdpTimeout {
            phase: "recv".to_owned(),
            after_ms: 10_000,
            hint: Some("first diagnosis".to_owned()),
        }
        .with_timeout_hint("second diagnosis")
        .to_string();
        assert!(rendered.contains("first diagnosis"), "{rendered}");
        assert!(!rendered.contains("second diagnosis"), "{rendered}");
    }

    #[test]
    fn with_timeout_hint_is_a_noop_on_other_variants() {
        let err = AppError::User("bad selector".to_owned()).with_timeout_hint("irrelevant");
        let rendered = err.to_string();
        assert_eq!(rendered, "bad selector");
        assert_eq!(err.error_type(), "User");
    }

    #[test]
    fn eval_target_destroyed_maps_to_actor_destroyed() {
        // iter-220: `page_view::collect_settled` branches on this variant to
        // re-resolve the target and retry, so the mapping is load-bearing.
        let err = AppError::from(ff_rdp_core::ProtocolError::EvalTargetDestroyed {
            inner_window_id: 15_032_385_539,
        });
        match err {
            AppError::RdpActorDestroyed { ref actor } => {
                assert!(
                    actor.contains("15032385539"),
                    "the destroyed document must be named: {actor}"
                );
            }
            ref other => panic!("expected RdpActorDestroyed, got {other:?}"),
        }
        assert_eq!(err.exit_code(), 3);
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

    // ── iter-105 Theme A: lossless ProtocolError bridge ─────────────────────

    /// AC: `unit_protocol_error_roundtrip_preserves_kind` — an
    /// `ActorErrorKind::WrongState` protocol error must stay distinguishable
    /// from `NoSuchActor` (`UnknownActor`) after crossing
    /// `ProtocolError -> RdpError -> AppError`, and a `ProtocolError::Timeout`
    /// must NOT be fabricated into `after_ms: 0`.
    #[test]
    fn unit_protocol_error_roundtrip_preserves_kind() {
        // WrongState → RdpProtocol (exit 3), distinct from UnknownActor → User.
        let wrong_state = ff_rdp_core::ProtocolError::ActorError {
            actor: "conn0/actor1".to_owned(),
            kind: ff_rdp_core::ActorErrorKind::WrongState,
            error: "wrongState".to_owned(),
            message: "bad state".to_owned(),
        };
        let rdp: ff_rdp_core::RdpError = wrong_state.into();
        let app = AppError::from(rdp);
        assert!(
            matches!(app, AppError::User(_)),
            "WrongState maps to a User hint; got {app:?}"
        );
        assert_eq!(app.exit_code(), 1, "WrongState is a runtime/user error");

        let unknown_actor = ff_rdp_core::ProtocolError::ActorError {
            actor: "conn0/actor1".to_owned(),
            kind: ff_rdp_core::ActorErrorKind::UnknownActor,
            error: "noSuchActor".to_owned(),
            message: String::new(),
        };
        let rdp: ff_rdp_core::RdpError = unknown_actor.into();
        let app_unknown = AppError::from(rdp);
        // Both are User hints but carry distinct messages — the discriminant
        // survived (WrongState mentions "unexpected state", UnknownActor mentions
        // "closed or navigated"), proving the kind was not flattened away.
        assert_ne!(
            app.to_string(),
            app_unknown.to_string(),
            "WrongState and UnknownActor must remain distinguishable after the bridge"
        );

        // Timeout must pass through as an RdpTimeout with the recv phase — not a
        // fabricated `after_ms: 0` on a top-level RdpError::Timeout.
        let rdp: ff_rdp_core::RdpError = ff_rdp_core::ProtocolError::Timeout.into();
        let app = AppError::from(rdp);
        match app {
            AppError::RdpTimeout { ref phase, .. } => {
                assert_eq!(
                    phase, "recv",
                    "Timeout bridges via the ProtocolError mapping"
                );
            }
            other => panic!("expected RdpTimeout, got {other:?}"),
        }
        assert_eq!(app.exit_code(), 5, "RDP timeout exit code");
    }

    // ── iter-137 Theme B: no fabricated durations in timeout errors ─────────

    /// AC: `unit_timeout_error_never_reports_zero_ms` — bridging
    /// `ProtocolError::Timeout` must report the socket read deadline that
    /// actually elapsed. Dogfooding kept hitting
    /// "operation timed out after 0ms (phase: recv)" from contended daemon
    /// commands: a duration of zero is never true and tells the user nothing
    /// about what to change.
    #[test]
    fn unit_timeout_error_never_reports_zero_ms() {
        remember_socket_timeout_ms(7_500);

        let app = AppError::from(ff_rdp_core::ProtocolError::Timeout);
        match app {
            AppError::RdpTimeout {
                ref phase,
                after_ms,
                ..
            } => {
                assert_eq!(phase, "recv");
                assert_eq!(
                    after_ms, 7_500,
                    "the reported duration must be the socket read deadline the CLI set"
                );
            }
            ref other => panic!("expected RdpTimeout, got {other:?}"),
        }
        let rendered = app.to_string();
        assert!(
            !rendered.contains("after 0ms"),
            "no fabricated zero duration: {rendered}"
        );
        assert!(
            rendered.contains("7500ms"),
            "the real deadline must appear in the message: {rendered}"
        );

        // Reset so the fallback branch is observable too: an in-process caller
        // that never ran `main` still gets the default `--timeout`, never 0.
        remember_socket_timeout_ms(0);
        let fallback = AppError::from(ff_rdp_core::ProtocolError::Timeout);
        match fallback {
            AppError::RdpTimeout { after_ms, .. } => assert_eq!(
                after_ms,
                crate::cli::args::DEFAULT_TIMEOUT_MS,
                "unrecorded deadline falls back to the documented --timeout default"
            ),
            ref other => panic!("expected RdpTimeout, got {other:?}"),
        }
    }

    /// AC: `unit_zero_after_ms_renders_without_a_duration_claim` — even if a
    /// `0` ever reaches `Display` (a future construction site, a decoded
    /// payload), it must not render as "after 0ms".
    #[test]
    fn unit_zero_after_ms_renders_without_a_duration_claim() {
        let rendered = AppError::RdpTimeout {
            hint: None,
            phase: "recv".to_owned(),
            after_ms: 0,
        }
        .to_string();
        assert!(
            !rendered.contains("0ms"),
            "a zero must never be presented as an elapsed duration: {rendered}"
        );
        assert!(
            rendered.contains("phase: recv"),
            "the phase is still useful context: {rendered}"
        );
    }

    // ── iter-105 Theme C: one exit-code map + frozen discriminants ──────────

    /// AC: `unit_exit_code_and_error_type_frozen` — every `AppError` variant's
    /// exit code AND `error_type` string is pinned exactly as shipped.  Renaming
    /// a discriminant or changing an exit code is a breaking change we are not
    /// taking; new discriminants MUST be snake_case (see the assertion below).
    #[test]
    fn unit_exit_code_and_error_type_frozen() {
        use ff_rdp_core::NavCause;

        // (variant instance, expected exit code, expected error_type)
        let table: Vec<(AppError, i32, &str)> = vec![
            (AppError::User("x".to_owned()), 1, "User"),
            (AppError::Internal(anyhow::anyhow!("x")), 1, "Internal"),
            (AppError::Exit(1), 1, "Exit"),
            (AppError::Exit(42), 42, "Exit"),
            (AppError::Connection("x".to_owned()), 3, "Connection"),
            (AppError::Timeout("x".to_owned()), 124, "Timeout"),
            (
                AppError::Diagnostics {
                    message: "x".to_owned(),
                    payload: serde_json::Value::Null,
                },
                1,
                "User",
            ),
            (
                AppError::RdpProtocol {
                    actor: "a".to_owned(),
                    name: "n".to_owned(),
                    message: "m".to_owned(),
                },
                3,
                "Protocol",
            ),
            (
                AppError::RdpShape {
                    path: "p".to_owned(),
                    expected: "e".to_owned(),
                    got: "g".to_owned(),
                },
                4,
                "Shape",
            ),
            (
                AppError::RdpTimeout {
                    hint: None,
                    phase: "recv".to_owned(),
                    after_ms: 10,
                },
                5,
                "Timeout",
            ),
            (AppError::RdpTransport("x".to_owned()), 6, "Transport"),
            (AppError::RdpRemoteClosed("x".to_owned()), 6, "RemoteClosed"),
            (
                AppError::DaemonVersionMismatch { daemon: 0, cli: 1 },
                1,
                "daemon_version_mismatch",
            ),
            (
                AppError::RdpActorDestroyed {
                    actor: "a".to_owned(),
                },
                3,
                "actor_destroyed",
            ),
            (
                AppError::Navigation {
                    cause: NavCause::DnsFail,
                    url: "u".to_owned(),
                },
                7,
                "nav_dns_fail",
            ),
            (
                AppError::Navigation {
                    cause: NavCause::CertError,
                    url: "u".to_owned(),
                },
                8,
                "nav_cert_error",
            ),
            (
                AppError::Navigation {
                    cause: NavCause::ConnReset,
                    url: "u".to_owned(),
                },
                9,
                "nav_conn_reset",
            ),
            (
                AppError::Navigation {
                    cause: NavCause::Timeout,
                    url: "u".to_owned(),
                },
                10,
                "nav_timeout",
            ),
            (
                AppError::Navigation {
                    cause: NavCause::ContentBlocked,
                    url: "u".to_owned(),
                },
                11,
                "nav_content_blocked",
            ),
            (
                AppError::Navigation {
                    cause: NavCause::Unknown("x".to_owned()),
                    url: "u".to_owned(),
                },
                12,
                "nav_unknown",
            ),
            (
                AppError::RdpBulkOversize {
                    announced: 100,
                    max: 10,
                },
                78,
                "rdp_bulk_oversize",
            ),
            (
                AppError::Unsupported {
                    error_type: "since_requires_daemon",
                    message: "x".to_owned(),
                    details: None,
                },
                1,
                "since_requires_daemon",
            ),
        ];

        for (err, expected_exit, expected_type) in table {
            assert_eq!(
                err.exit_code(),
                expected_exit,
                "exit code for {err:?} is frozen at {expected_exit}"
            );
            assert_eq!(
                err.error_type(),
                expected_type,
                "error_type for {err:?} is frozen at {expected_type:?}"
            );
            // JSON envelope must echo the same frozen discriminant.
            assert_eq!(
                err.to_error_json()["error_type"].as_str(),
                Some(expected_type),
                "JSON error_type must match the frozen discriminant for {err:?}"
            );
        }
    }
}
