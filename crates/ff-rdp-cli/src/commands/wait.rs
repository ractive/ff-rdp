use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
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

pub fn run(cli: &Cli, opts: &WaitOptions<'_>) -> Result<(), AppError> {
    // Validate that at least one condition is specified (safety net — clap
    // enforces this via the "condition" argument group, but this catches
    // programmatic misuse of WaitOptions).
    if opts.selector.is_none() && opts.text.is_none() && opts.eval.is_none() {
        return Err(AppError::User(
            "wait: specify at least one of --selector, --text, or --eval".into(),
        ));
    }

    let js = build_wait_js(opts)?;

    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let condition = describe_condition(opts);
    let timeout_msg = format!(
        "wait timed out after {}ms — condition not met: {condition}; increase with --wait-timeout",
        opts.wait_timeout
    );

    let elapsed_ms = poll_js_condition(
        &mut ctx,
        &console_actor,
        &js,
        opts.wait_timeout,
        "wait condition threw an exception",
        &timeout_msg,
    )?;

    let result_json = json!({"matched": true, "elapsed_ms": elapsed_ms, "condition": condition});
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result_json, 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
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
}
