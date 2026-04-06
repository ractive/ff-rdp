use std::time::{Duration, Instant};

use ff_rdp_core::{Grip, TabActor, WatcherActor, WebConsoleActor, WindowGlobalTarget};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::escape_selector;
use super::network_events::{
    build_network_entries, drain_network_events, drain_network_from_daemon, merge_updates,
};
use super::url_validation::validate_url;

const POLL_INTERVAL_MS: u64 = 100;

/// Options controlling an optional wait condition after navigation.
///
/// # False positive risk
///
/// If the *previous* page already satisfies the wait condition (same selector
/// present, or same text visible) before the new page begins loading, the poll
/// loop may observe a truthy result on the old DOM and return immediately —
/// before the navigation has actually completed.  Callers should be aware of
/// this when reusing the same selector or text across navigations.
// Field names intentionally carry the `wait_` prefix to match the CLI flags
// they correspond to (--wait-text, --wait-selector, --wait-timeout).
#[allow(clippy::struct_field_names)]
pub struct WaitAfterNav<'a> {
    /// Wait until this text appears anywhere on the page body.
    pub wait_text: Option<&'a str>,
    /// Wait until an element matching this CSS selector exists in the DOM.
    pub wait_selector: Option<&'a str>,
    /// Timeout in milliseconds for the wait condition (default: 5000).
    pub wait_timeout: u64,
}

impl WaitAfterNav<'_> {
    fn has_condition(&self) -> bool {
        self.wait_text.is_some() || self.wait_selector.is_some()
    }
}

pub fn run(cli: &Cli, url: &str, wait_opts: &WaitAfterNav<'_>) -> Result<(), AppError> {
    if !cli.allow_unsafe_urls {
        validate_url(url)?;
    }
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();

    WindowGlobalTarget::navigate_to(ctx.transport_mut(), &target_actor, url)
        .map_err(AppError::from)?;

    let wait_result = wait_after_navigate(&mut ctx, wait_opts)?;

    let mut result = json!({"navigated": url});
    if let Some(w) = wait_result
        && let Some(obj) = result.as_object_mut()
    {
        obj.insert("wait".to_string(), w);
    }
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result, 1, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Navigate to `url` and capture all network requests made during navigation.
///
/// The flow on a single TCP connection is:
/// 1. Connect and resolve the target tab.
/// 2. Get the WatcherActor via `TabActor::get_watcher`.
/// 3. Subscribe to `"network-event"` resources via `WatcherActor::watch_resources`.
/// 4. Navigate with `WindowGlobalTarget::navigate_to`.
/// 5. Drain `resources-available-array` / `resources-updated-array` events
///    (timeout-bounded, same pattern as the `network` command).
/// 6. Merge updates into resources by `resource_id`.
/// 7. Unwatch resources to clean up server-side state.
/// 8. Optionally wait for a condition (--wait-text / --wait-selector).
/// 9. Emit combined JSON output.
pub fn run_with_network(
    cli: &Cli,
    url: &str,
    wait_opts: &WaitAfterNav<'_>,
) -> Result<(), AppError> {
    if !cli.allow_unsafe_urls {
        validate_url(url)?;
    }
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();

    if ctx.via_daemon {
        // The daemon has already subscribed to network-event resources.
        // Navigate first, then drain the daemon buffer for events from this
        // navigation.  The daemon continues buffering after the drain so
        // subsequent commands see events from future navigations too.
        WindowGlobalTarget::navigate_to(ctx.transport_mut(), &target_actor, url)
            .map_err(AppError::from)?;

        let (all_resources, all_updates) = drain_network_from_daemon(ctx.transport_mut())?;

        // NOTE: In the daemon path, wait_after_navigate is called *before*
        // building the network entries.  This differs from the non-daemon path
        // where the wait happens *after* unwatching resources.  The ordering
        // is intentional: the daemon keeps the subscription open across
        // commands, so there is nothing to unwatch here, and we want to let
        // the page settle before returning to the caller regardless.
        let wait_result = wait_after_navigate(&mut ctx, wait_opts)?;

        let update_map = merge_updates(all_updates);
        let network_entries = build_network_entries(&all_resources, &update_map);

        let total = network_entries.len();
        let mut result = json!({
            "navigated": url,
            "network": network_entries,
        });
        if let Some(w) = wait_result
            && let Some(obj) = result.as_object_mut()
        {
            obj.insert("wait".to_string(), w);
        }
        let meta = json!({"host": cli.host, "port": cli.port});
        let envelope = output::envelope(&result, total, &meta);
        return OutputPipeline::new(cli.jq.clone())
            .finalize(&envelope)
            .map_err(AppError::from);
    }

    let tab_actor = ctx.target_tab_actor().clone();

    // Get watcher actor for resource subscriptions.
    let watcher_actor =
        TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;

    // Subscribe to network events before navigating so we capture everything.
    WatcherActor::watch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"])
        .map_err(AppError::from)?;

    // Navigate to the target URL.
    WindowGlobalTarget::navigate_to(ctx.transport_mut(), &target_actor, url)
        .map_err(AppError::from)?;

    // Drain resource events until the timeout fires (no more events).
    let (all_resources, all_updates) =
        drain_network_events(ctx.transport_mut()).map_err(AppError::from)?;

    // Merge updates into resources by resource_id.
    let update_map = merge_updates(all_updates);

    // Build the network entries array (no URL/method filtering here).
    let network_entries = build_network_entries(&all_resources, &update_map);

    // Unwatch to clean up server-side resources.
    let _ =
        WatcherActor::unwatch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"]);

    // NOTE: In the non-daemon path, wait_after_navigate is called *after*
    // draining network events and unwatching resources, so network data is
    // already fully collected before we begin waiting.  The daemon path
    // (above) starts the wait before building entries because there is no
    // subscription lifecycle to tear down.
    let wait_result = wait_after_navigate(&mut ctx, wait_opts)?;

    let total = network_entries.len();
    let mut result = json!({
        "navigated": url,
        "network": network_entries,
    });
    if let Some(w) = wait_result
        && let Some(obj) = result.as_object_mut()
    {
        obj.insert("wait".to_string(), w);
    }
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result, total, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}

/// Poll a JS condition after navigation until it becomes truthy or times out.
///
/// Returns `Ok(Some(json))` when the condition is met, `Ok(None)` when no
/// condition was requested, and `Err` when the timeout expires or evaluation
/// fails with an exception.
fn wait_after_navigate(
    ctx: &mut super::connect_tab::ConnectedTab,
    opts: &WaitAfterNav<'_>,
) -> Result<Option<serde_json::Value>, AppError> {
    if !opts.has_condition() {
        return Ok(None);
    }

    let js = if let Some(sel) = opts.wait_selector {
        let escaped = escape_selector(sel);
        format!("document.querySelector('{escaped}') !== null")
    } else if let Some(text) = opts.wait_text {
        let escaped = serde_json::to_string(text)
            .map_err(|e| AppError::from(anyhow::anyhow!("failed to encode wait-text: {e}")))?;
        format!("(document.body && document.body.innerText.includes({escaped}))")
    } else {
        // has_condition() guarantees at least one is set; this branch is unreachable.
        return Ok(None);
    };

    let console_actor = ctx.target.console_actor.clone();
    let timeout = Duration::from_millis(opts.wait_timeout);
    let poll = Duration::from_millis(POLL_INTERVAL_MS);
    let started = Instant::now();

    loop {
        let eval_result =
            WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, &js)
                .map_err(AppError::from)?;

        // A JS exception (e.g. SyntaxError from an invalid CSS selector) will
        // never resolve to truthy — return an error immediately rather than
        // burning the entire timeout.
        if let Some(exc) = &eval_result.exception {
            let msg = exc
                .message
                .as_deref()
                .unwrap_or("JS exception during wait condition");
            eprintln!("error: navigate wait aborted due to JS exception: {msg}");
            return Err(AppError::Exit(1));
        }

        if is_truthy(&eval_result.result) {
            // Saturate at u64::MAX rather than panic.
            let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let condition = describe_wait_condition(opts);
            return Ok(Some(json!({
                "waited": true,
                "elapsed_ms": elapsed_ms,
                "condition": condition,
            })));
        }

        if started.elapsed() >= timeout {
            let condition = describe_wait_condition(opts);
            eprintln!(
                "error: navigate wait timed out after {}ms — condition not met: {condition}",
                opts.wait_timeout
            );
            return Err(AppError::Exit(1));
        }

        std::thread::sleep(poll);
    }
}

fn describe_wait_condition(opts: &WaitAfterNav<'_>) -> String {
    if let Some(sel) = opts.wait_selector {
        format!("selector={sel:?}")
    } else if let Some(text) = opts.wait_text {
        format!("text={text:?}")
    } else {
        "(none)".into()
    }
}

fn is_truthy(grip: &Grip) -> bool {
    match grip {
        // Null, Undefined, NaN, and -0 are all falsy in JavaScript.
        Grip::Null | Grip::Undefined | Grip::NaN | Grip::NegZero => false,
        Grip::Value(v) => {
            if let Some(b) = v.as_bool() {
                return b;
            }
            if let Some(n) = v.as_f64() {
                return n != 0.0;
            }
            if let Some(s) = v.as_str() {
                return !s.is_empty();
            }
            // Objects and arrays are truthy.
            !v.is_null()
        }
        // Infinity, -Infinity, LongString, Object are all truthy.
        Grip::Inf | Grip::NegInf | Grip::LongString { .. } | Grip::Object { .. } => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_after_nav_no_condition_returns_none() {
        let opts = WaitAfterNav {
            wait_text: None,
            wait_selector: None,
            wait_timeout: 5000,
        };
        assert!(!opts.has_condition());
    }

    #[test]
    fn wait_after_nav_text_has_condition() {
        let opts = WaitAfterNav {
            wait_text: Some("Hello"),
            wait_selector: None,
            wait_timeout: 5000,
        };
        assert!(opts.has_condition());
    }

    #[test]
    fn wait_after_nav_selector_has_condition() {
        let opts = WaitAfterNav {
            wait_text: None,
            wait_selector: Some("button.submit"),
            wait_timeout: 5000,
        };
        assert!(opts.has_condition());
    }

    #[test]
    fn is_truthy_true_values() {
        assert!(is_truthy(&Grip::Value(json!(true))));
        assert!(is_truthy(&Grip::Value(json!(1))));
        assert!(is_truthy(&Grip::Value(json!("hello"))));
        assert!(is_truthy(&Grip::Inf));
        assert!(is_truthy(&Grip::NegInf));
    }

    #[test]
    fn is_truthy_false_values() {
        assert!(!is_truthy(&Grip::Null));
        assert!(!is_truthy(&Grip::Undefined));
        assert!(!is_truthy(&Grip::Value(json!(false))));
        assert!(!is_truthy(&Grip::Value(json!(0))));
        assert!(!is_truthy(&Grip::Value(json!(""))));
        assert!(!is_truthy(&Grip::NaN));
        assert!(!is_truthy(&Grip::NegZero));
    }

    #[test]
    fn describe_wait_condition_selector() {
        let opts = WaitAfterNav {
            wait_text: None,
            wait_selector: Some("div#main"),
            wait_timeout: 3000,
        };
        assert_eq!(describe_wait_condition(&opts), r#"selector="div#main""#);
    }

    #[test]
    fn describe_wait_condition_text() {
        let opts = WaitAfterNav {
            wait_text: Some("Loaded"),
            wait_selector: None,
            wait_timeout: 3000,
        };
        assert_eq!(describe_wait_condition(&opts), r#"text="Loaded""#);
    }
}
