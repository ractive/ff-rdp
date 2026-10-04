use std::time::{Duration, Instant};

use ff_rdp_core::{ActorErrorKind, ActorId, ProtocolError, WebConsoleActor, sanitize_for_terminal};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use super::js_helpers::{escape_selector, is_truthy};

pub struct WaitOptions<'a> {
    pub selector: Option<&'a str>,
    pub text: Option<&'a str>,
    pub eval: Option<&'a str>,
    /// iter-142 Theme F: a plain sleep, in milliseconds — no condition, no
    /// Firefox connection. Mutually exclusive with `selector`/`text`/`eval`
    /// at the CLI layer (the `condition` ArgGroup); `run_core` also treats
    /// it as taking priority if a caller somehow sets more than one.
    pub sleep_ms: Option<u64>,
    pub wait_timeout: u64,
}

/// Emit a deprecation warning to stderr when the caller passed `--timeout`
/// (the global flag) to the `wait` command instead of `--timeout-ms`.
///
/// Clap does not expose which alias was used, so we inspect raw argv.  This
/// is intentionally simple: only the exact `--timeout` spelling (or
/// `--timeout=<value>`) triggers the warning; other global-timeout forms
/// (`-t`, future short flags) do not — they are not deprecated aliases.
fn warn_if_timeout_alias_used() {
    let used = std::env::args().any(|a| a == "--timeout" || a.starts_with("--timeout="));
    if used {
        // stderr-ok: (b) deprecation warning — see the doc comment above.
        eprintln!(
            "warning: --timeout is deprecated for `wait`, use --timeout-ms instead \
             (this alias will be removed in a future release)"
        );
    }
}

/// Wait for a condition and return the result value without printing.
///
/// Called by the script runner, which handles its own NDJSON output.
pub fn run_core(cli: &Cli, opts: &WaitOptions<'_>) -> Result<serde_json::Value, AppError> {
    // iter-142 Theme F: --sleep-ms is a plain delay — no condition to poll,
    // no Firefox connection needed at all. Takes priority over the other
    // fields so a caller that somehow sets both never falls through to the
    // (meaningless, since sleep_ms doesn't describe a JS condition)
    // condition-polling path below.
    if let Some(ms) = opts.sleep_ms {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        return Ok(
            json!({"matched": true, "elapsed_ms": ms, "condition": format!("sleep={ms}ms")}),
        );
    }

    if opts.selector.is_none() && opts.text.is_none() && opts.eval.is_none() {
        return Err(AppError::User(
            "wait: specify at least one of --selector, --text, --eval, --ref, or --sleep-ms".into(),
        ));
    }

    let js = build_wait_js(opts)?;

    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target().console_actor.clone();
    let tab_actor_id = ctx.target_tab_actor().to_string();

    let not_found_msg = if let Some(sel) = opts.selector {
        format!(
            "selector '{sel}' not found after {}ms on tab '{tab_actor_id}' — the element may not exist; verify with `ff-rdp dom '{sel}' --count`",
            opts.wait_timeout
        )
    } else {
        let condition = describe_condition(opts);
        format!(
            "wait timed out after {}ms — condition not met: {condition}; increase with --timeout-ms",
            opts.wait_timeout
        )
    };

    let condition = describe_condition(opts);

    let elapsed_ms = poll_across_navigation(
        &mut ctx,
        console_actor,
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
    warn_if_timeout_alias_used();
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
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
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

/// Poll interval for [`poll_across_navigation`] (matches `poll_js_condition`).
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Whether an evaluation error means the document the console actor belonged
/// to is gone — replaced by a navigation — rather than a real failure.
fn is_document_gone(err: &ProtocolError) -> bool {
    matches!(
        err,
        ProtocolError::EvalNavigatedDuringEval
            | ProtocolError::EvalTargetDestroyed { .. }
            | ProtocolError::ActorError {
                kind: ActorErrorKind::UnknownActor,
                ..
            }
    )
}

/// [`poll_js_condition`](super::js_helpers::poll_js_condition), but a
/// navigation that lands while the wait runs does not end it: the condition
/// moves on to the new document (dogfooding session 64 #12, where a `run`
/// playbook's `wait` after a navigating click polled the outgoing document
/// until it timed out).
///
/// Two signals move it: Firefox announcing a navigation on this connection,
/// and an evaluation failing because the console actor's document is gone.
/// Either way the target is re-resolved before the next poll. Each
/// evaluation is guarded on the current document's `innerWindowId`, so one
/// torn down mid-evaluation fails fast instead of holding the reply until
/// the deadline.
fn poll_across_navigation(
    ctx: &mut ConnectedTab,
    mut console_actor: ActorId,
    js: &str,
    timeout_ms: u64,
    error_context: &str,
    timeout_context: &str,
) -> Result<u64, AppError> {
    // A zero budget is the documented single evaluation with no deadline;
    // a navigation cannot be followed inside it.
    if timeout_ms == 0 {
        return super::js_helpers::poll_js_condition(
            ctx,
            &console_actor,
            js,
            0,
            error_context,
            timeout_context,
        );
    }
    let started = Instant::now();
    let deadline = started + Duration::from_millis(timeout_ms);
    loop {
        let outcome = {
            let inner_window_id = ctx.target().inner_window_id;
            let mut guarded = ctx.arm_target_guard(inner_window_id);
            guarded.transport_mut().with_read_deadline(deadline, |t| {
                WebConsoleActor::evaluate_js_async(t, &console_actor, js)
            })
        };
        let mut refresh = ctx.take_navigation_started().is_some();
        match outcome {
            Ok(result) => {
                if let Some(exc) = result.exception {
                    let msg = match exc.message.as_deref() {
                        Some(m) => format!("{error_context}: {m}"),
                        None => error_context.to_owned(),
                    };
                    return Err(AppError::User(sanitize_for_terminal(&msg).into_owned()));
                }
                if is_truthy(&result.result) {
                    return Ok(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));
                }
            }
            Err(ProtocolError::Timeout) => {}
            Err(e) if is_document_gone(&e) => refresh = true,
            Err(e) => return Err(AppError::from(e)),
        }
        if Instant::now() >= deadline {
            return Err(AppError::Timeout(timeout_context.to_owned()));
        }
        if refresh {
            console_actor = follow_to_new_document(ctx, console_actor, deadline);
        }
        std::thread::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
    }
}

/// How long [`follow_to_new_document`] waits for `getTarget` to stop handing
/// back the outgoing document before polling whatever it has.
const HANDOVER_BUDGET: Duration = Duration::from_secs(3);

/// Re-resolve the target after a navigation signal, waiting (bounded) for a
/// document with a different `innerWindowId`: until the new one commits,
/// `getTarget` keeps returning the outgoing docshell, which may never answer
/// another evaluation. Returns the console actor to poll next.
fn follow_to_new_document(ctx: &mut ConnectedTab, current: ActorId, deadline: Instant) -> ActorId {
    let before = ctx.target().inner_window_id;
    let until = deadline.min(Instant::now() + HANDOVER_BUDGET);
    let mut console_actor = current;
    loop {
        if let Ok(actor) = ctx.refresh_target_until(until) {
            console_actor = actor;
            let now = ctx.target().inner_window_id;
            if before.is_none() || now.is_none() || now != before {
                return console_actor;
            }
        }
        if Instant::now() >= until {
            return console_actor;
        }
        std::thread::sleep(
            Duration::from_millis(50).min(until.saturating_duration_since(Instant::now())),
        );
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

    /// A navigation landing mid-wait surfaces as one of these on the old
    /// console actor; each moves the wait to the new document rather than
    /// failing it. Anything else is still an error.
    #[test]
    fn document_gone_errors_are_recognised() {
        assert!(is_document_gone(&ProtocolError::EvalNavigatedDuringEval));
        assert!(is_document_gone(&ProtocolError::EvalTargetDestroyed {
            inner_window_id: 7
        }));
        assert!(is_document_gone(&ProtocolError::ActorError {
            actor: "conn0/consoleActor2".to_owned(),
            kind: ActorErrorKind::UnknownActor,
            error: "noSuchActor".to_owned(),
            message: String::new(),
        }));
        assert!(!is_document_gone(&ProtocolError::ActorError {
            actor: "conn0/consoleActor2".to_owned(),
            kind: ActorErrorKind::WrongState,
            error: "wrongState".to_owned(),
            message: String::new(),
        }));
        assert!(!is_document_gone(&ProtocolError::Timeout));
    }

    #[test]
    fn build_wait_js_selector() {
        let opts = WaitOptions {
            selector: Some("button.submit"),
            text: None,
            eval: None,
            sleep_ms: None,
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
            sleep_ms: None,
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
            sleep_ms: None,
            wait_timeout: 5000,
        };
        let js = build_wait_js(&opts).unwrap();
        assert!(js.contains("document.readyState === 'complete'"));
    }

    // iter-142 Theme F: plain sleep form

    /// AC `e2e_wait_sleep_form` (unit half): `run_core` with `sleep_ms` set
    /// sleeps for approximately that duration and returns a `matched: true`
    /// result without requiring any condition field — it must never reach
    /// `connect_and_get_target` (which would fail with no live Firefox in a
    /// unit test), proving the sleep path really does skip the connection.
    #[test]
    fn run_core_sleep_form_does_not_require_a_connection() {
        use clap::Parser as _;
        let cli = Cli::try_parse_from(["ff-rdp", "wait", "--sleep-ms", "5"])
            .expect("should parse --sleep-ms 5");
        let opts = WaitOptions {
            selector: None,
            text: None,
            eval: None,
            sleep_ms: Some(5),
            wait_timeout: 5000,
        };
        let started = std::time::Instant::now();
        let result = run_core(&cli, &opts).expect("sleep form must succeed with no connection");
        let elapsed = started.elapsed();

        assert_eq!(result["matched"], true);
        assert_eq!(result["elapsed_ms"], 5);
        assert!(
            elapsed >= std::time::Duration::from_millis(5),
            "must actually sleep for the requested duration, elapsed={elapsed:?}"
        );
    }

    /// `--time` is accepted as a hidden legacy alias for `--sleep-ms` —
    /// this is the exact flag name dogfooding session 63 reached for first.
    #[test]
    fn wait_args_time_alias_parses_as_sleep_ms() {
        use crate::cli::args::Command;
        use clap::Parser as _;
        let cli = Cli::try_parse_from(["ff-rdp", "wait", "--time", "6000"])
            .expect("should parse --time 6000");
        let Command::Wait(args) = cli.command else {
            panic!("expected Command::Wait");
        };
        assert_eq!(args.sleep_ms, Some(6000));
    }

    // iter-85 Theme K-followup: deprecation warning for --timeout alias

    /// The deprecation message must contain the word "deprecat" (lowercase) so
    /// the dogfood script can grep for it reliably.
    #[test]
    fn timeout_alias_deprecation_message_contains_deprecat() {
        // Build the warning message string the same way `warn_if_timeout_alias_used` does,
        // without touching argv (which varies per test runner invocation).
        let msg = "warning: --timeout is deprecated for `wait`, use --timeout-ms instead \
             (this alias will be removed in a future release)";
        assert!(
            msg.contains("deprecat"),
            "deprecation message must contain 'deprecat'; got: {msg}"
        );
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
