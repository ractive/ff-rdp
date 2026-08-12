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
/// Returns the result value alongside the resolved route (`Some(via_daemon)`),
/// or `None` when `--sleep-ms` took the no-connection short-circuit below —
/// there is no route to report because no Firefox connection was ever
/// resolved (iter-134: `meta.route` on every command).
pub fn run_core(
    cli: &Cli,
    opts: &WaitOptions<'_>,
) -> Result<(serde_json::Value, Option<bool>), AppError> {
    // iter-142 Theme F: --sleep-ms is a plain delay — no condition to poll,
    // no Firefox connection needed at all. Takes priority over the other
    // fields so a caller that somehow sets both never falls through to the
    // (meaningless, since sleep_ms doesn't describe a JS condition)
    // condition-polling path below.
    if let Some(ms) = opts.sleep_ms {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        return Ok((
            json!({"matched": true, "elapsed_ms": ms, "condition": format!("sleep={ms}ms")}),
            None,
        ));
    }

    if opts.selector.is_none() && opts.text.is_none() && opts.eval.is_none() {
        return Err(AppError::User(
            "wait: specify at least one of --selector, --text, --eval, --ref, or --sleep-ms".into(),
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

    Ok((
        json!({"matched": true, "elapsed_ms": elapsed_ms, "condition": condition}),
        Some(ctx.via_daemon),
    ))
}

pub fn run(cli: &Cli, opts: &WaitOptions<'_>) -> Result<(), AppError> {
    warn_if_timeout_alias_used();
    let (result_json, via_daemon) = run_core(cli, opts)?;
    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    // iter-134: always present, not gated by --verbose, except when
    // --sleep-ms short-circuited before resolving a connection at all.
    if let Some(via_daemon) = via_daemon {
        crate::connection_meta::merge_route(&mut meta, via_daemon);
    }
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
        let (result, via_daemon) =
            run_core(&cli, &opts).expect("sleep form must succeed with no connection");
        let elapsed = started.elapsed();

        assert_eq!(result["matched"], true);
        assert_eq!(result["elapsed_ms"], 5);
        assert_eq!(
            via_daemon, None,
            "sleep form never resolves a connection, so there is no route to report"
        );
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
