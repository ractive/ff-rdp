use std::time::Duration;

use ff_rdp_core::{
    ActorId, ProtocolError, RdpConnection, RdpTransport, RootActor, TabActor, TargetInfo,
};

use crate::cli::args::Cli;
use crate::daemon::client::{ConnectionTarget, resolve_connection_target};
use crate::error::AppError;
use crate::tab_target::resolve_tab;

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

    let (connect_host, connect_port, via_daemon) = match target {
        ConnectionTarget::Daemon { port } => ("127.0.0.1".to_owned(), port, true),
        ConnectionTarget::Direct => (cli.host.clone(), cli.port, false),
    };

    let connection = connect_to_firefox(&connect_host, connect_port, cli, via_daemon)?;

    handshake_and_resolve_tab(connection, cli, via_daemon)
}

/// Like [`connect_and_get_target`] but always bypasses the daemon and
/// connects directly to Firefox.  Use this for commands (e.g. screenshot)
/// whose protocol interactions are incompatible with the daemon proxy.
pub fn connect_direct(cli: &Cli) -> Result<ConnectedTab, AppError> {
    let connection = connect_to_firefox(&cli.host, cli.port, cli, false)?;

    handshake_and_resolve_tab(connection, cli, false)
}

/// Establish a TCP connection to Firefox (or daemon proxy) and produce
/// user-friendly errors on failure.
fn connect_to_firefox(
    host: &str,
    port: u16,
    cli: &Cli,
    via_daemon: bool,
) -> Result<RdpConnection, AppError> {
    RdpConnection::connect(host, port, Duration::from_millis(cli.timeout)).map_err(|e| match e {
        ProtocolError::ConnectionFailed(_) | ProtocolError::Timeout if !via_daemon => {
            AppError::User(format!(
                "could not connect to Firefox at {}:{} — is Firefox running with --start-debugger-server {}?",
                cli.host, cli.port, cli.port
            ))
        }
        ProtocolError::ConnectionFailed(_) | ProtocolError::Timeout if via_daemon => {
            AppError::User(format!(
                "could not connect to daemon on port {port} — try --no-daemon to connect directly to Firefox"
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
    connection.warn_if_version_unsupported();

    let tabs = RootActor::list_tabs(connection.transport_mut()).map_err(AppError::from)?;

    let tab = resolve_tab(&tabs, cli.tab.as_deref(), cli.tab_id.as_deref())?;
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
