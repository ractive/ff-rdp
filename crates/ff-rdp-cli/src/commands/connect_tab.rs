use std::time::Duration;

use ff_rdp_core::{
    ActorId, DeviceActor, ProtocolError, RdpConnection, RdpTransport, RootActor, TabActor,
    TargetInfo,
};
use serde_json::json;

use crate::cli::args::Cli;
use crate::daemon::client::{ConnectionTarget, resolve_connection_target};
use crate::error::AppError;

/// Shared state after connecting to Firefox and resolving a tab target.
pub struct ConnectedTab {
    connection: RdpConnection,
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

/// Run the RDP handshake: list tabs, resolve the target tab, call `getTarget`.
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

    Ok(ConnectedTab {
        connection,
        target: target_info,
        tab_actor,
        via_daemon,
    })
}

impl ConnectedTab {
    pub fn transport_mut(&mut self) -> &mut RdpTransport {
        self.connection.transport_mut()
    }

    pub fn target_tab_actor(&self) -> &ActorId {
        &self.tab_actor
    }
}
