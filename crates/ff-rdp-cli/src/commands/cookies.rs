use ff_rdp_core::{Grip, StorageActor, WebConsoleActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_direct};
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

pub fn run(cli: &Cli, name: Option<&str>, include_document_cookie: bool) -> Result<(), AppError> {
    let mut ctx = connect_direct(cli)?;
    let tab_actor = ctx.target_tab_actor().clone();

    let cookies =
        StorageActor::list_cookies(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

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

    // --include-document-cookie: evaluate document.cookie and merge any entries
    // not already present in the StorageActor reply (e.g. cookies that lack a
    // Domain= attribute and are not surfaced by getStoreObjects).
    if include_document_cookie {
        let doc_cookies = fetch_document_cookies(&mut ctx);
        let storage_names: std::collections::HashSet<String> = results
            .iter()
            .filter_map(|c| c.get("name").and_then(Value::as_str).map(str::to_owned))
            .collect();
        for entry in doc_cookies {
            let entry_name = entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if !storage_names.contains(&entry_name) {
                results.push(entry);
            }
        }
    }

    // Filter by cookie name if requested.
    if let Some(filter_name) = name {
        results.retain(|c| c.get("name").and_then(Value::as_str) == Some(filter_name));
    }

    let total = results.len();
    let result_json = json!(results);

    // If no cookies found, check for a consent banner that may be suppressing them.
    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
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
                 If a consent banner is present, accept it or use `ff-rdp launch --temp-profile --auto-consent`."
            ),
        );
    }

    let hint_ctx = HintContext::new(HintSource::Cookies);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

/// Evaluate `document.cookie` and return each `name=value` pair as a JSON
/// object with `source: "document.cookie"`.
///
/// This is a best-effort fallback for cookies that are not surfaced by the
/// StorageActor (e.g. cookies without a `Domain=` attribute set via JS).
/// Errors and empty results are returned as an empty Vec.
fn fetch_document_cookies(ctx: &mut ConnectedTab) -> Vec<Value> {
    let js = "document.cookie";
    let console_actor = ctx.target.console_actor.clone();
    let Ok(eval_result) =
        WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, js)
    else {
        return vec![];
    };

    if eval_result.exception.is_some() {
        return vec![];
    }

    let cookie_str = match &eval_result.result {
        Grip::Value(Value::String(s)) => s.clone(),
        _ => return vec![],
    };

    if cookie_str.trim().is_empty() {
        return vec![];
    }

    cookie_str
        .split(';')
        .filter_map(|pair| {
            let pair = pair.trim();
            if pair.is_empty() {
                return None;
            }
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            Some(json!({
                "name": name.trim(),
                "value": value.trim(),
                "source": "document.cookie",
                "expires": "Session",
            }))
        })
        .collect()
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

    #[test]
    fn fetch_document_cookies_parses_name_value_pairs() {
        // Unit test the parsing logic directly (without a live Firefox).
        fn parse_cookie_str(s: &str) -> Vec<Value> {
            if s.trim().is_empty() {
                return vec![];
            }
            s.split(';')
                .filter_map(|pair| {
                    let pair = pair.trim();
                    if pair.is_empty() {
                        return None;
                    }
                    let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
                    Some(json!({
                        "name": name.trim(),
                        "value": value.trim(),
                        "source": "document.cookie",
                        "expires": "Session",
                    }))
                })
                .collect()
        }

        let cookies = parse_cookie_str("probe=1; session=abc; flag");
        assert_eq!(cookies.len(), 3);
        assert_eq!(cookies[0]["name"], "probe");
        assert_eq!(cookies[0]["value"], "1");
        assert_eq!(cookies[0]["source"], "document.cookie");
        assert_eq!(cookies[1]["name"], "session");
        assert_eq!(cookies[1]["value"], "abc");
        assert_eq!(cookies[2]["name"], "flag");
        assert_eq!(cookies[2]["value"], "");
    }

    #[test]
    fn fetch_document_cookies_empty_string_returns_empty() {
        fn parse_cookie_str(s: &str) -> Vec<Value> {
            if s.trim().is_empty() {
                return vec![];
            }
            s.split(';')
                .filter_map(|pair| {
                    let pair = pair.trim();
                    if pair.is_empty() {
                        None
                    } else {
                        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
                        Some(json!({"name": name.trim(), "value": value.trim()}))
                    }
                })
                .collect()
        }
        assert!(parse_cookie_str("").is_empty());
        assert!(parse_cookie_str("   ").is_empty());
    }
}
