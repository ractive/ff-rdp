//! `ff-rdp run <script>` command.
//!
//! Executes a JSON/YAML script file, emitting one NDJSON line per step.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::script::recorder::FileRecorder;
use crate::script::runner::{RunOptions, run_script_file};
use crate::script::vars::EnvPolicy;

/// Options parsed from CLI flags for the `run` subcommand.
pub struct RunCommandOpts<'a> {
    pub script_path: &'a Path,
    pub extra_vars: HashMap<String, String>,
    pub bail_on_failure: bool,
    pub dry_run: bool,
    pub show_secrets: bool,
    pub record_output: Option<&'a Path>,
    /// Fail the entire run if a recording step fails.
    pub record_strict: bool,
    pub format_override: Option<&'a str>,
    /// Explicit page-map path (or None to try the default .ffrdp/page-map.json).
    pub page_map_path: Option<&'a Path>,
    /// Env var names the user has opted into for `{{env.X}}` resolution.
    pub allow_env: Vec<String>,
    /// Allow sub-script `run:` paths to escape the top-level script's dir.
    pub allow_unsafe_script_paths: bool,
}

/// Return `true` if any step in the script references a page-map target
/// (`page_map:`, `field:`, or `api_route:`).
fn script_needs_page_map(script: &crate::script::format::Script) -> bool {
    use crate::script::format::Step;
    script.steps.iter().any(|step| match step {
        Step::Click(s) => s.target.page_map.is_some() || s.target.field.is_some(),
        Step::Type(s) => s.target.page_map.is_some() || s.target.field.is_some(),
        Step::AssertNetwork(s) => s.api_route.is_some(),
        _ => false,
    })
}

pub fn run(cli: &Cli, opts: &RunCommandOpts<'_>) -> Result<(), AppError> {
    let fmt_override = opts
        .format_override
        .and_then(crate::script::format::ScriptFormat::from_str_hint);

    // Validate the format override early so callers get a clear error.
    if let Some(raw) = opts.format_override
        && fmt_override.is_none()
    {
        return Err(AppError::User(format!(
            "--script-format must be 'json', 'yaml', or 'yml', got: {raw:?}"
        )));
    }

    // Load page-map: explicit path wins; fall back to .ffrdp/page-map.json only
    // when the script actually references page-map selectors.  If the default
    // file is missing and the script doesn't need it, skip silently.
    let page_map = if let Some(path) = opts.page_map_path {
        Some(
            crate::page_map::PageMap::load(path)
                .map_err(|e| AppError::User(format!("loading page-map: {e}")))?,
        )
    } else {
        // Parse the script first to check if it needs a page-map.
        // We do a lightweight parse here; run_script_file will parse again.
        let script_fmt = crate::script::format::ScriptFormat::from_path(opts.script_path);
        let script_content = std::fs::read_to_string(opts.script_path)
            .map_err(|e| AppError::User(format!("reading script: {e}")))?;
        let script = crate::script::format::parse_script_str(&script_content, script_fmt)
            .map_err(|e| AppError::User(format!("parsing script: {e:#}")))?;

        if script_needs_page_map(&script) {
            crate::page_map::PageMap::load_default()
                .map_err(|e| AppError::User(format!("loading default page-map: {e}")))?
        } else {
            None
        }
    };

    let recorder = if let Some(out_path) = opts.record_output {
        let name = opts.script_path.file_stem().and_then(|s| s.to_str());
        Some(
            FileRecorder::new(out_path, name)
                .map_err(|e| AppError::User(format!("creating recorder: {e}")))?,
        )
    } else {
        None
    };

    let mut run_opts = RunOptions {
        extra_vars: &opts.extra_vars,
        bail_on_failure: opts.bail_on_failure,
        dry_run: opts.dry_run,
        show_secrets: opts.show_secrets,
        recorder,
        record_strict: opts.record_strict,
        format_override: fmt_override,
        page_map,
        env_policy: EnvPolicy::from_names(opts.allow_env.iter().cloned()),
        allow_unsafe_script_paths: opts.allow_unsafe_script_paths,
        top_level_dir: None,
    };

    let call_stack: Vec<PathBuf> = Vec::new();
    let run_result = run_script_file(opts.script_path, cli, &mut run_opts, &call_stack);

    // Always finalise the recorder, even if the run failed.
    // This ensures the output file is valid JSON with the steps array closed.
    if let Some(rec) = run_opts.recorder.take() {
        match rec.finish() {
            Ok(out_path) => eprintln!("recording saved to: {}", out_path.display()),
            Err(e) => eprintln!("warning: failed to finalise recording: {e}"),
        }
    }

    run_result
}
