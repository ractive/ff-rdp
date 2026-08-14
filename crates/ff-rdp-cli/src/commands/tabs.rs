use std::time::Duration;

use ff_rdp_core::{ProtocolError, RdpConnection, RootActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

/// Known localized titles of Consent-O-Matic's options page
/// (`ui.js`'s `OPTIONS_TITLE` message table, vendored in
/// `crates/ff-rdp-cli/assets/extensions/consent-o-matic-1.1.5.xpi`).
/// `launch`'s pinned `intl.locale.requested = "en-US"` (`USER_JS` in
/// `commands/launch.rs`) should keep this at the English string, but all
/// five shipped locales are matched defensively — see the iteration-144
/// Theme F note about that pin's reliability being itself under
/// investigation.
const CONSENT_O_MATIC_OPTIONS_TITLES: &[&str] = &[
    "Consent-O-Matic Options",       // en
    "Consent-O-Matic indstillinger", // da
    "Consent-O-Matic Einstellungen", // de
    "Opções do Consent-O-Matic",     // pt
    "Options de Consent-O-Matic",    // fr
];

/// True for the permanent options tab `launch --auto-consent` leaves open
/// after installing Consent-O-Matic (iter-144 Theme C). Matches on both the
/// `moz-extension://` scheme (the extension's per-profile UUID is random,
/// so the URL can't be matched exactly) and a known localized title, so an
/// unrelated extension's options tab in a caller-supplied `--profile`
/// isn't accidentally hidden.
fn is_consent_o_matic_options_tab(tab: &Value) -> bool {
    let url = tab.get("url").and_then(Value::as_str).unwrap_or_default();
    let title = tab.get("title").and_then(Value::as_str).unwrap_or_default();
    url.starts_with("moz-extension://") && CONSENT_O_MATIC_OPTIONS_TITLES.contains(&title)
}

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
    // iter-144 Theme C: `launch --auto-consent` installs Consent-O-Matic,
    // which opens its own options tab on first run and never closes it —
    // that synthetic tab isn't something a caller targeting `--tab N`
    // should ever see or count against tab indices, so it's filtered
    // before sort/limit/total are computed (kb/iterations/
    // iteration-142-session-hygiene.md Theme C).
    items.retain(|item| !is_consent_o_matic_options_tab(item));
    controls.apply_sort(&mut items)?;
    controls.validate_fields(&items)?;
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
    // iter-134: `tabs` connects via a raw `RdpConnection::connect` above,
    // bypassing `ConnectedTab`/the daemon entirely — there is no
    // daemon-vs-direct routing decision to report, so the route is
    // unconditionally "direct".
    crate::connection_meta::merge_route(&mut meta, false);
    // Use envelope_with_truncation so --limit emits the same `truncated`
    // signal as other OutputControls-backed list commands (e.g. network, dom).
    let envelope =
        output::envelope_with_truncation(&json!(limited), shown, total, truncated, &meta);

    let hint_ctx = HintContext::new(HintSource::Tabs);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
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

    // ── Consent-O-Matic options tab filtering (iter-144 Theme C) ────────

    /// AC: `live_144_no_consent_o_matic_tab_leak` (matching half) — the
    /// exact shape observed live: `launch --auto-consent` on port 6103
    /// left `{"title":"Consent-O-Matic Options","url":"moz-extension://
    /// 959682e5-.../options.html"}` in the `tabs` listing.
    #[test]
    fn is_consent_o_matic_options_tab_matches_live_shape() {
        let tab = json!({
            "actor": "server1.conn1.tabDescriptor2",
            "title": "Consent-O-Matic Options",
            "url": "moz-extension://959682e5-00b2-4372-a120-ed784d7b7b73/options.html",
            "selected": false,
        });
        assert!(is_consent_o_matic_options_tab(&tab));
    }

    #[test]
    fn is_consent_o_matic_options_tab_matches_every_shipped_locale() {
        for title in CONSENT_O_MATIC_OPTIONS_TITLES {
            let tab = json!({
                "title": title,
                "url": "moz-extension://any-uuid/options.html",
            });
            assert!(
                is_consent_o_matic_options_tab(&tab),
                "locale title {title:?} should match"
            );
        }
    }

    /// AC: `live_144_no_consent_o_matic_tab_leak` (no-match half) — a real
    /// page tab, and a same-titled tab on a non-extension scheme (so the
    /// filter can't be spoofed by page content), must not be filtered.
    #[test]
    fn is_consent_o_matic_options_tab_no_match_for_normal_tab() {
        let normal = json!({"title": "Example", "url": "https://example.com"});
        assert!(!is_consent_o_matic_options_tab(&normal));

        let same_title_wrong_scheme =
            json!({"title": "Consent-O-Matic Options", "url": "https://example.com"});
        assert!(!is_consent_o_matic_options_tab(&same_title_wrong_scheme));
    }

    /// A different extension's options tab (arbitrary title, moz-extension
    /// scheme) must not be filtered — only Consent-O-Matic's known titles
    /// match, so a caller-supplied `--profile` with other extensions keeps
    /// seeing their tabs.
    #[test]
    fn is_consent_o_matic_options_tab_no_match_for_other_extension() {
        let other = json!({
            "title": "uBlock Origin",
            "url": "moz-extension://other-uuid/options.html",
        });
        assert!(!is_consent_o_matic_options_tab(&other));
    }

    #[test]
    fn consent_o_matic_tab_filtered_out_of_tabs_list() {
        let mut items = vec![
            json!({"title": "Example", "url": "https://example.com"}),
            json!({
                "title": "Consent-O-Matic Options",
                "url": "moz-extension://uuid/options.html",
            }),
        ];
        items.retain(|item| !is_consent_o_matic_options_tab(item));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"], "Example");
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
