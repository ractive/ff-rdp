//! `ff-rdp record start/stop` commands.
//!
//! `record start <output.json>` begins a recording session.
//! `record stop` finalises the session and prints the output path.

use std::path::Path;

use crate::error::AppError;
use crate::script::recorder;

/// Start a new recording session.
pub fn run_start(output_path: &Path, name: Option<&str>) -> Result<(), AppError> {
    recorder::start_recording(output_path, name)
        .map_err(|e| AppError::User(format!("record start: {e}")))?;
    eprintln!("recording started: {}", output_path.display());
    Ok(())
}

/// Stop the active recording session.
pub fn run_stop() -> Result<(), AppError> {
    let out_path =
        recorder::stop_recording().map_err(|e| AppError::User(format!("record stop: {e}")))?;
    println!("{}", out_path.display());
    Ok(())
}

/// Print the recording status.
pub fn run_status() -> Result<(), AppError> {
    if let Some(state) = recorder::get_recording_status()
        .map_err(|e| AppError::User(format!("record status: {e}")))?
    {
        let json = serde_json::json!({
            "active": true,
            "output": state.output_path,
            "steps": state.step_count,
            "name": state.name,
        });
        println!("{json}");
    } else {
        let json = serde_json::json!({"active": false});
        println!("{json}");
    }
    Ok(())
}
