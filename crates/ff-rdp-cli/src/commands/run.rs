//! `ff-rdp run <script>` command.
//!
//! Executes a JSON/YAML script file, emitting one NDJSON line per step.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::script::recorder::FileRecorder;
use crate::script::runner::{RunOptions, run_script_file};

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
