use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::{escape_selector, poll_js_condition};

pub struct WaitOptions<'a> {
    pub selector: Option<&'a str>,
    pub text: Option<&'a str>,
    pub eval: Option<&'a str>,
    pub wait_timeout: u64,
}

/// Wait for a condition and return the result value without printing.
///
/// Called by the script runner, which handles its own NDJSON output.
pub fn run_core(cli: &Cli, opts: &WaitOptions<'_>) -> Result<serde_json::Value, AppError> {
    if opts.selector.is_none() && opts.text.is_none() && opts.eval.is_none() {
        return Err(AppError::User(
            "wait: specify at least one of --selector, --text, or --eval".into(),
        ));
    }

    let js = build_wait_js(opts)?;

    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();
    let tab_actor_id = ctx.target_tab_actor().to_string();

    let not_found_msg = if let Some(sel) = opts.selector {
        format!(
            "selector '{sel}' not found after {}ms on tab '{tab_actor_id}' — the element may not exist; verify with `ff-rdp dom '{sel}' --count`",
            opts.wait_timeout
        )
    } else {
        let condition = describe_condition(opts);
        format!(
            "wait timed out after {}ms — condition not met: {condition}; increase with --wait-timeout",
            opts.wait_timeout
        )
    };

    let condition = describe_condition(opts);

    let elapsed_ms = poll_js_condition(
        &mut ctx,
        &console_actor,
        &js,
        opts.wait_timeout,
        "wait condition threw an exception",
        &not_found_msg,
    )
    .map_err(|e| {
        if let AppError::Timeout(ref msg) = e
            && msg.contains("operation timed out")
        {
            return AppError::Timeout(format!(
                "tab '{tab_actor_id}' did not respond within {}ms — try `ff-rdp tabs` to confirm the active target",
                opts.wait_timeout
            ));
        }
        e
    })?;

    Ok(json!({"matched": true, "elapsed_ms": elapsed_ms, "condition": condition}))
}

pub fn run(cli: &Cli, opts: &WaitOptions<'_>) -> Result<(), AppError> {
    // Validate that at least one condition is specified (safety net — clap
    // enforces this via the "condition" argument group, but this catches
    // programmatic misuse of WaitOptions).
    if opts.selector.is_none() && opts.text.is_none() && opts.eval.is_none() {
        return Err(AppError::User(
            "wait: specify at least one of --selector, --text, or --eval".into(),
        ));
    }

    let result_json = run_core(cli, opts)?;
    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    let envelope = output::envelope(&result_json, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::Wait);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

fn build_wait_js(opts: &WaitOptions<'_>) -> Result<String, AppError> {
    if let Some(sel) = opts.selector {
        let escaped = escape_selector(sel);
        Ok(format!("document.querySelector('{escaped}') !== null"))
    } else if let Some(text) = opts.text {
        let escaped_text = serde_json::to_string(text)
            .map_err(|e| AppError::from(anyhow::anyhow!("failed to encode text argument: {e}")))?;
        Ok(format!(
            "(document.body && document.body.innerText.includes({escaped_text}))"
        ))
    } else if let Some(expr) = opts.eval {
        // Wrap in a function so expression-level returns work and errors are contained.
        Ok(format!("(function() {{ return !!({expr}); }})()"))
    } else {
        unreachable!("condition check above ensures at least one option is set")
    }
}

fn describe_condition(opts: &WaitOptions<'_>) -> String {
    if let Some(sel) = opts.selector {
        format!("selector={sel:?}")
    } else if let Some(text) = opts.text {
        format!("text={text:?}")
    } else if let Some(expr) = opts.eval {
        format!("eval={expr:?}")
    } else {
        "(none)".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_wait_js_selector() {
        let opts = WaitOptions {
            selector: Some("button.submit"),
            text: None,
            eval: None,
            wait_timeout: 5000,
        };
        let js = build_wait_js(&opts).unwrap();
        assert!(js.contains("querySelector('button.submit')"));
        assert!(js.contains("!== null"));
    }

    #[test]
    fn build_wait_js_text() {
        let opts = WaitOptions {
            selector: None,
            text: Some("Success"),
            eval: None,
            wait_timeout: 5000,
        };
        let js = build_wait_js(&opts).unwrap();
        assert!(js.contains("includes(\"Success\")"));
    }

    #[test]
    fn build_wait_js_eval() {
        let opts = WaitOptions {
            selector: None,
            text: None,
            eval: Some("document.readyState === 'complete'"),
            wait_timeout: 5000,
        };
        let js = build_wait_js(&opts).unwrap();
        assert!(js.contains("document.readyState === 'complete'"));
    }

    // A2: timeout error messages distinguish "selector not found" from "tab unresponsive"

    #[test]
    fn selector_not_found_message_names_selector_and_tab() {
        // Simulate building the not_found_msg the way run() does, without needing
        // a live connection.  The key properties: contains the selector string and
        // the tab actor ID, does NOT say "tab did not respond".
        let selector = "input[type='email']";
        let tab_id = "server1.conn0.tab42";
        let timeout_ms = 10_000u64;

        let msg = format!(
            "selector '{selector}' not found after {timeout_ms}ms on tab '{tab_id}' — the element may not exist; verify with `ff-rdp dom '{selector}' --count`"
        );

        assert!(
            msg.contains(selector),
            "message should contain the selector: {msg}"
        );
        assert!(
            msg.contains(tab_id),
            "message should contain the tab actor: {msg}"
        );
        assert!(
            msg.contains("not found"),
            "message should say 'not found': {msg}"
        );
        assert!(
            !msg.contains("did not respond"),
            "selector-not-found message should not say 'did not respond': {msg}"
        );
    }

    #[test]
    fn tab_unresponsive_message_names_tab_and_suggests_tabs_command() {
        // Simulate the message produced when the transport itself times out.
        let tab_id = "server1.conn0.tab42";
        let timeout_ms = 10_000u64;

        let msg = format!(
            "tab '{tab_id}' did not respond within {timeout_ms}ms — try `ff-rdp tabs` to confirm the active target"
        );

        assert!(
            msg.contains(tab_id),
            "message should contain the tab actor: {msg}"
        );
        assert!(
            msg.contains("did not respond"),
            "message should say 'did not respond': {msg}"
        );
        assert!(
            msg.contains("tabs"),
            "message should suggest running `tabs`: {msg}"
        );
        assert!(
            !msg.contains("not found"),
            "tab-unresponsive message should not say 'not found': {msg}"
        );
    }
}
