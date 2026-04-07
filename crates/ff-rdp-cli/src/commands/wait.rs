use std::time::{Duration, Instant};

use ff_rdp_core::{Grip, WebConsoleActor};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::escape_selector;

const POLL_INTERVAL_MS: u64 = 100;

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

    let timeout = Duration::from_millis(opts.wait_timeout);
    let poll = Duration::from_millis(POLL_INTERVAL_MS);
    let started = Instant::now();

    loop {
        let eval_result =
            WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, &js)
                .map_err(AppError::from)?;

        if let Some(ref exc) = eval_result.exception {
            let msg = exc
                .message
                .as_deref()
                .unwrap_or("wait condition threw an exception");
            eprintln!("error: {msg}");
            return Err(AppError::Exit(1));
        }

        if is_truthy(&eval_result.result) {
            // Condition met — return confirmation.
            // Saturate at u64::MAX (about 585 million years) rather than panic.
            let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let condition = describe_condition(opts);
            let result_json =
                json!({"matched": true, "elapsed_ms": elapsed_ms, "condition": condition});
            let meta = json!({"host": cli.host, "port": cli.port});
            let envelope = output::envelope(&result_json, 1, &meta);

            return OutputPipeline::from_cli(cli)?
                .finalize(&envelope)
                .map_err(AppError::from);
        }

        if started.elapsed() >= timeout {
            let condition = describe_condition(opts);
            eprintln!(
                "error: wait timed out after {}ms — condition not met: {condition}",
                opts.wait_timeout
            );
            return Err(AppError::Exit(1));
        }

        std::thread::sleep(poll);
    }
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
    use serde_json::json;

    #[test]
    fn is_truthy_true() {
        assert!(is_truthy(&Grip::Value(json!(true))));
        assert!(is_truthy(&Grip::Value(json!(1))));
        assert!(is_truthy(&Grip::Value(json!("hello"))));
        assert!(is_truthy(&Grip::Inf));
        assert!(is_truthy(&Grip::NegInf));
    }

    #[test]
    fn is_truthy_false() {
        assert!(!is_truthy(&Grip::Null));
        assert!(!is_truthy(&Grip::Undefined));
        assert!(!is_truthy(&Grip::Value(json!(false))));
        assert!(!is_truthy(&Grip::Value(json!(0))));
        assert!(!is_truthy(&Grip::Value(json!(""))));
        assert!(!is_truthy(&Grip::NaN));
        assert!(!is_truthy(&Grip::NegZero));
    }

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
