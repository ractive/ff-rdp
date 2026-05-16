//! Recording support: append executed steps to a JSON file.
//!
//! The recorder writes one step per append call, building up a valid script
//! file incrementally.  The state is stored in a small JSON state file under
//! the XDG state directory (or `$HOME/.local/state/ff-rdp/` on platforms that
//! don't have XDG).

use std::io::Write as _;
use std::path::{Path, PathBuf};

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
pub fn write_state(state: &RecordingState) -> anyhow::Result<()> {
    let path = state_file_path()?;
    let dir = path
        .parent()
        .context("state file has no parent directory")?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating state dir '{}'", dir.display()))?;
    let content = serde_json::to_string_pretty(state).context("serializing recording state")?;
    std::fs::write(&path, content).with_context(|| format!("writing {}", path.display()))
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
pub fn finalise_output_file(output_path: &Path, _step_count: usize) -> anyhow::Result<()> {
    let closing = "  ]\n}\n";
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(output_path)
        .with_context(|| format!("opening '{}' for append", output_path.display()))?;
    file.write_all(closing.as_bytes())
        .with_context(|| format!("writing footer to '{}'", output_path.display()))
}

/// Append a single step to the recording output file.
pub fn append_step(output_path: &Path, step: &Step, step_count: usize) -> anyhow::Result<()> {
    let step_json = step_to_json(step).context("serializing step")?;
    let comma = if step_count > 0 { "," } else { "" };
    let line = format!("{comma}\n    {step_json}");

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(output_path)
        .with_context(|| format!("opening '{}' for append", output_path.display()))?;
    file.write_all(line.as_bytes())
        .with_context(|| format!("appending step to '{}'", output_path.display()))
}

/// Serialise a step to a compact JSON object string.
fn step_to_json(step: &Step) -> anyhow::Result<String> {
    let obj = serde_json::to_value(step).context("step to value")?;
    serde_json::to_string(&obj).context("step to string")
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
            Ok(abs_parent.join(output_path.file_name().unwrap_or_default()))
        })
        .with_context(|| format!("resolving path '{}'", output_path.display()))?;

    init_output_file(&abs_path, name)?;

    let state = RecordingState {
        output_path: abs_path,
        step_count: 0,
        name: name.map(str::to_owned),
    };
    write_state(&state)
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

/// Append a step to the active recording (called from the runner when
/// `record start` has been invoked externally).
#[allow(dead_code)]
pub fn record_step_to_active(step: &Step) -> anyhow::Result<()> {
    let mut state = read_state()?.with_context(|| "record_step called but no active recording")?;
    append_step(&state.output_path, step, state.step_count)?;
    state.step_count += 1;
    write_state(&state)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::format::{NavigateStep, Step};
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
}
