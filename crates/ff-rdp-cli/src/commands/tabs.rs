use std::time::Duration;

use ff_rdp_core::{ProtocolError, RdpConnection, RootActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
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

    crate::connection_meta::remember_version(connection.firefox_version());

    let tabs = RootActor::list_tabs(connection.transport_mut()).map_err(AppError::from)?;

    let results_json: Value = serde_json::to_value(&tabs)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("failed to serialize tabs: {e}")))?;

    // Apply output controls (sort/limit/fields) so --fields is honoured like
    // other commands (iter-61k J).
    let controls = OutputControls::from_cli(cli, SortDir::Asc);
    let mut items: Vec<Value> = match results_json {
        Value::Array(arr) => arr,
        other => vec![other],
    };
    controls.apply_sort(&mut items);
    let (limited, total, truncated) = controls.apply_limit(items, None);
    let shown = limited.len();
    let limited = controls.apply_fields(limited);

    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    // Use envelope_with_truncation so --limit emits the same `truncated`
    // signal as other OutputControls-backed list commands (e.g. network, dom).
    let envelope =
        output::envelope_with_truncation(&json!(limited), shown, total, truncated, &meta);

    let hint_ctx = HintContext::new(HintSource::Tabs);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_controls(fields: Option<Vec<String>>) -> OutputControls {
        OutputControls {
            limit: None,
            all: false,
            sort_field: None,
            sort_dir: SortDir::Asc,
            fields,
        }
    }

    #[test]
    fn fields_filter_applied_to_tab_entries() {
        let items = vec![
            json!({"url": "https://example.com", "title": "Example", "id": 1}),
            json!({"url": "https://rust-lang.org", "title": "Rust", "id": 2}),
        ];
        let fields = vec!["url".to_owned(), "title".to_owned()];
        let controls = make_controls(Some(fields));
        let filtered = controls.apply_fields(items);
        assert_eq!(filtered.len(), 2);
        for entry in &filtered {
            assert!(entry.get("url").is_some(), "url should be present");
            assert!(entry.get("title").is_some(), "title should be present");
            assert!(entry.get("id").is_none(), "id should be filtered out");
        }
    }

    #[test]
    fn fields_noop_when_none() {
        let items = vec![json!({"url": "https://example.com", "title": "Example", "id": 1})];
        // fields=None means no filtering — all keys preserved.
        let controls = make_controls(None);
        let filtered = controls.apply_fields(items);
        assert_eq!(filtered[0].get("id"), Some(&json!(1)));
        assert_eq!(filtered[0].get("url").unwrap(), "https://example.com");
    }
}
