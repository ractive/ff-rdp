//! Recording support: append executed steps to a JSON file.
//!
//! The recorder writes one step per append call, building up a valid script
//! file incrementally.  The state is stored in a small JSON state file under
//! the XDG state directory (or `$HOME/.local/state/ff-rdp/` on platforms that
//! don't have XDG).

use std::io::Write as _;
use std::path::{Path, PathBuf};

use fs2::FileExt as _;

use super::format::Step;
use anyhow::{Context as _, bail};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Recording state file
// ---------------------------------------------------------------------------

/// State persisted to disk while a recording is active.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingState {
    /// Absolute path to the output script file.
    pub output_path: PathBuf,
    /// Number of steps recorded so far.
    pub step_count: usize,
    /// Script name (optional, written into the header).
    pub name: Option<String>,
}

/// Return the path to the recording state file.
pub fn state_file_path() -> anyhow::Result<PathBuf> {
    let base = state_dir()?;
    Ok(base.join("recording.json"))
}

fn state_dir() -> anyhow::Result<PathBuf> {
    // Use XDG_STATE_HOME if set, then dirs::state_dir(), then fallback.
    let base = if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
        PathBuf::from(xdg)
    } else if let Some(d) = dirs::state_dir() {
        d
    } else {
        dirs::home_dir()
            .context("cannot determine home directory")?
            .join(".local")
            .join("state")
    };
    Ok(base.join("ff-rdp"))
}

/// Read the current recording state, if any.
pub fn read_state() -> anyhow::Result<Option<RecordingState>> {
    let path = state_file_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let state: RecordingState =
        serde_json::from_str(&content).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(state))
}

/// Write the current recording state.
///
/// When `create_new` is true (used for `record start`), the write is atomic:
/// it fails if the state file already exists, preventing two parallel
/// `record start` invocations from racing.
pub fn write_state(state: &RecordingState) -> anyhow::Result<()> {
    write_state_inner(state, false)
}

/// Create the state file atomically (fails if already exists).
pub fn create_state(state: &RecordingState) -> anyhow::Result<()> {
    write_state_inner(state, true)
}

fn write_state_inner(state: &RecordingState, create_new: bool) -> anyhow::Result<()> {
    let path = state_file_path()?;
    let dir = path
        .parent()
        .context("state file has no parent directory")?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating state dir '{}'", dir.display()))?;
    let content = serde_json::to_string_pretty(state).context("serializing recording state")?;
    if create_new {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .with_context(|| {
                format!(
                    "a recording is already active (state file exists: '{}')",
                    path.display()
                )
            })?
            .write_all(content.as_bytes())
            .with_context(|| format!("writing {}", path.display()))
    } else {
        std::fs::write(&path, content).with_context(|| format!("writing {}", path.display()))
    }
}

/// Remove the recording state file (called on `record stop`).
pub fn clear_state() -> anyhow::Result<()> {
    let path = state_file_path()?;
    if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Output file management
// ---------------------------------------------------------------------------

/// Initialise a new script output file with a minimal header.
pub fn init_output_file(output_path: &Path, name: Option<&str>) -> anyhow::Result<()> {
    if let Some(dir) = output_path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating output dir '{}'", dir.display()))?;
    }
    // Write the script header.  Steps will be appended as individual JSON
    // objects separated by newlines; the file is opened and the steps array
    // is built incrementally using a simple text-append strategy.
    let header = build_header(name);
    std::fs::write(output_path, header)
        .with_context(|| format!("writing '{}'", output_path.display()))
}

fn build_header(name: Option<&str>) -> String {
    let name_line = if let Some(n) = name {
        format!(
            r#"  "name": {name_q},"#,
            name_q = serde_json::to_string(n).unwrap_or_default()
        )
    } else {
        String::new()
    };
    format!(
        "{{\n  \"$schema\": \"https://ff-rdp.dev/schemas/script/v1.json\",\n  \"version\": 1,\n{name_line}\n  \"steps\": [\n"
    )
}

/// Finalise the output file by closing the steps array and root object.
///
/// Idempotent: if the file already ends with the closing sequence `]\n}\n`,
/// this is a no-op.
pub fn finalise_output_file(output_path: &Path, _step_count: usize) -> anyhow::Result<()> {
    // Check if already finalised by reading the tail of the file.
    let content = std::fs::read_to_string(output_path)
        .with_context(|| format!("reading '{}' to check finalisation", output_path.display()))?;
    if content.trim_end().ends_with("]\n}") || content.ends_with("\n  ]\n}\n") {
        // Already closed — no-op.
        return Ok(());
    }

    // A newline before the closing bracket separates the last step's `}` from
    // `  ]` so the output matches what `serde_json::to_string_pretty` would
    // produce for the same document.
    let closing = "\n  ]\n}\n";
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(output_path)
        .with_context(|| format!("opening '{}' for append", output_path.display()))?;
    file.write_all(closing.as_bytes())
        .with_context(|| format!("writing footer to '{}'", output_path.display()))
}

/// Append a single step to the recording output file.
///
/// Acquires an exclusive file lock before writing to prevent interleaved bytes
/// when two CLI invocations append concurrently against the same recording.
pub fn append_step(output_path: &Path, step: &Step, step_count: usize) -> anyhow::Result<()> {
    let step_json = step_to_json(step).context("serializing step")?;
    let comma = if step_count > 0 { "," } else { "" };
    let line = format!("{comma}\n    {step_json}");

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(output_path)
        .with_context(|| format!("opening '{}' for append", output_path.display()))?;
    // SAFETY: exclusive lock ensures concurrent CLI invocations don't interleave bytes.
    file.lock_exclusive()
        .with_context(|| format!("locking '{}' for exclusive write", output_path.display()))?;
    let result = file
        .write_all(line.as_bytes())
        .with_context(|| format!("appending step to '{}'", output_path.display()));
    // Always unlock, even on write failure.
    let _ = file.unlock();
    result
}

/// Returns `true` when a CSS selector looks like a password field.
///
/// Matches common patterns: `input[type=password]`, `input[type="password"]`,
/// `input[type='password']`, or any selector containing "password" or "passwd"
/// (case-insensitive).  Users can override with `secret: false` if they record
/// a non-secret value into a password-shaped field.
fn is_password_selector(selector: &str) -> bool {
    let lower = selector.to_lowercase();
    lower.contains("password")
        || lower.contains("passwd")
        || lower.contains("[type=password]")
        || lower.contains("[type=\"password\"]")
        || lower.contains("[type='password']")
}

/// Serialise a step to a pretty-printed JSON object string indented by 4 spaces.
///
/// The output matches the hand-authored format used in `examples/scripts/`:
/// each step is a multi-line JSON object with 2-space relative indentation,
/// indented by 4 spaces within the enclosing `"steps"` array.
///
/// Applies the B2 heuristic: if recording a `type` step into a password-shaped
/// selector and `secret` is not already `true`, it is auto-elevated to `true`.
fn step_to_json(step: &Step) -> anyhow::Result<String> {
    // B2: auto-elevate `secret: true` for password-shaped selectors.
    let effective_step;
    let step = if let Step::Type(ts) = step
        && !ts.secret
        && ts
            .target
            .selector
            .as_deref()
            .is_some_and(is_password_selector)
    {
        effective_step = Step::Type(super::format::TypeStep {
            secret: true,
            ..ts.clone()
        });
        &effective_step
    } else {
        step
    };

    let obj = serde_json::to_value(step).context("step to value")?;
    // B3: pretty-print with 2-space indent, then indent every line by 4 spaces
    // so the step body aligns with the surrounding `"steps": [` array in the file.
    let pretty = serde_json::to_string_pretty(&obj).context("step to pretty string")?;
    let indented = pretty
        .lines()
        .enumerate()
        .map(|(i, line)| {
            if i == 0 {
                // The first line is placed after the `    ` prefix already added by append_step.
                line.to_owned()
            } else {
                format!("    {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(indented)
}

// ---------------------------------------------------------------------------
// In-process recorder for `run --record`
// ---------------------------------------------------------------------------

/// A file-based step recorder used during `ff-rdp run --record`.
pub struct FileRecorder {
    output_path: PathBuf,
    step_count: usize,
}

impl FileRecorder {
    /// Create a new recorder, initialising the output file.
    pub fn new(output_path: &Path, name: Option<&str>) -> anyhow::Result<Self> {
        init_output_file(output_path, name)?;
        Ok(Self {
            output_path: output_path.to_owned(),
            step_count: 0,
        })
    }

    /// Record one step.
    pub fn record(&mut self, step: &Step) -> anyhow::Result<()> {
        append_step(&self.output_path, step, self.step_count)?;
        self.step_count += 1;
        Ok(())
    }

    /// Finalise the output file and return its path.
    pub fn finish(self) -> anyhow::Result<PathBuf> {
        finalise_output_file(&self.output_path, self.step_count)?;
        Ok(self.output_path)
    }
}

// ---------------------------------------------------------------------------
// Record start/stop commands
// ---------------------------------------------------------------------------

/// Start a new recording session.  Errors if one is already active.
pub fn start_recording(output_path: &Path, name: Option<&str>) -> anyhow::Result<()> {
    if let Some(existing) = read_state()? {
        bail!(
            "a recording is already active — output: '{}'. \
             Stop it first with `ff-rdp record stop`.",
            existing.output_path.display()
        );
    }
    // Resolve to an absolute path so the state file is always portable.
    let filename = output_path
        .file_name()
        .context("output path must have a filename component")?;
    let abs_path = output_path
        .canonicalize()
        .or_else(|_| -> anyhow::Result<PathBuf> {
            // File doesn't exist yet — resolve the parent and append the filename.
            let parent = output_path.parent().unwrap_or(Path::new("."));
            let abs_parent = if parent.as_os_str().is_empty() {
                std::env::current_dir().context("current_dir")?
            } else {
                parent
                    .canonicalize()
                    .with_context(|| format!("resolving parent of '{}'", output_path.display()))?
            };
            Ok(abs_parent.join(filename))
        })
        .with_context(|| format!("resolving path '{}'", output_path.display()))?;

    init_output_file(&abs_path, name)?;

    let state = RecordingState {
        output_path: abs_path,
        step_count: 0,
        name: name.map(str::to_owned),
    };
    // Use atomic create_new to prevent two parallel `record start` invocations
    // from both succeeding when they race.
    create_state(&state)
}

/// Stop the active recording session and finalise the output file.
///
/// Returns the path to the output file.
pub fn stop_recording() -> anyhow::Result<PathBuf> {
    let state = read_state()?.with_context(
        || "no active recording — start one with `ff-rdp record start <output.json>`",
    )?;
    finalise_output_file(&state.output_path, state.step_count)?;
    clear_state()?;
    Ok(state.output_path)
}

/// Return the current recording state, or None if no recording is active.
pub fn get_recording_status() -> anyhow::Result<Option<RecordingState>> {
    read_state()
}

/// Append a step to the active recording (called from dispatch after a
/// successful command when `record start` has been invoked externally).
pub fn record_step_to_active(step: &Step) -> anyhow::Result<()> {
    let mut state = read_state()?.with_context(|| "record_step called but no active recording")?;
    append_step(&state.output_path, step, state.step_count)?;
    state.step_count += 1;
    write_state(&state)
}

/// If a recording is active, record a step for the given command.
/// Errors are logged to stderr but do not fail the command.
/// If no recording is active, this is a no-op (no warning emitted).
///
/// Called from dispatch after a command succeeds.
pub fn record_step_if_active(step: &Step) {
    // Check if a recording is active before attempting to write.
    let is_active = match read_state() {
        Err(e) => {
            eprintln!("warning: could not read recording state: {e}");
            return;
        }
        Ok(None) => return,
        Ok(Some(_)) => true,
    };
    debug_assert!(is_active);
    match record_step_to_active(step) {
        Ok(()) => {}
        Err(e) => eprintln!("warning: recording step failed: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::format::{ElementTarget, NavigateStep, Step, TypeStep};
    use tempfile::NamedTempFile;

    #[test]
    fn file_recorder_produces_valid_json() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_owned();
        // Drop to allow FileRecorder to write.
        drop(tmp);

        let step = Step::Navigate(NavigateStep {
            url: "https://example.com".to_owned(),
            wait_text: None,
            wait_selector: None,
        });

        let mut recorder = FileRecorder::new(&path, Some("test")).unwrap();
        recorder.record(&step).unwrap();
        let final_path = recorder.finish().unwrap();

        let content = std::fs::read_to_string(&final_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["version"], 1);
        assert!(parsed["steps"].is_array());
        assert_eq!(parsed["steps"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn file_recorder_empty_script_is_valid_json() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_owned();
        drop(tmp);

        let recorder = FileRecorder::new(&path, None).unwrap();
        let final_path = recorder.finish().unwrap();

        let content = std::fs::read_to_string(&final_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(parsed["steps"].as_array().unwrap().is_empty());
    }

    /// B2: Recording a type step into a password selector auto-sets secret: true.
    #[test]
    fn b2_password_selector_auto_secret() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_owned();
        drop(tmp);

        let step = Step::Type(TypeStep {
            target: ElementTarget {
                selector: Some("input[type=password]".to_owned()),
                ..Default::default()
            },
            text: "hunter2".to_owned(),
            clear: false,
            secret: false, // not explicitly set — recorder should elevate it
        });

        let mut recorder = FileRecorder::new(&path, None).unwrap();
        recorder.record(&step).unwrap();
        let final_path = recorder.finish().unwrap();

        let content = std::fs::read_to_string(&final_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let recorded_step = &parsed["steps"][0];
        assert_eq!(
            recorded_step["type"]["secret"],
            serde_json::json!(true),
            "password selector should auto-elevate secret to true: {recorded_step}"
        );
    }

    /// B2: is_password_selector heuristic coverage.
    #[test]
    fn b2_password_selector_heuristics() {
        assert!(is_password_selector("input[type=password]"));
        assert!(is_password_selector("input[type=\"password\"]"));
        assert!(is_password_selector("input[type='password']"));
        assert!(is_password_selector(".password-field"));
        assert!(is_password_selector("#passwd"));
        assert!(!is_password_selector("input[type=text]"));
        assert!(!is_password_selector("#username"));
    }

    /// B1: Recording a wait step with a non-default timeout records the timeout field.
    #[test]
    fn b1_wait_step_records_timeout() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_owned();
        drop(tmp);

        let step = Step::Wait(super::super::format::WaitStep {
            selector: Some(".result".to_owned()),
            text: None,
            eval: None,
            timeout: Some(10_000),
        });

        let mut recorder = FileRecorder::new(&path, None).unwrap();
        recorder.record(&step).unwrap();
        let final_path = recorder.finish().unwrap();

        let content = std::fs::read_to_string(&final_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let recorded_step = &parsed["steps"][0];
        assert_eq!(
            recorded_step["wait"]["timeout"],
            serde_json::json!(10_000),
            "wait step should record non-default timeout: {recorded_step}"
        );
        assert_eq!(
            recorded_step["wait"]["selector"],
            serde_json::json!(".result"),
        );
    }

    /// B1: Recording a click step with wait_for_text records the field.
    #[test]
    fn b1_click_step_records_wait_for_text() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_owned();
        drop(tmp);

        let step = Step::Click(super::super::format::ElementStep {
            target: ElementTarget {
                selector: Some("button#submit".to_owned()),
                ..Default::default()
            },
            wait_for_text: Some("Welcome".to_owned()),
            wait_for_selector: None,
        });

        let mut recorder = FileRecorder::new(&path, None).unwrap();
        recorder.record(&step).unwrap();
        let final_path = recorder.finish().unwrap();

        let content = std::fs::read_to_string(&final_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let recorded_step = &parsed["steps"][0];
        assert_eq!(
            recorded_step["click"]["wait_for_text"],
            serde_json::json!("Welcome"),
            "click step should record wait_for_text: {recorded_step}"
        );
    }

    /// B3: Recorded steps use pretty-printed JSON with 2-space indent, indented 4 spaces
    /// within the enclosing steps array — matching the hand-authored script format.
    #[test]
    fn b3_recorded_output_is_pretty_printed() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_owned();
        drop(tmp);

        let steps = vec![
            Step::Navigate(NavigateStep {
                url: "https://example.com".to_owned(),
                wait_text: None,
                wait_selector: None,
            }),
            Step::Type(TypeStep {
                target: ElementTarget {
                    selector: Some("#email".to_owned()),
                    ..Default::default()
                },
                text: "user@example.com".to_owned(),
                clear: true,
                secret: false,
            }),
            Step::Wait(super::super::format::WaitStep {
                selector: Some(".dashboard".to_owned()),
                text: None,
                eval: None,
                timeout: Some(8_000),
            }),
        ];

        let mut recorder = FileRecorder::new(&path, None).unwrap();
        for step in &steps {
            recorder.record(step).unwrap();
        }
        let final_path = recorder.finish().unwrap();

        let content = std::fs::read_to_string(&final_path).unwrap();

        // Must be valid JSON.
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["steps"].as_array().unwrap().len(), 3);

        // B3: Each step entry must span multiple lines (pretty-printed, not compact).
        // The navigate step should have at least: `{`, `  "navigate": {`, `    "url": ...`, `  }`, `}`.
        assert!(
            content.contains("  \"navigate\": {"),
            "navigate step should be pretty-printed with nested braces:\n{content}"
        );
        assert!(
            content.contains("    \"url\":"),
            "navigate url should be indented 4 spaces within step:\n{content}"
        );

        // B3: Steps must be indented 4 spaces from the file margin.
        let step_lines: Vec<&str> = content
            .lines()
            .filter(|l| l.trim_start().starts_with('{') && l.starts_with("    {"))
            .collect();
        assert!(
            !step_lines.is_empty(),
            "step opening braces should be indented 4 spaces:\n{content}"
        );
    }

    /// C1: A 2-step recorded file must end with `}\n  ]\n}\n` — the same
    /// suffix that `serde_json::to_string_pretty` would produce for the same
    /// document structure.
    #[test]
    fn c1_two_step_recording_ends_with_correct_closing_sequence() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_owned();
        drop(tmp);

        let steps = vec![
            Step::Navigate(NavigateStep {
                url: "https://example.com".to_owned(),
                wait_text: None,
                wait_selector: None,
            }),
            Step::Navigate(NavigateStep {
                url: "https://example.org".to_owned(),
                wait_text: None,
                wait_selector: None,
            }),
        ];

        let mut recorder = FileRecorder::new(&path, None).unwrap();
        for step in &steps {
            recorder.record(step).unwrap();
        }
        let final_path = recorder.finish().unwrap();

        let content = std::fs::read_to_string(&final_path).unwrap();

        // The file must end with a newline before the closing bracket,
        // then `  ]`, then `\n}\n`.
        assert!(
            content.ends_with("}\n  ]\n}\n"),
            "2-step recording must end with `}}\\n  ]\\n}}\\n`; actual tail: {:?}",
            &content[content.len().saturating_sub(30)..]
        );

        // Must still be valid JSON.
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["steps"].as_array().unwrap().len(), 2);
    }
}
