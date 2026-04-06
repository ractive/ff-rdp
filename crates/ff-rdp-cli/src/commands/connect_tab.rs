use std::time::Duration;

use ff_rdp_core::{
    ActorId, ProtocolError, RdpConnection, RdpTransport, RootActor, TabActor, TargetInfo,
};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::tab_target::resolve_tab;

/// Shared state after connecting to Firefox and resolving a tab target.
pub struct ConnectedTab {
    connection: RdpConnection,
    pub(crate) target: TargetInfo,
    tab_actor: ActorId,
}

/// Connect to Firefox, resolve the target tab, and call `getTarget` on it.
///
/// This is the common setup for commands that operate on a single tab
/// (navigate, eval, reload, back, forward).
pub fn connect_and_get_target(cli: &Cli) -> Result<ConnectedTab, AppError> {
    let mut connection = RdpConnection::connect(
        &cli.host,
        cli.port,
        Duration::from_millis(cli.timeout),
    )
    .map_err(|e| match e {
        ProtocolError::ConnectionFailed(_) | ProtocolError::Timeout => AppError::User(format!(
            "could not connect to Firefox at {}:{} — is Firefox running with --start-debugger-server {}?",
            cli.host, cli.port, cli.port
        )),
        other => AppError::from(other),
    })?;

    let tabs = RootActor::list_tabs(connection.transport_mut()).map_err(AppError::from)?;

    let tab = resolve_tab(&tabs, cli.tab.as_deref(), cli.tab_id.as_deref())?;
    let tab_actor = tab.actor.clone();

    let target =
        TabActor::get_target(connection.transport_mut(), &tab_actor).map_err(AppError::from)?;

    Ok(ConnectedTab {
        connection,
        target,
        tab_actor,
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
