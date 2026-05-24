use std::sync::{Arc, Mutex};
use std::time::Duration;

use ff_rdp_core::{
    ActorId, DeviceActor, FrontKind, ProtocolError, RdpConnection, RdpTransport, Registry,
    ResourceCommand, RootActor, Session, TabActor, TargetInfo,
};
use serde_json::json;

use crate::cli::args::Cli;
use crate::daemon::client::{ConnectionTarget, resolve_connection_target};
use crate::error::AppError;

/// Shared state after connecting to Firefox and resolving a tab target.
///
/// Owns a [`Session`] (transport + actor registry) and the resolved tab
/// metadata.  The registry is pre-populated with the target and console fronts
/// discovered during the `getTarget` handshake.
pub struct ConnectedTab {
    session: Session,
    /// Firefox major version, if detectable from the greeting or device actor.
    ///
    /// Currently the version is also stored in the process-global
    /// `connection_meta::remembered_version()`.  This field is kept for
    /// potential future use (e.g. exposing it via `ConnectedTab::firefox_version()`
    /// without a global read).
    #[allow(dead_code)]
    pub(crate) firefox_version: Option<u32>,
    pub(crate) target: TargetInfo,
    tab_actor: ActorId,
    /// Whether this connection goes through the daemon proxy.
    pub(crate) via_daemon: bool,
}

/// Connect to Firefox (directly or via daemon), resolve the target tab, and
/// call `getTarget` on it.
///
/// When a daemon is available, the CLI connects to the daemon's proxy port on
/// localhost.  The daemon transparently forwards RDP frames, so the rest of
/// the protocol handshake is identical.
pub fn connect_and_get_target(cli: &Cli) -> Result<ConnectedTab, AppError> {
    let target = resolve_connection_target(&cli.host, cli.port, cli.daemon_timeout, cli.no_daemon);

    let (connect_host, connect_port, via_daemon, auth_token, deferred_warning) = match target {
        ConnectionTarget::Daemon { port, auth_token } => {
            ("127.0.0.1".to_owned(), port, true, Some(auth_token), None)
        }
        ConnectionTarget::Direct { deferred_warning } => {
            (cli.host.clone(), cli.port, false, None, deferred_warning)
        }
    };

    let connection = connect_to_firefox(
        &connect_host,
        connect_port,
        cli,
        via_daemon,
        auth_token.as_deref(),
    )
    .inspect_err(|_| {
        // The direct fallback failed too — surface the original
        // daemon-side warning alongside the connection error so the user
        // sees the full picture.
        if let Some(w) = &deferred_warning {
            eprintln!("{w}");
        }
    })?;

    handshake_and_resolve_tab(connection, cli, via_daemon)
}

/// Like [`connect_and_get_target`] but always bypasses the daemon and
/// connects directly to Firefox.  Use this for commands (e.g. screenshot)
/// whose protocol interactions are incompatible with the daemon proxy.
pub fn connect_direct(cli: &Cli) -> Result<ConnectedTab, AppError> {
    let connection = connect_to_firefox(&cli.host, cli.port, cli, false, None)?;

    handshake_and_resolve_tab(connection, cli, false)
}

/// Establish a TCP connection to Firefox (or daemon proxy) and produce
/// user-friendly errors on failure.
///
/// When `auth_token` is `Some`, the token is sent as the very first frame
/// (`{"auth": "<token>"}`) to authenticate with the daemon before the normal
/// RDP handshake begins.
fn connect_to_firefox(
    host: &str,
    port: u16,
    cli: &Cli,
    via_daemon: bool,
    auth_token: Option<&str>,
) -> Result<RdpConnection, AppError> {
    let timeout = Duration::from_millis(cli.timeout);

    if let Some(token) = auth_token {
        // Daemon auth path:
        // 1. Open raw TCP (no greeting read yet).
        // 2. Send auth frame.
        // 3. Then proceed with the normal connect (reads greeting).
        let mut transport =
            RdpTransport::connect_raw(host, port, timeout).map_err(|e| match e {
                ProtocolError::ConnectionFailed(_) | ProtocolError::Timeout => {
                    AppError::Connection(format!(
                        "could not connect to daemon on port {port} — try --no-daemon to connect directly to Firefox.\n\
                         hint: run `ff-rdp doctor` to inspect daemon health."
                    ))
                }
                other => AppError::from(other),
            })?;

        // Send the auth frame before any other request.
        transport
            .send(&json!({"auth": token}))
            .map_err(|e| AppError::Internal(anyhow::anyhow!("sending daemon auth frame: {e}")))?;

        // Read the greeting (daemon sends it after successful auth).
        // If the daemon closes the connection here it rejected our auth token.
        // If it times out, the daemon is overloaded or the socket is stale.
        let greeting = transport.recv().map_err(|e| {
            // Distinguish: a read timeout (or transient I/O error) means the
            // daemon isn't responding, not that the token was wrong
            // (E1 — honest error messages).
            if e.is_transient() {
                AppError::Timeout(
                    "daemon did not respond within the timeout after auth — \
                     the daemon may be overloaded or the connection is stale.\n\
                     hint: run `ff-rdp daemon stop` then retry, or use --no-daemon."
                        .to_string(),
                )
            } else {
                AppError::User(format!(
                    "daemon auth rejected (wrong token): {e}\n\
                     hint: stop the running daemon (`ff-rdp daemon stop`) or use --no-daemon."
                ))
            }
        })?;

        // Verify protocol version — a mismatch means the running daemon is a
        // different build than this CLI binary.  A missing or non-numeric
        // `protocol_version` field is also treated as a mismatch (reported as
        // daemon=0) — a pre-versioning daemon predates this CLI and must be
        // restarted to pick up the new handshake.
        let daemon_version = greeting
            .get("protocol_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0);
        let expected = crate::daemon::server::DAEMON_PROTOCOL_VERSION;
        if daemon_version != expected {
            return Err(AppError::DaemonVersionMismatch {
                daemon: daemon_version,
                cli: expected,
            });
        }

        // Now wrap in RdpConnection. We already consumed the greeting; pass it
        // through so the Firefox version stays available for connection_meta.
        return Ok(RdpConnection::from_authenticated_transport(
            transport, &greeting,
        ));
    }

    RdpConnection::connect(host, port, timeout).map_err(|e| match e {
        ProtocolError::ConnectionFailed(_) | ProtocolError::Timeout if !via_daemon => {
            AppError::Connection(format!(
                "could not connect to Firefox at {}:{} — is Firefox running with --start-debugger-server {}?\n\
                 hint: run `ff-rdp doctor` for a full diagnostic, or `ff-rdp launch` to start Firefox with debugging enabled.",
                cli.host, cli.port, cli.port
            ))
        }
        ProtocolError::ConnectionFailed(_) | ProtocolError::Timeout if via_daemon => {
            AppError::Connection(format!(
                "could not connect to daemon on port {port} — try --no-daemon to connect directly to Firefox.\n\
                 hint: run `ff-rdp doctor` to inspect daemon health."
            ))
        }
        other => AppError::from(other),
    })
}

/// Run the RDP handshake: list tabs, resolve the target tab, call `getTarget`,
/// and register the discovered actor fronts in the session registry.
fn handshake_and_resolve_tab(
    mut connection: RdpConnection,
    cli: &Cli,
    via_daemon: bool,
) -> Result<ConnectedTab, AppError> {
    // When the RDP greeting omits the `ua` field (some Firefox builds strip
    // it), try the device actor's `getDescription` as a version fallback.
    // This ensures `remembered_version()` is populated for all downstream
    // callers (e.g. `version_mismatch_message()` in the screenshot path) and
    // that the compatibility warning is emitted based on the resolved
    // version, not the (absent) greeting one.
    let greeting_version = connection.firefox_version();
    let effective_version = if greeting_version.is_none() {
        DeviceActor::query_version(connection.transport_mut())
            .unwrap_or(None)
            .or(greeting_version)
    } else {
        greeting_version
    };
    if effective_version != greeting_version {
        connection.set_firefox_version(effective_version);
    }
    crate::connection_meta::remember_version(effective_version);

    let tabs = RootActor::list_tabs(connection.transport_mut()).map_err(AppError::from)?;

    let tab = crate::tab_target::resolve_tab_with_context(
        &tabs,
        cli.tab.as_deref(),
        cli.tab_id.as_deref(),
        &cli.host,
        cli.port,
    )?;
    let tab_actor = tab.actor.clone();

    let target_info =
        TabActor::get_target(connection.transport_mut(), &tab_actor).map_err(AppError::from)?;

    // Consume the RdpConnection and build a Session so all subsequent
    // actor interactions use the registry for front resolution.
    let firefox_version = connection.firefox_version();
    let transport = connection.into_transport();
    let session = Session::new(transport);

    // Register the target front (WindowGlobalTarget) and its console front.
    // These two are always present after getTarget.
    register_target_fronts(session.registry(), &target_info);

    Ok(ConnectedTab {
        session,
        firefox_version,
        target: target_info,
        tab_actor,
        via_daemon,
    })
}

/// Register target and console fronts in the registry after `getTarget`.
///
/// Called once per `handshake_and_resolve_tab` and again after each
/// `refresh_target` to keep the registry in sync with Firefox's actor state.
pub(crate) fn register_target_fronts(registry: &Arc<Registry>, target: &TargetInfo) {
    let target_id = target.actor.clone();
    let console_id = target.console_actor.clone();

    registry.register(target_id.clone(), FrontKind::Target, None);
    registry.register(console_id, FrontKind::Console, Some(target_id));
}

impl ConnectedTab {
    /// Borrow the underlying RDP transport.
    pub fn transport_mut(&mut self) -> &mut RdpTransport {
        self.session.transport_mut()
    }

    /// Borrow the session (transport + registry).
    ///
    /// Used by theme C/D agents (iter-61t) to access both transport and
    /// registry together without separate borrows.
    #[allow(dead_code)]
    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    /// Return a reference to the actor registry for this session.
    ///
    /// Callers use this to look up pre-registered fronts or to register
    /// additional fronts (e.g. after acquiring a watcher actor).
    pub fn registry(&self) -> &Arc<Registry> {
        self.session.registry()
    }

    pub fn target_tab_actor(&self) -> &ActorId {
        &self.tab_actor
    }

    /// Return the attached [`ResourceCommand`] bus, or create-and-attach one
    /// if none exists yet.
    ///
    /// This helper centralises the "lazily construct the bus once and reuse"
    /// pattern used by navigate and other commands.  The created bus is stored
    /// on the session so subsequent calls return the same instance.
    pub fn get_or_init_resource_command(
        &mut self,
        watcher_actor: ActorId,
    ) -> Arc<Mutex<ResourceCommand>> {
        if let Some(existing) = self.session.resource_command() {
            return Arc::clone(existing);
        }
        let rc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));
        self.session.set_resource_command(Arc::clone(&rc));
        rc
    }

    /// Re-resolve the target actors (consoleActor, etc.) from Firefox.
    ///
    /// After a navigation the docshell is torn down and replaced.  The old
    /// `consoleActor` ID becomes stale — any `evaluateJSAsync` sent to it
    /// returns `noSuchActor`.  Calling this refreshes `self.target` so the
    /// next `eval` uses the actor bound to the new docshell.
    ///
    /// When a refresh succeeds the registry is also updated: the old console
    /// front is invalidated (via `invalidate_target` on the old target ID) and
    /// the new target + console fronts are registered.
    ///
    /// Errors are intentionally swallowed: a failed refresh is non-fatal
    /// since the caller will get a `noSuchActor` error on the next eval
    /// (same failure mode as before) and the retry with a fresh target will
    /// succeed.
    pub fn refresh_target(&mut self) {
        let tab_actor = self.tab_actor.clone();
        match TabActor::get_target(self.session.transport_mut(), &tab_actor) {
            Ok(fresh) => {
                // Invalidate the stale target front and all its owned actors.
                let old_target_id = self.target.actor.clone();
                self.session.registry().invalidate_target(&old_target_id);
                // Register the fresh fronts.
                register_target_fronts(self.session.registry(), &fresh);
                self.target = fresh;
            }
            Err(e) => {
                eprintln!("warning: navigate: could not refresh target actors: {e:#}");
            }
        }
    }
}
