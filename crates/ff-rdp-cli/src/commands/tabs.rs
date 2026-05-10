use std::time::Duration;

use ff_rdp_core::{ProtocolError, RdpConnection, RootActor};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

pub fn run(cli: &Cli) -> Result<(), AppError> {
    let mut connection = RdpConnection::connect(
        &cli.host,
        cli.port,
        Duration::from_millis(cli.timeout),
    )
    .map_err(|e| match e {
        ProtocolError::ConnectionFailed(_) | ProtocolError::Timeout => AppError::Connection(format!(
            "could not connect to Firefox at {}:{} — is Firefox running with --start-debugger-server {}?\n\
             hint: run `ff-rdp doctor` for a full diagnostic, or `ff-rdp launch` to start Firefox with debugging enabled.",
            cli.host, cli.port, cli.port
        )),
        other => AppError::from(other),
    })?;

    connection.warn_if_version_unsupported();
    crate::connection_meta::remember_version(connection.firefox_version());

    let tabs = RootActor::list_tabs(connection.transport_mut()).map_err(AppError::from)?;

    let results_json: serde_json::Value = serde_json::to_value(&tabs)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("failed to serialize tabs: {e}")))?;

    let mut meta = json!({"host": cli.host, "port": cli.port});
    crate::connection_meta::merge_into(&mut meta, &cli.host, cli.port, None);
    let envelope = output::envelope(&results_json, tabs.len(), &meta);

    let hint_ctx = HintContext::new(HintSource::Tabs);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}
