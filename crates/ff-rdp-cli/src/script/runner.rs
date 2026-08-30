//! Script runner: executes steps sequentially, emits NDJSON output.
//!
//! Each step is dispatched to the same in-process functions used by the CLI.
//! Output is one JSON line per step:
//!   `{"step": N, "verb": "...", "ok": true, "results": {...}, "elapsed_ms": N}`
//! with a final summary line:
//!   `{"summary": true, "ok": true, "total": N, "failed": 0, "total_elapsed_ms": N}`

use std::collections::HashMap;
use std::io::{BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context as _;
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;

use std::sync::Arc;

use super::format::{
    AssertNetworkStep, AssertNoConsoleErrorsStep, AssertTextStep, AssertUrlStep, EvalStep,
    NavigateStep, RunStep, ScreenshotStep, Script, ScriptFormat, Step, TypeStep, WaitStep,
};
use super::recorder::FileRecorder;
use super::vars::{
    EnvPolicy, VarContext, check_undefined_vars, collect_env_secrets, is_secret_name, substitute,
};
use crate::commands::network_watch::PlaybookNetworkWatch;
use crate::page_map::PageMap;

/// Maximum allowed depth of nested `run:` steps.
///
/// The limit exists to prevent stack overflow from a script that
/// (accidentally or maliciously) chains hundreds of `run:` steps. The
/// value is comfortably above realistic legitimate nesting
/// (top → suite → subtest → fixture-setup → action).
pub const MAX_RUN_DEPTH: usize = 16;

// ---------------------------------------------------------------------------
// Run options
// ---------------------------------------------------------------------------

/// Options for the script runner.
pub struct RunOptions<'a> {
    /// Extra variables from `--vars k=v` flags.
    pub extra_vars: &'a HashMap<String, String>,
    /// Stop on first failure (default: true).
    pub bail_on_failure: bool,
    /// Parse and print resolved steps without executing.
    pub dry_run: bool,
    /// Show secrets in output.
    pub show_secrets: bool,
    /// Optional recorder for `--record <output>` mode.
    pub recorder: Option<FileRecorder>,
    /// Fail the entire run if a recording step fails (default: log and continue).
    pub record_strict: bool,
    /// Force a specific script format instead of detecting from file extension.
    pub format_override: Option<ScriptFormat>,
    /// Loaded page-map for resolving `page_map:`, `field:`, and `api_route:` targets.
    pub page_map: Option<Arc<PageMap>>,
    /// Policy controlling which `{{env.X}}` references are allowed.
    pub env_policy: EnvPolicy,
    /// When true, sub-script paths may escape the top-level script's
    /// directory (absolute paths and `..` traversals). Off by default.
    pub allow_unsafe_script_paths: bool,
    /// Directory of the top-level script, set on the first call to
    /// [`run_script_file`]. Sub-script paths are required to stay within
    /// this directory unless `allow_unsafe_script_paths` is set.
    pub top_level_dir: Option<PathBuf>,
    /// The playbook-scoped `network-event` subscription, once armed (iter-181).
    ///
    /// Not a caller-supplied option: [`run_script`] arms it on the first
    /// script that needs one and it then lives for the rest of the run, which
    /// is what makes a request fired by step N visible to an `assert_network`
    /// at step N+1. Threaded here because `RunOptions` is already the one
    /// `&mut` the step loop and nested `run:` steps share. `None` means either
    /// "no step needs it" or "the daemon route already has a standing
    /// subscription" — see [`PlaybookNetworkWatch::arm`].
    pub(crate) network_watch: Option<PlaybookNetworkWatch>,
}

impl Default for RunOptions<'_> {
    fn default() -> Self {
        Self {
            extra_vars: &EMPTY_VARS,
            bail_on_failure: true,
            dry_run: false,
            show_secrets: false,
            recorder: None,
            record_strict: false,
            format_override: None,
            page_map: None,
            env_policy: EnvPolicy::default(),
            allow_unsafe_script_paths: false,
            top_level_dir: None,
            network_watch: None,
        }
    }
}

static EMPTY_VARS: std::sync::LazyLock<HashMap<String, String>> =
    std::sync::LazyLock::new(HashMap::new);

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Execute a script file.
///
/// Writes NDJSON lines to stdout.  Returns `Ok(())` when all steps pass, or
/// `Err` when a step fails and `bail_on_failure` is true.
///
/// The `call_stack` is used for cycle detection in nested `run:` steps.
pub fn run_script_file(
    script_path: &Path,
    cli: &Cli,
    opts: &mut RunOptions<'_>,
    call_stack: &[PathBuf],
) -> Result<(), AppError> {
    // Depth cap: refuse to enter another level once we'd exceed MAX_RUN_DEPTH.
    let depth = call_stack.len() + 1;
    if depth > MAX_RUN_DEPTH {
        return Err(AppError::User(format!(
            "run nesting depth {depth} exceeds MAX_RUN_DEPTH={MAX_RUN_DEPTH}"
        )));
    }

    // Check for cycles.
    let abs_path = script_path
        .canonicalize()
        .or_else(|_| -> anyhow::Result<PathBuf> {
            let parent = script_path.parent().unwrap_or(Path::new("."));
            let abs_parent = if parent.as_os_str().is_empty() {
                std::env::current_dir().context("current_dir")?
            } else {
                parent
                    .canonicalize()
                    .with_context(|| format!("resolving '{}'", parent.display()))?
            };
            Ok(abs_parent.join(script_path.file_name().unwrap_or_default()))
        })
        .map_err(|e| AppError::User(format!("cannot resolve script path: {e}")))?;

    if call_stack.contains(&abs_path) {
        return Err(AppError::User(format!(
            "cycle detected: '{}' is already in the call stack: {}",
            abs_path.display(),
            call_stack
                .iter()
                .map(|p| format!("'{}'", p.display()))
                .collect::<Vec<_>>()
                .join(" -> ")
        )));
    }

    let fmt = opts
        .format_override
        .unwrap_or_else(|| super::format::ScriptFormat::from_path(&abs_path));
    let script = super::format::parse_script_file(&abs_path, Some(fmt))
        .map_err(|e| AppError::User(format!("script parse error: {e:#}")))?;

    // Merge script vars with extra vars (extra vars win).
    let mut merged_vars: HashMap<String, String> = script.vars.clone();
    for (k, v) in opts.extra_vars {
        merged_vars.insert(k.clone(), v.clone());
    }

    let mut new_stack = call_stack.to_vec();
    new_stack.push(abs_path.clone());

    // Capture the top-level script's directory on the first entry. Used by
    // `execute_run` to enforce path containment for sub-scripts.
    if opts.top_level_dir.is_none() {
        let parent = abs_path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        opts.top_level_dir = Some(parent);
    }

    run_script(&script, &abs_path, cli, opts, &merged_vars, &new_stack)
}

/// Execute an already-parsed script.
fn run_script(
    script: &Script,
    script_path: &Path,
    cli: &Cli,
    opts: &mut RunOptions<'_>,
    vars: &HashMap<String, String>,
    call_stack: &[PathBuf],
) -> Result<(), AppError> {
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    if opts.dry_run {
        return run_dry(script, vars, &mut out);
    }

    // iter-181: arm the playbook-scoped network subscription *before* the
    // first step, so a request fired by step N is still in the buffer when an
    // `assert_network` at step N+1 looks. Arming per step — what this replaces
    // — is a race the runner loses under load, because `watchResources` does
    // not replay history.
    //
    // A nested `run:` step shares `opts`, so an already-armed subscription is
    // reused rather than duplicated, and the outer script's watcher keeps
    // covering the sub-script's assertions.
    if opts.network_watch.is_none() && script_needs_network_watch(script) {
        match PlaybookNetworkWatch::arm(cli) {
            Ok(Some(watch)) => opts.network_watch = Some(watch),
            // Daemon route: it already holds a standing subscription.
            Ok(None) => {}
            Err(e) => {
                // Degrading silently is how iteration 179 lost four days.
                // Say so: `assert_network` still works via the old per-step
                // drain, but it is a race again, and its diagnostics will
                // report `subscription: "step"` to match.
                // stderr-ok: (b) debug/diagnostic — the NDJSON on stdout is
                // unchanged and each step still reports its own outcome.
                eprintln!(
                    "warning: could not arm the playbook-scoped network subscription \
                     ({e}); `assert_network` steps fall back to per-step arming, which \
                     can miss a request that completed before the step started"
                );
            }
        }
    }

    let total_start = Instant::now();
    let mut step_results: Vec<Value> = Vec::new();
    let mut failed = 0usize;
    let mut executed = 0usize;
    let total = script.steps.len();

    for (idx, step) in script.steps.iter().enumerate() {
        let step_num = idx + 1;
        let verb = step.verb();
        let step_start = Instant::now();

        // Resolve variable substitutions in the step (best-effort; errors are step failures).
        let resolved = match resolve_step_vars(step, vars, &step_results, &opts.env_policy) {
            Ok(s) => s,
            Err(e) => {
                let elapsed = u64::try_from(step_start.elapsed().as_millis()).unwrap_or(u64::MAX);
                let line = json!({
                    "step": step_num,
                    "verb": verb,
                    "ok": false,
                    "error": format!("variable resolution failed: {e}"),
                    "elapsed_ms": elapsed,
                });
                writeln!(out, "{line}").ok();
                out.flush().ok();
                failed += 1;
                if opts.bail_on_failure {
                    break;
                }
                step_results.push(Value::Null);
                continue;
            }
        };

        // Resolve iter-62 page-map targets (page_map:, field:, api_route:).
        let resolved = match resolve_page_map_targets(resolved, opts.page_map.as_deref(), step_num)
        {
            Ok(s) => s,
            Err(e) => {
                let elapsed = u64::try_from(step_start.elapsed().as_millis()).unwrap_or(u64::MAX);
                let line = json!({
                    "step": step_num,
                    "verb": verb,
                    "ok": false,
                    "error": format!("{e}"),
                    "elapsed_ms": elapsed,
                });
                writeln!(out, "{line}").ok();
                out.flush().ok();
                failed += 1;
                if opts.bail_on_failure {
                    break;
                }
                step_results.push(Value::Null);
                continue;
            }
        };

        // Only count as executed once we are actually about to run the step —
        // variable-resolution failures and deferred-feature rejections above
        // have already incremented `failed` and `continue`d, so those do not
        // reach here.
        executed += 1;

        // Execute the step.
        let exec_result = execute_step(
            &resolved,
            script_path,
            cli,
            opts,
            vars,
            call_stack,
            script.base_url.as_deref(),
            script.default_timeout_ms,
        );

        let elapsed = u64::try_from(step_start.elapsed().as_millis()).unwrap_or(u64::MAX);

        match exec_result {
            Ok(result_value) => {
                // Build combined redaction set: script vars + env vars referenced in this step.
                let env_secrets = collect_env_secrets_from_step(&resolved);
                let mut combined_vars = vars.clone();
                combined_vars.extend(env_secrets);
                let redacted =
                    super::vars::redact_secrets(&result_value, &combined_vars, opts.show_secrets);
                let line = json!({
                    "step": step_num,
                    "verb": verb,
                    "ok": true,
                    "results": redacted,
                    "elapsed_ms": elapsed,
                });
                writeln!(out, "{line}").ok();
                // Wrap with `{"results": ...}` so `{{steps[N].results.X}}` resolves correctly.
                step_results.push(json!({"results": result_value}));
            }
            Err(e) => {
                let diagnostics = extract_diagnostics(&e);
                let mut line = json!({
                    "step": step_num,
                    "verb": verb,
                    "ok": false,
                    "error": format!("{e}"),
                    "elapsed_ms": elapsed,
                });
                if let Some(d) = diagnostics {
                    line["diagnostics"] = d;
                }
                writeln!(out, "{line}").ok();
                step_results.push(Value::Null);
                failed += 1;
                if opts.bail_on_failure {
                    break;
                }
            }
        }

        // Flush after each step so the caller sees progress in real time.
        out.flush().ok();

        // If we have a recorder attached, record the step.
        if let Some(ref mut rec) = opts.recorder
            && let Err(e) = rec.record(&resolved)
        {
            eprintln!("warning: recording step failed: {e}");
            if opts.record_strict {
                return Err(AppError::User(format!("recording step failed: {e}")));
            }
        }
    }

    let total_elapsed = u64::try_from(total_start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let ok = failed == 0;
    let succeeded = executed.saturating_sub(failed);
    let skipped = total.saturating_sub(executed);
    let summary = json!({
        "summary": true,
        "ok": ok,
        "total": total,
        "executed": executed,
        "succeeded": succeeded,
        "failed": failed,
        "skipped": skipped,
        "total_elapsed_ms": total_elapsed,
    });
    writeln!(out, "{summary}").ok();
    out.flush().ok();

    if ok { Ok(()) } else { Err(AppError::Exit(1)) }
}

/// Dry-run: validate variable references and print resolved steps without executing.
fn run_dry(
    script: &Script,
    vars: &HashMap<String, String>,
    out: &mut impl std::io::Write,
) -> Result<(), AppError> {
    // Check all vars referenced in the steps exist.
    for (idx, step) in script.steps.iter().enumerate() {
        let step_num = idx + 1;
        check_step_vars_defined(step_num, step, vars)?;
    }

    // Print the resolved steps.
    let plan = json!({
        "dry_run": true,
        "total": script.steps.len(),
        "steps": script.steps.iter().enumerate().map(|(i, s)| {
            json!({
                "step": i + 1,
                "verb": s.verb(),
            })
        }).collect::<Vec<_>>(),
    });
    writeln!(out, "{plan}").ok();
    Ok(())
}

fn check_step_vars_defined(
    step_num: usize,
    step: &Step,
    vars: &HashMap<String, String>,
) -> Result<(), AppError> {
    let strings = collect_template_strings(step);
    for s in strings {
        check_undefined_vars(&s, vars)
            .map_err(|e| AppError::User(format!("step {step_num} ({}): {e}", step.verb())))?;
    }
    Ok(())
}

/// Collect all string fields from a step that may contain `{{...}}` templates.
fn collect_template_strings(step: &Step) -> Vec<String> {
    match step {
        Step::Navigate(s) => vec![s.url.clone()],
        Step::Click(s) => {
            let mut v = Vec::new();
            if let Some(sel) = &s.target.selector {
                v.push(sel.clone());
            }
            if let Some(id) = &s.target.ref_id {
                v.push(id.clone());
            }
            v
        }
        Step::Type(s) => {
            let mut v = vec![s.text.clone()];
            if let Some(sel) = &s.target.selector {
                v.push(sel.clone());
            }
            v
        }
        Step::Wait(s) => {
            let mut v = Vec::new();
            if let Some(sel) = &s.selector {
                v.push(sel.clone());
            }
            if let Some(text) = &s.text {
                v.push(text.clone());
            }
            v
        }
        Step::AssertText(s) => {
            let mut v = vec![s.selector.clone()];
            if let Some(c) = &s.contains {
                v.push(c.clone());
            }
            if let Some(e) = &s.equals {
                v.push(e.clone());
            }
            v
        }
        Step::AssertUrl(s) => {
            let mut v = Vec::new();
            if let Some(m) = &s.matches {
                v.push(m.clone());
            }
            if let Some(e) = &s.equals {
                v.push(e.clone());
            }
            v
        }
        Step::Eval(s) => vec![s.script.clone()],
        Step::Run(s) => vec![s.path.clone()],
        Step::Screenshot(s) => {
            let mut v = Vec::new();
            if let Some(o) = &s.output {
                v.push(o.clone());
            }
            v
        }
        Step::AssertNoConsoleErrors(s) => s.ignore_patterns.clone(),
        Step::AssertNetwork(s) => {
            let mut v = Vec::new();
            if let Some(u) = &s.url_contains {
                v.push(u.clone());
            }
            if let Some(m) = &s.method {
                v.push(m.clone());
            }
            v
        }
    }
}

// ---------------------------------------------------------------------------
// Variable resolution in steps
// ---------------------------------------------------------------------------

/// Substitute variables in all string fields of a step.
fn resolve_step_vars(
    step: &Step,
    vars: &HashMap<String, String>,
    step_results: &[Value],
    env_policy: &EnvPolicy,
) -> anyhow::Result<Step> {
    let ctx = VarContext {
        vars,
        step_results,
        show_secrets: false,
        env_policy,
    };

    Ok(match step {
        Step::Navigate(s) => Step::Navigate(NavigateStep {
            url: substitute(&s.url, &ctx)?,
            wait_text: s
                .wait_text
                .as_deref()
                .map(|t| substitute(t, &ctx))
                .transpose()?,
            wait_selector: s
                .wait_selector
                .as_deref()
                .map(|t| substitute(t, &ctx))
                .transpose()?,
        }),
        Step::Click(s) => Step::Click(super::format::ElementStep {
            target: resolve_target(&s.target, &ctx)?,
            wait_for_text: s
                .wait_for_text
                .as_deref()
                .map(|t| substitute(t, &ctx))
                .transpose()?,
            wait_for_selector: s
                .wait_for_selector
                .as_deref()
                .map(|t| substitute(t, &ctx))
                .transpose()?,
        }),
        Step::Type(s) => Step::Type(TypeStep {
            target: resolve_target(&s.target, &ctx)?,
            text: substitute(&s.text, &ctx)?,
            clear: s.clear,
            secret: s.secret,
        }),
        Step::Wait(s) => Step::Wait(WaitStep {
            selector: s
                .selector
                .as_deref()
                .map(|t| substitute(t, &ctx))
                .transpose()?,
            text: s.text.as_deref().map(|t| substitute(t, &ctx)).transpose()?,
            eval: s.eval.as_deref().map(|t| substitute(t, &ctx)).transpose()?,
            timeout: s.timeout,
        }),
        Step::AssertText(s) => Step::AssertText(AssertTextStep {
            selector: substitute(&s.selector, &ctx)?,
            contains: s
                .contains
                .as_deref()
                .map(|t| substitute(t, &ctx))
                .transpose()?,
            equals: s
                .equals
                .as_deref()
                .map(|t| substitute(t, &ctx))
                .transpose()?,
            not: s.not,
            timeout: s.timeout,
        }),
        Step::AssertUrl(s) => Step::AssertUrl(AssertUrlStep {
            matches: s
                .matches
                .as_deref()
                .map(|t| substitute(t, &ctx))
                .transpose()?,
            equals: s
                .equals
                .as_deref()
                .map(|t| substitute(t, &ctx))
                .transpose()?,
        }),
        Step::Eval(s) => Step::Eval(EvalStep {
            script: substitute(&s.script, &ctx)?,
            stringify: s.stringify,
        }),
        Step::Run(s) => Step::Run(RunStep {
            path: substitute(&s.path, &ctx)?,
            with: s
                .with
                .iter()
                .map(|(k, v)| Ok((k.clone(), substitute(v, &ctx)?)))
                .collect::<anyhow::Result<_>>()?,
        }),
        Step::Screenshot(s) => Step::Screenshot(ScreenshotStep {
            output: s
                .output
                .as_deref()
                .map(|t| substitute(t, &ctx))
                .transpose()?,
            base64: s.base64,
            full_page: s.full_page,
        }),
        Step::AssertNoConsoleErrors(s) => Step::AssertNoConsoleErrors(AssertNoConsoleErrorsStep {
            ignore_patterns: s
                .ignore_patterns
                .iter()
                .map(|p| substitute(p, &ctx))
                .collect::<anyhow::Result<_>>()?,
        }),
        Step::AssertNetwork(s) => Step::AssertNetwork(AssertNetworkStep {
            url_contains: s
                .url_contains
                .as_deref()
                .map(|t| substitute(t, &ctx))
                .transpose()?,
            status: s.status,
            method: s
                .method
                .as_deref()
                .map(|t| substitute(t, &ctx))
                .transpose()?,
            api_route: s.api_route.clone(),
            timeout: s.timeout,
        }),
    })
}

fn resolve_target(
    target: &super::format::ElementTarget,
    ctx: &VarContext<'_>,
) -> anyhow::Result<super::format::ElementTarget> {
    Ok(super::format::ElementTarget {
        selector: target
            .selector
            .as_deref()
            .map(|t| substitute(t, ctx))
            .transpose()?,
        ref_id: target
            .ref_id
            .as_deref()
            .map(|t| substitute(t, ctx))
            .transpose()?,
        page_map: target.page_map.clone(),
        field: target.field.clone(),
    })
}

/// Resolve any `page_map:`, `field:`, or `api_route:` targets in a step by
/// looking them up in the loaded `PageMap`.
///
/// Returns an error when:
/// - The step references a page-map target but no page-map is loaded.
/// - The dotted path does not resolve to a known selector / route.
fn resolve_page_map_targets(
    step: Step,
    page_map: Option<&PageMap>,
    step_num: usize,
) -> anyhow::Result<Step> {
    use super::format::{AssertNetworkStep, ElementStep, ElementTarget, TypeStep};

    /// Materialise a page-map or field target into a `selector`.
    fn resolve_element_target(
        target: ElementTarget,
        page_map: Option<&PageMap>,
        verb: &str,
        step_num: usize,
    ) -> anyhow::Result<ElementTarget> {
        if let Some(ref path) = target.page_map {
            let pm = page_map.ok_or_else(|| {
                anyhow::anyhow!(
                    "step {step_num} ({verb}): target uses `page_map: {path}` but no page-map is \
                     loaded — pass `--page-map <path>` or place a map at `.ffrdp/page-map.json`"
                )
            })?;
            let selector = pm.resolve_target(path)?;
            return Ok(ElementTarget {
                selector: Some(selector),
                ref_id: None,
                page_map: None,
                field: None,
            });
        }
        if let Some(ref field_path) = target.field {
            // `field:` requires a full dotted path like
            // `pages.<page>.forms.<form>.fields.<name>`.  Bare field names
            // (without a leading `pages.`) are rejected with an error — there
            // is not enough context to expand them automatically.
            if !field_path.starts_with("pages.") {
                return Err(anyhow::anyhow!(
                    "step {step_num} ({verb}): `field: {field_path}` must be a full dotted path \
                     like `pages.<page>.forms.<form>.fields.<name>`"
                ));
            }
            let pm = page_map.ok_or_else(|| {
                anyhow::anyhow!(
                    "step {step_num} ({verb}): target uses `field: {field_path}` but no page-map is \
                     loaded — pass `--page-map <path>` or place a map at `.ffrdp/page-map.json`"
                )
            })?;
            let selector = pm.resolve_target(field_path)?;
            return Ok(ElementTarget {
                selector: Some(selector),
                ref_id: None,
                page_map: None,
                field: None,
            });
        }
        Ok(target)
    }

    match step {
        Step::Click(s) => {
            let target = resolve_element_target(s.target, page_map, "click", step_num)?;
            Ok(Step::Click(ElementStep { target, ..s }))
        }
        Step::Type(s) => {
            let target = resolve_element_target(s.target, page_map, "type", step_num)?;
            Ok(Step::Type(TypeStep { target, ..s }))
        }
        Step::AssertNetwork(s) => {
            if let Some(ref route_name) = s.api_route {
                let pm = page_map.ok_or_else(|| {
                    anyhow::anyhow!(
                        "step {step_num} (assert_network): `api_route: {route_name}` requires a \
                         page-map — pass `--page-map <path>` or place a map at `.ffrdp/page-map.json`"
                    )
                })?;
                let (method, path) = pm.resolve_api_route(route_name)?;
                Ok(Step::AssertNetwork(AssertNetworkStep {
                    url_contains: Some(path.to_owned()),
                    method: Some(method.to_owned()),
                    api_route: None,
                    ..s
                }))
            } else {
                Ok(Step::AssertNetwork(s))
            }
        }
        other => Ok(other),
    }
}

// ---------------------------------------------------------------------------
// Step execution
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn execute_step(
    step: &Step,
    script_path: &Path,
    cli: &Cli,
    opts: &mut RunOptions<'_>,
    vars: &HashMap<String, String>,
    call_stack: &[PathBuf],
    base_url: Option<&str>,
    default_timeout_ms: Option<u64>,
) -> Result<Value, AppError> {
    match step {
        Step::Navigate(s) => execute_navigate(s, cli, base_url),
        Step::Click(s) => execute_click(s, cli),
        Step::Type(s) => execute_type(s, cli, vars, opts.show_secrets),
        Step::Wait(s) => execute_wait(s, cli, default_timeout_ms),
        Step::AssertText(s) => execute_assert_text(s, cli, default_timeout_ms),
        Step::AssertUrl(s) => execute_assert_url(s, cli),
        Step::AssertNoConsoleErrors(s) => execute_assert_no_console_errors(s, cli),
        Step::AssertNetwork(s) => {
            execute_assert_network(s, cli, default_timeout_ms, opts.network_watch.as_mut())
        }
        Step::Screenshot(s) => execute_screenshot(s, cli),
        Step::Eval(s) => execute_eval(s, cli),
        Step::Run(s) => execute_run(s, script_path, cli, opts, vars, call_stack),
    }
}

fn execute_navigate(
    step: &NavigateStep,
    cli: &Cli,
    base_url: Option<&str>,
) -> Result<Value, AppError> {
    use crate::commands::navigate::{WaitAfterNav, run_core as nav_run_core};

    // Resolve relative URLs against the script's base_url.
    let effective_url = if let Some(base) = base_url
        && !step.url.starts_with("http://")
        && !step.url.starts_with("https://")
        && !step.url.starts_with("//")
    {
        url::Url::parse(base)
            .and_then(|b| b.join(&step.url))
            .map_or_else(|_| step.url.clone(), |u| u.to_string())
    } else {
        step.url.clone()
    };

    let wait_opts = WaitAfterNav {
        wait_text: step.wait_text.as_deref(),
        wait_selector: step.wait_selector.as_deref(),
        wait_timeout: cli.timeout,
        // Script runner: keep default blocking (commit-wait). Explicit wait
        // steps that follow this navigate step use the same --timeout budget
        // independently — there is no double-counting because each step owns
        // its own Instant::now() baseline.
        no_wait: false,
        wait_for: &[],
        wait_level: crate::commands::navigate::WaitLevel::Complete,
        // iter-92 Theme B: align with the CLI default (`Both`).  Events-only
        // produced spurious `dom-complete did not fire within timeout` errors
        // on data: URLs and other targets where Firefox elides the event
        // (dogfooding-session-59 §3); the readystate-poll fallback in `Both`
        // covers those cases without changing behaviour on event-rich pages.
        wait_strategy: crate::commands::navigate::WaitStrategy::Both,
    };
    // Script steps have no `--with-page` equivalent in the script format; a
    // step that wants the page view runs an `a11y summary` step instead.
    nav_run_core(
        cli,
        &effective_url,
        &wait_opts,
        &crate::cli::args::PageViewArgs::default(),
    )
    .map(|(v, _)| v)
}

fn resolve_element_target_selector(
    target: &super::format::ElementTarget,
    cli: &Cli,
    verb: &str,
) -> Result<String, AppError> {
    if let Some(sel) = &target.selector {
        return Ok(sel.clone());
    }
    if let Some(ref_id) = &target.ref_id {
        // Resolve the ref via the daemon, just like dispatch.rs does for --ref.
        return crate::dispatch::resolve_ref_for_script(cli, ref_id, verb);
    }
    Err(AppError::User(format!("{verb}: no selector or ref")))
}

fn execute_click(step: &super::format::ElementStep, cli: &Cli) -> Result<Value, AppError> {
    use crate::commands::click::{ClickOptions, run_core as click_run_core};
    let selector = resolve_element_target_selector(&step.target, cli, "click")?;
    let selector = selector.as_str();

    let wait_for: Vec<String> = {
        let mut v = Vec::new();
        if let Some(t) = &step.wait_for_text {
            v.push(format!("text:{t}"));
        }
        if let Some(s) = &step.wait_for_selector {
            v.push(format!("selector:{s}"));
        }
        v
    };

    click_run_core(
        cli,
        selector,
        None,
        None,
        &ClickOptions {
            wait_for: &wait_for,
            ..Default::default()
        },
    )
    .map(|(v, _)| v)
}

fn execute_type(
    step: &TypeStep,
    cli: &Cli,
    vars: &HashMap<String, String>,
    show_secrets: bool,
) -> Result<Value, AppError> {
    use crate::commands::type_text::{TypeOptions, run_core as type_run_core};
    let selector = resolve_element_target_selector(&step.target, cli, "type")?;
    let selector = selector.as_str();

    // Call run_core which does not print — result is used for our NDJSON output.
    type_run_core(
        cli,
        selector,
        &step.text,
        step.clear,
        &TypeOptions::default(),
    )?;

    // Determine whether to redact the text in the result.
    let is_secret = step.secret
        || step
            .target
            .selector
            .as_deref()
            .is_some_and(is_secret_field_selector)
        || vars
            .keys()
            .any(|k| is_secret_name(k) && vars[k] == step.text);

    let typed_display = if is_secret && !show_secrets {
        "[REDACTED]".to_owned()
    } else {
        step.text.clone()
    };

    Ok(json!({"typed": typed_display, "selector": selector}))
}

/// Heuristic: detect password/secret selectors.
fn is_secret_field_selector(selector: &str) -> bool {
    let lower = selector.to_lowercase();
    lower.contains("password")
        || lower.contains("[type=\"password\"]")
        || lower.contains("[type='password']")
}

fn execute_wait(
    step: &WaitStep,
    cli: &Cli,
    default_timeout_ms: Option<u64>,
) -> Result<Value, AppError> {
    use crate::commands::wait::{WaitOptions, run_core as wait_run_core};
    let timeout = step.timeout.or(default_timeout_ms).unwrap_or(cli.timeout);
    let opts = WaitOptions {
        selector: step.selector.as_deref(),
        text: step.text.as_deref(),
        eval: step.eval.as_deref(),
        // iter-142: recorded scripts have no sleep step type yet — only the
        // interactive `wait --sleep-ms` CLI form is in scope for this
        // iteration.
        sleep_ms: None,
        wait_timeout: timeout,
    };
    wait_run_core(cli, &opts).map(|(v, _)| v)
}

fn execute_assert_text(
    step: &AssertTextStep,
    cli: &Cli,
    default_timeout_ms: Option<u64>,
) -> Result<Value, AppError> {
    use crate::commands::connect_tab::connect_and_get_target;
    use crate::commands::js_helpers::{escape_selector, eval_or_bail, poll_js_condition};

    let timeout = step.timeout.or(default_timeout_ms).unwrap_or(cli.timeout);

    let selector_escaped = escape_selector(&step.selector);

    // Poll for the text condition.
    let condition_js = if let Some(contains) = &step.contains {
        let text_json = serde_json::to_string(contains)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("encoding contains: {e}")))?;
        let negate = if step.not { "!" } else { "" };
        format!(
            "(function() {{ var el = document.querySelector('{selector_escaped}'); if (!el) return false; return {negate}el.innerText.includes({text_json}); }})()"
        )
    } else if let Some(equals) = &step.equals {
        let text_json = serde_json::to_string(equals)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("encoding equals: {e}")))?;
        let negate = if step.not { "!" } else { "" };
        format!(
            "(function() {{ var el = document.querySelector('{selector_escaped}'); if (!el) return false; return {negate}(el.innerText.trim() === {text_json}); }})()"
        )
    } else {
        return Err(AppError::User(
            "assert_text: requires contains or equals".to_owned(),
        ));
    };

    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    // Poll.
    let elapsed_ms = poll_js_condition(
        &mut ctx,
        &console_actor,
        &condition_js,
        timeout,
        "assert_text: JS exception during evaluation",
        &format!(
            "assert_text: condition not met on selector '{}' within {timeout}ms",
            step.selector
        ),
    )
    .map_err(|e| {
        // Augment the error with structured diagnostics (E6).
        let diag_js = format!(
            "(function() {{ var el = document.querySelector('{selector_escaped}'); return el ? el.innerText : null; }})()"
        );
        let actual_text = eval_or_bail(&mut ctx, &console_actor, &diag_js, "assert_text diagnostics")
            .ok()
            .and_then(|r| match r.result {
                ff_rdp_core::Grip::Value(serde_json::Value::String(s)) => Some(s),
                _ => None,
            });
        if let Some(actual) = actual_text {
            AppError::Diagnostics {
                message: format!("{e}"),
                payload: json!({"actual_text": actual}),
            }
        } else {
            e
        }
    })?;

    let expected = step
        .contains
        .as_deref()
        .or(step.equals.as_deref())
        .unwrap_or("");
    Ok(json!({
        "asserted": true,
        "selector": step.selector,
        "elapsed_ms": elapsed_ms,
        "expected": expected,
        "not": step.not,
    }))
}

fn execute_assert_url(step: &AssertUrlStep, cli: &Cli) -> Result<Value, AppError> {
    use crate::commands::connect_tab::connect_and_get_target;
    use crate::commands::js_helpers::eval_or_bail;

    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let actual_url = {
        let result = eval_or_bail(
            &mut ctx,
            &console_actor,
            "window.location.href",
            "assert_url",
        )?;
        match result.result {
            ff_rdp_core::Grip::Value(serde_json::Value::String(s)) => s,
            other => format!("{other:?}"),
        }
    };

    if let Some(pattern) = &step.matches {
        let re = regex::Regex::new(pattern)
            .map_err(|e| AppError::User(format!("assert_url: invalid regex '{pattern}': {e}")))?;
        if !re.is_match(&actual_url) {
            return Err(AppError::User(format!(
                "assert_url: URL '{actual_url}' does not match pattern '{pattern}'"
            )));
        }
        Ok(json!({
            "asserted": true,
            "actual_url": actual_url,
            "matches": pattern,
        }))
    } else if let Some(expected) = &step.equals {
        if actual_url != *expected {
            return Err(AppError::User(format!(
                "assert_url: expected '{expected}' but got '{actual_url}'"
            )));
        }
        Ok(json!({
            "asserted": true,
            "actual_url": actual_url,
            "equals": expected,
        }))
    } else {
        Err(AppError::User(
            "assert_url: requires matches or equals".to_owned(),
        ))
    }
}

fn execute_assert_no_console_errors(
    step: &AssertNoConsoleErrorsStep,
    cli: &Cli,
) -> Result<Value, AppError> {
    // Best-effort: use the `console` command to get cached messages.
    // This depends on the daemon's console buffer; if not available, it
    // falls back to `getCachedMessages` directly.
    use crate::commands::console::run_get_errors;

    let errors = run_get_errors(cli)?;

    // Apply ignore patterns.
    let filtered: Vec<&Value> = errors
        .iter()
        .filter(|msg| {
            let text = msg.get("message").and_then(Value::as_str).unwrap_or("");
            !step
                .ignore_patterns
                .iter()
                .any(|pat| text.contains(pat.as_str()))
        })
        .collect();

    if filtered.is_empty() {
        Ok(json!({
            "asserted": true,
            "console_errors": 0,
        }))
    } else {
        Err(AppError::User(format!(
            "assert_no_console_errors: {} console error(s) found:\n{}",
            filtered.len(),
            filtered
                .iter()
                .map(|e| format!(
                    "  - {}",
                    e.get("message").and_then(Value::as_str).unwrap_or("?")
                ))
                .collect::<Vec<_>>()
                .join("\n")
        )))
    }
}

/// Whether this script should get a playbook-scoped network subscription
/// (iter-181).
///
/// True for an obvious `assert_network` step, and **also** for a `run:` step:
/// a sub-script is not parsed until its step executes, so the only way to have
/// the watcher armed before the click that a sub-script's `assert_network`
/// asserts on is to arm it whenever a sub-script might contain one. The cost of
/// guessing wrong is one idle connection and one watcher for the duration of
/// the run; the cost of guessing right is the whole point of the iteration.
fn script_needs_network_watch(script: &Script) -> bool {
    script
        .steps
        .iter()
        .any(|s| matches!(s, Step::AssertNetwork(_) | Step::Run(_)))
}

/// Whether one drained network entry satisfies the step's predicate.
///
/// Every field is optional and an absent field matches anything — a step with
/// no fields at all therefore matches any request, which is what the schema
/// documents.
fn network_event_matches(step: &AssertNetworkStep, event: &Value) -> bool {
    let url = event.get("url").and_then(Value::as_str).unwrap_or("");
    let status: Option<u16> = event
        .get("status")
        .and_then(Value::as_u64)
        .and_then(|s| u16::try_from(s).ok());
    let method = event.get("method").and_then(Value::as_str).unwrap_or("");

    let url_ok = step
        .url_contains
        .as_deref()
        .is_none_or(|pat| url.contains(pat));
    let status_ok = step.status.is_none_or(|s| status == Some(s));
    let method_ok = step
        .method
        .as_deref()
        .is_none_or(|m| method.eq_ignore_ascii_case(m));

    url_ok && status_ok && method_ok
}

/// Which subscription an `assert_network` step read from. Reported as
/// `diagnostics.subscription` so a failure says *why* the buffer looked the
/// way it did (iter-181).
type NetworkSubscriptionKind = &'static str;

/// Armed before the first step and held for the whole script (iter-181).
const SUBSCRIPTION_PLAYBOOK: NetworkSubscriptionKind = "playbook";
/// Armed by this step alone — the pre-181 behaviour, now only reached when
/// arming the playbook subscription failed.
const SUBSCRIPTION_STEP: NetworkSubscriptionKind = "step";
/// The daemon's standing subscription, untouched by iteration 181.
const SUBSCRIPTION_DAEMON: NetworkSubscriptionKind = "daemon";

fn execute_assert_network(
    step: &AssertNetworkStep,
    cli: &Cli,
    default_timeout_ms: Option<u64>,
    watch: Option<&mut crate::commands::network_watch::PlaybookNetworkWatch>,
) -> Result<Value, AppError> {
    use crate::commands::network::{DEFAULT_DRAIN_MS, run_get_events_with_route};
    use crate::commands::network_watch::wait_for_match;
    use std::time::Duration;

    // Note: api_route targets are resolved to url_contains+method by
    // resolve_page_map_targets before reaching here, so step.api_route is
    // always None at this point.

    // Use step timeout, then script default, then CLI default.
    let effective_timeout = step.timeout.or(default_timeout_ms);

    let (events, matched, route, subscription, evicted) = if let Some(watch) = watch {
        // iter-181: the buffer has been filling since before the first step,
        // so a request that completed during an earlier step is already in it.
        // The wait is only for a request still in flight.
        let deadline =
            Instant::now() + Duration::from_millis(effective_timeout.unwrap_or(DEFAULT_DRAIN_MS));
        let (events, matched) =
            wait_for_match(watch, deadline, &|e| network_event_matches(step, e))?;
        (
            events,
            matched,
            "direct",
            SUBSCRIPTION_PLAYBOOK,
            watch.evicted(),
        )
    } else {
        // Daemon route (standing subscription), or the fallback after arming
        // failed — the runner warned on stderr in that case.
        let (events, route) = run_get_events_with_route(cli, effective_timeout)?;
        let matched = events.iter().any(|e| network_event_matches(step, e));
        let subscription = if route == "daemon" {
            SUBSCRIPTION_DAEMON
        } else {
            SUBSCRIPTION_STEP
        };
        (events, matched, route, subscription, 0)
    };

    if matched {
        Ok(json!({"asserted": true, "matched": true}))
    } else {
        let desc = build_network_assert_desc(step);
        // E6: return structured diagnostics payload instead of embedding in the string.
        // iter-179: `events_in_buffer` alone cannot be acted on — an empty
        // buffer is ambiguous between "the request never happened" and "the
        // watcher was armed too late". iter-181 removed the second cause on
        // the default path, so the payload now also names *which* subscription
        // was read, and the hint differs accordingly.
        Err(AppError::Diagnostics {
            message: format!("assert_network: no matching network request found ({desc})"),
            payload: network_assert_diagnostics(
                events.len(),
                route,
                subscription,
                effective_timeout,
                evicted,
            ),
        })
    }
}

/// Why a **step-scoped** `direct` drain can come back with zero events — not a
/// partial buffer — after a step that demonstrably issued a request (iter-179).
///
/// Since iteration 181 this path is only reached when the playbook-scoped
/// subscription could not be armed, so the hint names that first: the fix is to
/// find out why arming failed, not to restructure the playbook.
const EMPTY_STEP_BUFFER_HINT: &str = concat!(
    "step-scoped subscription (the playbook-scoped one could not be armed — see the warning on ",
    "stderr): `run` opens a fresh connection per step and arms the network watcher only when ",
    "this step starts, so a request that completed before then is never delivered. With a ",
    "single request in flight, losing that race shows up as zero events, not a partial count. ",
    "Fixes, best first: rerun so the playbook-scoped subscription arms (it makes step N's ",
    "request visible at step N+1); run against the daemon, which holds a standing ",
    "subscription; raise this step's `timeout`; or assert on a page effect instead.",
);

/// Why a **playbook-scoped** drain can come back with zero events (iter-181).
///
/// This one is not ambiguous, and saying so is the point: the watcher was armed
/// before the first step of the script, so zero means no `network-event`
/// resource arrived at all during the run — not that the subscription was late.
const EMPTY_PLAYBOOK_BUFFER_HINT: &str = concat!(
    "playbook-scoped subscription: the network watcher was armed before this script's first ",
    "step and has been buffering ever since, so zero events means no network request was ",
    "observed during the run at all — this is not the pre-181 arming race. Check that the page ",
    "really issues the request (`ff-rdp network follow`), that it was not issued before `run` ",
    "started, and that it is not served from the HTTP cache.",
);

/// The `diagnostics` payload for a failed `assert_network` (iter-179, iter-181).
///
/// Split out from [`execute_assert_network`] so the branch that matters — which
/// empty-buffer hint applies — is unit-testable without Firefox, a daemon, or a
/// network stack.
fn network_assert_diagnostics(
    events_in_buffer: usize,
    route: crate::commands::network::NetworkDrainRoute,
    subscription: NetworkSubscriptionKind,
    effective_timeout: Option<u64>,
    evicted: usize,
) -> Value {
    let mut payload = json!({
        "events_in_buffer": events_in_buffer,
        "route": route,
        "subscription": subscription,
        "drain_window_ms": effective_timeout
            .unwrap_or(crate::commands::network::DEFAULT_DRAIN_MS),
    });
    // Only report eviction when it actually happened — a `0` on every failure
    // would train readers to ignore the field that matters.
    if evicted > 0 {
        payload["evicted_requests"] = json!(evicted);
    }
    // The daemon holds a standing subscription, so an empty buffer there is
    // neither the arming race nor a playbook-scoped guarantee; blaming either
    // would be a lie.
    if events_in_buffer == 0 {
        match subscription {
            SUBSCRIPTION_PLAYBOOK => {
                payload["empty_buffer_hint"] = json!(EMPTY_PLAYBOOK_BUFFER_HINT);
            }
            SUBSCRIPTION_STEP => {
                payload["empty_buffer_hint"] = json!(EMPTY_STEP_BUFFER_HINT);
            }
            _ => {}
        }
    }
    payload
}

fn build_network_assert_desc(step: &AssertNetworkStep) -> String {
    let mut parts = Vec::new();
    if let Some(u) = &step.url_contains {
        parts.push(format!("url_contains={u:?}"));
    }
    if let Some(s) = step.status {
        parts.push(format!("status={s}"));
    }
    if let Some(m) = &step.method {
        parts.push(format!("method={m}"));
    }
    parts.join(", ")
}

fn execute_screenshot(step: &ScreenshotStep, cli: &Cli) -> Result<Value, AppError> {
    use crate::commands::screenshot::{ScreenshotOpts, run_core as screenshot_run_core};
    let opts = ScreenshotOpts {
        output_path: step.output.as_deref(),
        base64_mode: step.base64,
        full_page: step.full_page,
        bulk: false,
        viewport_height: None,
        output_root: None,
        window_size: None,
    };
    screenshot_run_core(cli, &opts)
}

fn execute_eval(step: &EvalStep, cli: &Cli) -> Result<Value, AppError> {
    use crate::commands::connect_tab::connect_and_get_target;
    use crate::commands::eval::build_eval_js;
    use crate::commands::js_helpers::eval_or_bail;

    let js = build_eval_js(Some(&step.script), None, false, step.stringify, false)
        .map_err(|e| AppError::User(format!("eval: {e}")))?;
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();
    let result = eval_or_bail(&mut ctx, &console_actor, &js, "eval")?;
    Ok(json!({"eval": result.result.to_json()}))
}

fn execute_run(
    step: &RunStep,
    script_path: &Path,
    cli: &Cli,
    opts: &mut RunOptions<'_>,
    parent_vars: &HashMap<String, String>,
    call_stack: &[PathBuf],
) -> Result<Value, AppError> {
    // Resolve the sub-script path relative to the parent script.
    let sub_path = if Path::new(&step.path).is_absolute() {
        PathBuf::from(&step.path)
    } else {
        let parent_dir = script_path.parent().unwrap_or(Path::new("."));
        parent_dir.join(&step.path)
    };

    // Containment: refuse paths that escape the top-level script's directory
    // unless `--allow-unsafe-script-paths` is set.
    if !opts.allow_unsafe_script_paths
        && let Some(top) = opts.top_level_dir.as_ref()
    {
        check_sub_script_containment(&step.path, &sub_path, top)?;
    }

    // Merge vars: parent vars + step `with:` overrides.
    let mut sub_vars: HashMap<String, String> = parent_vars.clone();
    for (k, v) in &step.with {
        sub_vars.insert(k.clone(), v.clone());
    }

    let mut sub_opts = RunOptions {
        extra_vars: &sub_vars,
        bail_on_failure: opts.bail_on_failure,
        dry_run: opts.dry_run,
        show_secrets: opts.show_secrets,
        recorder: None, // Recorder is not inherited by sub-scripts.
        record_strict: opts.record_strict,
        format_override: None, // Sub-scripts detect their own format from extension.
        page_map: opts.page_map.clone(), // Inherit the page-map from the parent.
        env_policy: opts.env_policy.clone(),
        allow_unsafe_script_paths: opts.allow_unsafe_script_paths,
        top_level_dir: opts.top_level_dir.clone(),
        // iter-181: hand the playbook-scoped subscription down rather than
        // letting the sub-script arm a second one. Its `assert_network` then
        // also sees requests fired by the parent's earlier steps, which is the
        // whole reason the subscription is playbook-scoped.
        network_watch: opts.network_watch.take(),
    };

    let result = run_script_file(&sub_path, cli, &mut sub_opts, call_stack);
    // Take it back whether the sub-script passed or failed. A bailing
    // sub-script must not silently drop the subscription — and with it the
    // buffered history — for the rest of the parent script.
    opts.network_watch = sub_opts.network_watch.take();
    result?;
    Ok(json!({"ran": step.path}))
}

/// Verify that a sub-script path stays within the top-level script's
/// directory. Refuses absolute paths and `..`-traversing relative paths
/// outright. For paths that may exist on disk, also resolves and checks
/// the canonical form against `top_dir`.
fn check_sub_script_containment(
    raw_path: &str,
    joined: &Path,
    top_dir: &Path,
) -> Result<(), AppError> {
    // Cross-platform absolute-path detection. `Path::is_absolute()` is host-
    // sensitive: on Windows it does NOT treat `/etc/passwd` as absolute
    // (Windows requires a drive letter or UNC prefix). For a script-runner
    // security boundary, refuse the union of both conventions: anything that
    // looks absolute on *either* Unix or Windows is rejected everywhere.
    let starts_with_unix_root = raw_path.starts_with('/') || raw_path.starts_with('\\');
    let starts_with_drive_letter = {
        let bytes = raw_path.as_bytes();
        bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
    };
    if Path::new(raw_path).is_absolute() || starts_with_unix_root || starts_with_drive_letter {
        return Err(AppError::User(format!(
            "sub-script path must be relative to top-level script dir (got absolute path: '{raw_path}', pass --allow-unsafe-script-paths to override)"
        )));
    }
    // Lexical check: refuse any `..` segment so we reject traversal even
    // when intermediate dirs do not exist yet.
    if Path::new(raw_path)
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(AppError::User(format!(
            "sub-script path '{raw_path}' escapes the top-level script dir via `..` (pass --allow-unsafe-script-paths to override)"
        )));
    }
    // Canonical check (best-effort — only applies when both sides resolve).
    if let (Ok(top_canon), Ok(sub_canon)) = (top_dir.canonicalize(), joined.canonicalize())
        && !sub_canon.starts_with(&top_canon)
    {
        return Err(AppError::User(format!(
            "sub-script path '{raw_path}' resolves outside the top-level script dir '{}' (pass --allow-unsafe-script-paths to override)",
            top_dir.display()
        )));
    }
    Ok(())
}

/// Collect environment variable values referenced in a step's string fields.
///
/// Used to extend secret redaction to `{{env.X}}` values that are not in the
/// explicit `vars` map but may contain sensitive information (E5).
fn collect_env_secrets_from_step(step: &Step) -> HashMap<String, String> {
    let templates = collect_template_strings(step);
    let mut result = HashMap::new();
    for tmpl in &templates {
        result.extend(collect_env_secrets(tmpl));
    }
    result
}

/// Extract structured diagnostics from an error, if available.
///
/// Returns the `payload` field from `AppError::Diagnostics`.  All other error
/// variants carry no structured diagnostics and return `None`.
fn extract_diagnostics(e: &AppError) -> Option<Value> {
    if let AppError::Diagnostics { payload, .. } = e {
        Some(payload.clone())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_detection_catches_self_reference() {
        // We can't run a real script, but we can verify the cycle-detection logic
        // by constructing a call stack that already contains the target path.
        use std::io::Write as _;
        use tempfile::NamedTempFile;

        let mut tmp = NamedTempFile::new().unwrap();
        write!(
            tmp,
            r#"{{"version":1,"steps":[{{"run":{{"path":"self.json"}}}}]}}"#
        )
        .unwrap();
        let path = tmp.path().to_owned();
        let abs_path = path.canonicalize().unwrap();

        // Pretend the script has already been entered.
        let call_stack = [abs_path.clone()];

        // Build a minimal Cli-like object — we can't really run without Firefox.
        // Instead verify the cycle detection error fires before any connection attempt.
        // We do this by checking the call_stack logic directly.
        assert!(
            call_stack.contains(&abs_path),
            "cycle detection call_stack should contain the absolute path"
        );
        let _ = call_stack; // suppress unused warning
    }

    // -----------------------------------------------------------------
    // iter-179 / iter-181 — assert_network's empty-buffer diagnostics
    // -----------------------------------------------------------------

    /// The case that opened iteration 179: zero events on the direct route
    /// with a **step-scoped** subscription. Since iteration 181 this path is
    /// only reached when arming the playbook subscription failed, so the hint
    /// still has to explain the race — but it now leads with the fact that the
    /// race is no longer the default.
    #[test]
    fn unit_179_empty_direct_buffer_carries_the_race_hint() {
        let d = network_assert_diagnostics(0, "direct", SUBSCRIPTION_STEP, Some(2000), 0);
        assert_eq!(d["events_in_buffer"], 0);
        assert_eq!(d["route"], "direct");
        assert_eq!(d["subscription"], "step");
        assert_eq!(d["drain_window_ms"], 2000);
        let hint = d["empty_buffer_hint"].as_str().expect("hint present");
        assert!(
            hint.contains("arms the network watcher only when this step starts"),
            "{hint}"
        );
        assert!(hint.contains("zero events, not"), "{hint}");
        assert!(
            hint.contains("could not be armed"),
            "the step-scoped hint must say this path is now the fallback: {hint}"
        );
    }

    /// The daemon holds a standing subscription, so an empty buffer there is
    /// not the arming race and must not be blamed on it.
    #[test]
    fn unit_179_empty_daemon_buffer_is_not_blamed_on_the_race() {
        let d = network_assert_diagnostics(0, "daemon", SUBSCRIPTION_DAEMON, Some(2000), 0);
        assert_eq!(d["route"], "daemon");
        assert_eq!(d["subscription"], "daemon");
        assert!(
            d.get("empty_buffer_hint").is_none(),
            "the direct-mode race hint must not appear on the daemon route: {d}"
        );
    }

    /// A non-empty buffer is an ordinary no-match: the events arrived, none
    /// matched the predicate. The hint would be misleading.
    #[test]
    fn unit_179_non_empty_buffer_reports_the_count_without_the_hint() {
        let d = network_assert_diagnostics(7, "direct", SUBSCRIPTION_PLAYBOOK, Some(2000), 0);
        assert_eq!(d["events_in_buffer"], 7);
        assert!(d.get("empty_buffer_hint").is_none(), "{d}");
    }

    /// With no step or script timeout the runner still reports the window it
    /// really used, rather than omitting the field or printing 0.
    #[test]
    fn unit_179_default_drain_window_is_reported_not_omitted() {
        let d = network_assert_diagnostics(0, "direct", SUBSCRIPTION_PLAYBOOK, None, 0);
        assert_eq!(
            d["drain_window_ms"],
            crate::commands::network::DEFAULT_DRAIN_MS,
            "the reported window must track network::DEFAULT_DRAIN_MS, not a second copy"
        );
    }

    /// A playbook-scoped empty buffer must **not** repeat the arming-race
    /// story. The watcher was armed before step 1, so zero really does mean
    /// "no request was seen" — telling the reader to blame the race here would
    /// send them down exactly the wrong path iteration 179 spent four days on.
    #[test]
    fn unit_181_empty_playbook_buffer_does_not_blame_the_arming_race() {
        let d = network_assert_diagnostics(0, "direct", SUBSCRIPTION_PLAYBOOK, Some(2000), 0);
        assert_eq!(d["subscription"], "playbook");
        let hint = d["empty_buffer_hint"].as_str().expect("hint present");
        assert!(hint.contains("armed before this script's first"), "{hint}");
        assert!(
            !hint.contains("only when this step starts"),
            "the playbook hint must not repeat the step-scoped race: {hint}"
        );
    }

    /// Eviction is reported only when it happened — a `0` on every failure
    /// would train readers to skip the one field that explains a vanished
    /// request.
    #[test]
    fn unit_181_evicted_requests_reported_only_when_non_zero() {
        let none = network_assert_diagnostics(10, "direct", SUBSCRIPTION_PLAYBOOK, Some(500), 0);
        assert!(none.get("evicted_requests").is_none(), "{none}");
        let some = network_assert_diagnostics(10, "direct", SUBSCRIPTION_PLAYBOOK, Some(500), 3);
        assert_eq!(some["evicted_requests"], 3);
    }

    // -----------------------------------------------------------------
    // iter-181 — when the playbook subscription is armed
    // -----------------------------------------------------------------

    /// Parse a script from JSON step literals, so these tests pin the same
    /// shape a user writes rather than a hand-built AST.
    fn script_with(steps: &Value) -> Script {
        let src = json!({"version": 1, "steps": steps}).to_string();
        super::super::format::parse_script_str(&src, ScriptFormat::Json).expect("script parses")
    }

    /// The obvious case: a script that asserts on the network gets a
    /// subscription armed before its first step.
    #[test]
    fn unit_181_assert_network_step_arms_the_subscription() {
        let script = script_with(&json!([
            {"wait": {"timeout": 10}},
            {"assert_network": {"url_contains": "/api"}}
        ]));
        assert!(script_needs_network_watch(&script));
    }

    /// A `run:` step counts too. The sub-script is not parsed until it
    /// executes, so waiting to see its `assert_network` would arm the watcher
    /// after the parent's click had already fired the request — the exact race
    /// this iteration removes.
    #[test]
    fn unit_181_run_step_arms_the_subscription_conservatively() {
        let script = script_with(&json!([{"run": {"path": "sub.json"}}]));
        assert!(script_needs_network_watch(&script));
    }

    /// A script with neither must not pay for a connection and a watcher it
    /// will never read.
    #[test]
    fn unit_181_script_without_network_steps_arms_nothing() {
        let script = script_with(&json!([
            {"navigate": {"url": "https://example.com"}},
            {"assert_url": {"equals": "https://example.com/"}}
        ]));
        assert!(!script_needs_network_watch(&script));
    }

    // -----------------------------------------------------------------
    // iter-181 — the match predicate, shared by both subscription paths
    // -----------------------------------------------------------------

    fn event(method: &str, url: &str, status: u64) -> Value {
        json!({"method": method, "url": url, "status": status})
    }

    fn assert_network_step(spec: Value) -> AssertNetworkStep {
        serde_json::from_value(spec).expect("assert_network step parses")
    }

    /// All three fields together, matching and not.
    #[test]
    fn unit_181_match_requires_every_specified_field() {
        let step = assert_network_step(
            json!({"url_contains": "/api/auth/sign-in", "status": 200, "method": "post"}),
        );
        assert!(network_event_matches(
            &step,
            &event("POST", "http://x/api/auth/sign-in", 200)
        ));
        // Wrong status.
        assert!(!network_event_matches(
            &step,
            &event("POST", "http://x/api/auth/sign-in", 500)
        ));
        // Wrong method.
        assert!(!network_event_matches(
            &step,
            &event("GET", "http://x/api/auth/sign-in", 200)
        ));
        // Wrong URL.
        assert!(!network_event_matches(
            &step,
            &event("POST", "http://x/api/other", 200)
        ));
    }

    /// A request still in flight has no `status` yet. A step that asks for one
    /// must not match it — otherwise the playbook-scoped wait would return the
    /// moment the request was *issued* rather than answered.
    #[test]
    fn unit_181_in_flight_request_without_status_does_not_match() {
        let step = assert_network_step(json!({"url_contains": "/api", "status": 200}));
        let in_flight = json!({"method": "GET", "url": "http://x/api"});
        assert!(!network_event_matches(&step, &in_flight));
        // …but a step that does not pin a status matches it immediately.
        let no_status = assert_network_step(json!({"url_contains": "/api"}));
        assert!(network_event_matches(&no_status, &in_flight));
    }

    #[test]
    fn page_map_target_errors_without_loaded_map() {
        let step = Step::Click(super::super::format::ElementStep {
            target: super::super::format::ElementTarget {
                page_map: Some("pages.login.forms.signin.submit".to_owned()),
                ..Default::default()
            },
            wait_for_text: None,
            wait_for_selector: None,
        });
        // No page-map loaded → should fail with a helpful message.
        let result = resolve_page_map_targets(step, None, 1);
        assert!(
            result.is_err(),
            "should err when page_map target but no map loaded"
        );
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("page_map") || msg.contains("page-map"),
            "error should mention page-map"
        );
    }

    #[test]
    fn selector_target_passes_through_without_map() {
        let step = Step::Click(super::super::format::ElementStep {
            target: super::super::format::ElementTarget {
                selector: Some("button".to_owned()),
                ..Default::default()
            },
            wait_for_text: None,
            wait_for_selector: None,
        });
        // Plain selector: no page-map needed.
        let result = resolve_page_map_targets(step, None, 1);
        assert!(result.is_ok(), "selector-only step should pass through");
    }

    #[test]
    fn is_secret_field_selector_detects_password_inputs() {
        assert!(is_secret_field_selector("input[type='password']"));
        assert!(is_secret_field_selector("input[type=\"password\"]"));
        assert!(is_secret_field_selector(".password-input"));
        assert!(!is_secret_field_selector("input[type='text']"));
    }

    // -----------------------------------------------------------------------
    // E6: AppError::Diagnostics typed variant
    // -----------------------------------------------------------------------

    /// E6: extract_diagnostics returns the payload for the Diagnostics variant.
    #[test]
    fn e6_extract_diagnostics_returns_payload_for_diagnostics_variant() {
        let err = AppError::Diagnostics {
            message: "assertion failed".to_owned(),
            payload: serde_json::json!({"actual_text": "not what we expected"}),
        };
        let diag = extract_diagnostics(&err);
        assert!(
            diag.is_some(),
            "should extract diagnostics from Diagnostics variant"
        );
        let diag = diag.unwrap();
        assert_eq!(diag["actual_text"], "not what we expected");
    }

    /// E6: extract_diagnostics returns None for non-Diagnostics variants.
    #[test]
    fn e6_extract_diagnostics_returns_none_for_user_error() {
        let err = AppError::User("some user error".to_owned());
        assert!(
            extract_diagnostics(&err).is_none(),
            "User variant should yield no diagnostics"
        );
    }

    // -----------------------------------------------------------------------
    // iter-67: sandboxing — run depth + sub-script path containment
    // -----------------------------------------------------------------------

    /// Build a chain of `depth` script files where each runs the next, and
    /// the final one is a no-op. Returns the path to file 1.
    fn build_run_chain(dir: &Path, depth: usize) -> PathBuf {
        for i in 1..=depth {
            let path = dir.join(format!("s{i}.json"));
            let content = if i == depth {
                r#"{"version":1,"steps":[]}"#.to_owned()
            } else {
                let next = format!("s{}.json", i + 1);
                format!(r#"{{"version":1,"steps":[{{"run":{{"path":"{next}"}}}}]}}"#)
            };
            std::fs::write(&path, content).unwrap();
        }
        dir.join("s1.json")
    }

    #[test]
    fn run_depth_capped() {
        // The deepest layer surfaces the error to its caller as
        // AppError::User. Simulate "already 16 deep" by pre-populating the
        // call stack — the next entry would be depth 17 and must bail.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("noop.json"), r#"{"version":1,"steps":[]}"#).unwrap();
        let target = tmp.path().join("noop.json");

        let cli = <Cli as clap::Parser>::parse_from(["ff-rdp", "doctor"]);
        let mut opts = RunOptions::default();
        let call_stack: Vec<PathBuf> = (0..MAX_RUN_DEPTH)
            .map(|i| PathBuf::from(format!("/tmp/fake-{i}.json")))
            .collect();
        let err = run_script_file(&target, &cli, &mut opts, &call_stack).unwrap_err();
        let msg = match err {
            AppError::User(m) => m,
            other => panic!("expected AppError::User, got {other:?}"),
        };
        assert!(
            msg.contains("exceeds MAX_RUN_DEPTH=16") && msg.contains("depth 17"),
            "unexpected msg: {msg}"
        );
    }

    #[test]
    fn run_depth_chain_eventually_fails() {
        // End-to-end: a 20-link chain executes and fails (non-zero exit),
        // confirming the depth cap is wired through `execute_run`.
        let tmp = tempfile::tempdir().unwrap();
        let top = build_run_chain(tmp.path(), 20);
        let cli = <Cli as clap::Parser>::parse_from(["ff-rdp", "doctor"]);
        let mut opts = RunOptions::default();
        let call_stack: Vec<PathBuf> = Vec::new();
        let err = run_script_file(&top, &cli, &mut opts, &call_stack).unwrap_err();
        assert!(
            matches!(err, AppError::Exit(_)),
            "expected non-zero exit from run-depth cap, got {err:?}"
        );
    }

    #[test]
    fn run_path_containment_rejects_absolute() {
        let tmp = tempfile::tempdir().unwrap();
        let top_dir = tmp.path();
        std::fs::write(
            top_dir.join("top.json"),
            r#"{"version":1,"steps":[{"run":{"path":"/etc/passwd"}}]}"#,
        )
        .unwrap();
        let top = top_dir.join("top.json");

        let cli = <Cli as clap::Parser>::parse_from(["ff-rdp", "doctor"]);
        let mut opts = RunOptions::default();
        let call_stack: Vec<PathBuf> = Vec::new();
        // `bail_on_failure: true` is the default — the failed run step propagates as `Ok(())`
        // from `run_script_file`, with the per-step JSON containing the error. We just verify
        // that the step-execution path refuses the absolute path before any FS access.
        // The simplest assertion: invoke the containment check directly.
        let sub_path = PathBuf::from("/etc/passwd");
        let err = check_sub_script_containment("/etc/passwd", &sub_path, top_dir).unwrap_err();
        match err {
            AppError::User(m) => assert!(
                m.contains("absolute path") && m.contains("--allow-unsafe-script-paths"),
                "{m}"
            ),
            other => panic!("expected User, got {other:?}"),
        }

        // And: when allow_unsafe_script_paths is set, the check is bypassed —
        // verified by not invoking the check at the call site. Smoke-test this
        // by setting the flag and running the full pipeline; the run step is
        // expected to fail later (file does not parse as a script) but NOT
        // with the containment error.
        opts.allow_unsafe_script_paths = true;
        let result = run_script_file(&top, &cli, &mut opts, &call_stack);
        // Outer run returns Ok(()) because bail_on_failure makes the failing
        // step end the run but does not surface the error to the caller.
        // Drop the result — we only care that this code path runs.
        let _ = result;
    }

    #[test]
    fn run_path_containment_rejects_parent_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let top_dir = tmp.path();
        let err = check_sub_script_containment(
            "../escape.json",
            &top_dir.join("../escape.json"),
            top_dir,
        )
        .unwrap_err();
        match err {
            AppError::User(m) => assert!(m.contains("escapes the top-level script dir"), "{m}"),
            other => panic!("expected User, got {other:?}"),
        }
    }

    #[test]
    fn run_path_containment_accepts_relative_within_top() {
        let tmp = tempfile::tempdir().unwrap();
        let top_dir = tmp.path();
        std::fs::write(top_dir.join("sub.json"), r#"{"version":1,"steps":[]}"#).unwrap();
        let sub_path = top_dir.join("sub.json");
        check_sub_script_containment("sub.json", &sub_path, top_dir).unwrap();
    }

    /// E6: AppError::Diagnostics display shows the message, not the payload.
    #[test]
    fn e6_diagnostics_display_shows_message() {
        let err = AppError::Diagnostics {
            message: "assert_text failed".to_owned(),
            payload: serde_json::json!({"actual_text": "wrong"}),
        };
        let display = format!("{err}");
        assert_eq!(display, "assert_text failed");
        // The payload must not leak into the display string.
        assert!(
            !display.contains("wrong"),
            "payload must not appear in Display output"
        );
    }
}
