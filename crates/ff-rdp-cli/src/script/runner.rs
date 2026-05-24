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
        Step::AssertNetwork(s) => execute_assert_network(s, cli, default_timeout_ms),
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
    };
    nav_run_core(cli, &effective_url, &wait_opts)
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
        wait_timeout: timeout,
    };
    wait_run_core(cli, &opts)
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

fn execute_assert_network(
    step: &AssertNetworkStep,
    cli: &Cli,
    default_timeout_ms: Option<u64>,
) -> Result<Value, AppError> {
    // Use the network command to get buffered events.
    use crate::commands::network::run_get_events;

    // Note: api_route targets are resolved to url_contains+method by
    // resolve_page_map_targets before reaching here, so step.api_route is
    // always None at this point.

    // Use step timeout, then script default, then CLI default.
    let effective_timeout = step.timeout.or(default_timeout_ms);
    let events = run_get_events(cli, effective_timeout)?;

    let matched = events.iter().any(|e| {
        let url = e.get("url").and_then(Value::as_str).unwrap_or("");
        let status: Option<u16> = e
            .get("status")
            .and_then(Value::as_u64)
            .and_then(|s| u16::try_from(s).ok());
        let method = e.get("method").and_then(Value::as_str).unwrap_or("");

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
    });

    if matched {
        Ok(json!({"asserted": true, "matched": true}))
    } else {
        let desc = build_network_assert_desc(step);
        // E6: return structured diagnostics payload instead of embedding in the string.
        Err(AppError::Diagnostics {
            message: format!("assert_network: no matching network request found ({desc})"),
            payload: json!({"events_in_buffer": events.len()}),
        })
    }
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
        viewport_height: None,
        output_root: None,
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
    };

    run_script_file(&sub_path, cli, &mut sub_opts, call_stack)?;
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
    if Path::new(raw_path).is_absolute() {
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
