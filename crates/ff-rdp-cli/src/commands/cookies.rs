use std::time::Duration;

use ff_rdp_core::{Grip, RdpConnection, RootActor, StorageActor, WebConsoleActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;
use crate::tab_target::resolve_tab;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use super::js_helpers::escape_selector;

/// Common CMP (Consent Management Platform) selectors used to detect consent
/// banners that may be blocking cookie creation.
const CMP_SELECTORS: &[&str] = &[
    "#CybotCookiebotDialog",
    "#onetrust-consent-sdk",
    ".cmp-container",
    "[data-testid=\"uc-default-wall\"]",
    "#didomi-host",
    ".qc-cmp-ui-container",
];

pub fn run(cli: &Cli, name: Option<&str>) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let tab_actor = ctx.target_tab_actor().clone();

    let cookies = if ctx.via_daemon {
        // In daemon mode, the daemon proxy multiplexes all CLI requests through
        // a single Firefox connection.  Because `watchResources("cookies")` sends
        // a `resources-available-array` response and Firefox assigns the same
        // watcher actor to every `getWatcher` call on a given connection, the
        // daemon's `firefox_reader_loop` intercepts this response and it never
        // reaches the CLI client.
        //
        // Work around this by opening a short-lived *direct* connection to Firefox
        // (bypassing the daemon proxy) only for the cookies lookup.  Firefox
        // accepts multiple simultaneous connections, each with its own actor
        // namespace, so this is safe and has no effect on the daemon's state.
        let direct_timeout = Duration::from_millis(cli.timeout);
        let mut direct_conn =
            RdpConnection::connect(&cli.host, cli.port, direct_timeout).map_err(AppError::from)?;
        let direct_transport = direct_conn.transport_mut();
        let tabs = RootActor::list_tabs(direct_transport).map_err(AppError::from)?;
        let tab = resolve_tab(&tabs, cli.tab.as_deref(), cli.tab_id.as_deref())?;
        let direct_tab_actor = tab.actor.clone();
        StorageActor::list_cookies(direct_transport, &direct_tab_actor).map_err(AppError::from)?
    } else {
        StorageActor::list_cookies(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?
    };

    let mut results: Vec<Value> = cookies
        .iter()
        .map(|c| {
            let mut obj = serde_json::to_value(c).unwrap_or_default();
            // Replace numeric expires=0 with human-readable "Session".
            if c.expires == 0 {
                obj["expires"] = json!("Session");
            }
            // Drop internal-only fields that aren't useful for CLI output.
            if let Some(o) = obj.as_object_mut() {
                o.remove("lastAccessed");
                o.remove("creationTime");
            }
            obj
        })
        .collect();

    // Filter by cookie name if requested.
    if let Some(filter_name) = name {
        results.retain(|c| c.get("name").and_then(Value::as_str) == Some(filter_name));
    }

    let total = results.len();
    let result_json = json!(results);

    // If no cookies found, check for a consent banner that may be suppressing them.
    let mut meta = json!({"host": cli.host, "port": cli.port});
    if total == 0
        && let Some(note) = detect_consent_banner(&mut ctx)
        && let Some(m) = meta.as_object_mut()
    {
        m.insert("note".to_string(), json!(note));
    }

    let mut envelope = output::envelope(&result_json, total, &meta);
    if total == 0
        && let Some(obj) = envelope.as_object_mut()
    {
        obj.insert(
            "hint".to_string(),
            json!(
                "No cookies found. The page may not set cookies, or try navigating first. \
                 If a consent banner is present, accept it or use `ff-rdp launch --auto-consent`."
            ),
        );
    }

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Check if common CMP/consent banner elements exist on the page via JS eval.
///
/// Returns a human-readable note string when a banner is detected, or `None`
/// if no banner is found or the evaluation fails for any reason.  This is
/// intentionally best-effort: errors are silently ignored so that the primary
/// cookie output is never blocked by a failed CMP probe.
fn detect_consent_banner(ctx: &mut ConnectedTab) -> Option<String> {
    let selectors_js = CMP_SELECTORS
        .iter()
        .map(|s| format!("'{}'", escape_selector(s)))
        .collect::<Vec<_>>()
        .join(",");

    let js = format!(
        r"(function() {{
  var sels = [{selectors_js}];
  for (var i = 0; i < sels.length; i++) {{
    if (document.querySelector(sels[i])) return sels[i];
  }}
  return null;
}})()"
    );

    let console_actor = ctx.target.console_actor.clone();
    let eval_result =
        WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, &js).ok()?;

    // Treat any JS exception as "no banner detected" — the page may not have a
    // DOM at all (e.g. about:blank) or the eval may have hit a security error.
    if eval_result.exception.is_some() {
        return None;
    }

    match &eval_result.result {
        Grip::Value(Value::String(selector)) => Some(format!(
            "0 cookies found — a consent banner was detected ({selector}); \
             cookies may appear after accepting consent"
        )),
        // null return from JS means no CMP element was found.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmp_selectors_are_valid_css() {
        // Basic sanity: selectors must not be empty and must not contain single
        // quotes, which would break the JS string literal embedding.
        for sel in CMP_SELECTORS {
            assert!(!sel.is_empty(), "CMP selector should not be empty");
            assert!(
                !sel.contains('\''),
                "CMP selector should not contain single quotes: {sel}"
            );
        }
    }
}
