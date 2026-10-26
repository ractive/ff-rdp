//! `ff-rdp stop` — stop a Firefox that ff-rdp launched and remove its
//! temporary profile.
//!
//! # Safety
//!
//! ff-rdp runs with the caller's rights, so the threat is not privilege
//! escalation but ff-rdp being steered — by an agent, a stale file, a recycled
//! PID or a planted marker — into signalling a process it did not start.
//! Every rule fails closed:
//!
//! 1. **No PID input.** `stop` takes a port or `--all`; the PID always comes
//!    from the OS (the port's listener, or a marker under the profile root)
//!    and must be confirmed by an ff-rdp owner marker.
//! 2. **Ownership requires the start token**, recorded and matching.
//! 3. **Trusted roots only.**
//! 4. **It must be Firefox** (executable name, from the OS).
//!
//!    Rules 2–4 are [`profile_dir::authorize_kill`].
//! 5. **Process-group kill only for a group leader** (`getpgid(pid) == pid`),
//!    enforced in `util::process`.
//! 6. **Re-verify before every signal** — [`replace::stop_owned_firefox`].
//! 7. **Refusal is an error**, naming the PID and process. Under `--all`,
//!    unowned candidates never appear at all.

use std::time::Duration;

use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::commands::launch::replace;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;
use crate::util::profile_dir::{self, OwnedFirefox, ProfileCleanup};

/// How long to wait for the process to exit once its port is free, before
/// the profile is removed. A Firefox whose port closed is shutting down; its
/// profile must not be deleted out from under a process still writing it.
const EXIT_AFTER_RELEASE_WAIT: Duration = Duration::from_secs(3);

/// `ff-rdp stop [--all] [--dry-run]`.
pub fn run(cli: &Cli, all: bool, dry_run: bool) -> Result<(), AppError> {
    if all {
        run_all(cli, dry_run)
    } else {
        run_port(cli, dry_run)
    }
}

/// `ff-rdp stop` on `--port`.
fn run_port(cli: &Cli, dry_run: bool) -> Result<(), AppError> {
    if !crate::connection_meta::is_loopback(&cli.host) {
        return Err(AppError::User(format!(
            "`ff-rdp stop` only stops a Firefox on this machine; --host {} is not a loopback \
             address",
            cli.host
        )));
    }
    let port = cli.port;
    let listener = match crate::port_owner::find_listener(port) {
        Ok(Some(listener)) => listener,
        Ok(None) | Err(_) if !crate::port_owner::is_port_in_use(port) => {
            let results = json!({
                "stopped": false,
                "pid": Value::Null,
                "port": port,
                "profile": Value::Null,
                "profile_removed": false,
                "dry_run": dry_run,
                "reason": format!("nothing is listening on port {port}"),
            });
            return emit(cli, &results, 1);
        }
        Ok(None) | Err(_) => {
            return Err(AppError::User(format!(
                "port {port} is in use, but its owner could not be identified, so ff-rdp will \
                 not stop it. Run `ff-rdp doctor` or `lsof -i :{port}` to investigate."
            )));
        }
    };
    let owned = profile_dir::authorize_kill(listener.pid).map_err(|refusal| {
        replace::refusal_error(
            port,
            &listener.process_name,
            listener.pid,
            &refusal,
            "`ff-rdp stop` only stops a Firefox ff-rdp launched; stop it yourself if you mean to.",
        )
    })?;

    if dry_run {
        let results = json!({
            "stopped": false,
            "would_stop": true,
            "pid": owned.pid,
            "port": port,
            "profile": owned.profile.display().to_string(),
            "profile_removed": false,
            "dry_run": true,
        });
        return emit(cli, &results, 1);
    }

    let report = replace::stop_owned_firefox(&owned, Some(port));
    if !report.released {
        return Err(AppError::User(report.message));
    }
    let profile_removed = remove_profile_once_exited(&owned, report.exited);
    let results = json!({
        "stopped": true,
        "pid": owned.pid,
        "port": port,
        "profile": owned.profile.display().to_string(),
        "profile_removed": profile_removed,
        "dry_run": false,
    });
    emit(cli, &results, 1)
}

/// `ff-rdp stop --all`: every live Firefox ff-rdp launched under the
/// configured profile root.
fn run_all(cli: &Cli, dry_run: bool) -> Result<(), AppError> {
    let instances = profile_dir::owned_instances();
    let mut any_failed = false;
    let entries: Vec<Value> = instances
        .iter()
        .map(|owned| {
            if dry_run {
                return json!({
                    "stopped": false,
                    "would_stop": true,
                    "pid": owned.pid,
                    "profile": owned.profile.display().to_string(),
                    "profile_removed": false,
                });
            }
            let report = replace::stop_owned_firefox(owned, None);
            if !report.released {
                any_failed = true;
                return json!({
                    "stopped": false,
                    "pid": owned.pid,
                    "profile": owned.profile.display().to_string(),
                    "profile_removed": false,
                    "error": report.message,
                });
            }
            json!({
                "stopped": true,
                "pid": owned.pid,
                "profile": owned.profile.display().to_string(),
                "profile_removed": remove_profile_once_exited(owned, report.exited),
            })
        })
        .collect();
    let total = entries.len();
    let envelope = output::envelope(
        &Value::Array(entries),
        total,
        &json!({ "dry_run": dry_run }),
    );
    OutputPipeline::from_cli(cli)?.finalize(&envelope)?;
    if any_failed {
        Err(AppError::Exit(1))
    } else {
        Ok(())
    }
}

/// Remove the stopped instance's managed profile — only once the process is
/// gone, never while it might still be writing into it.
fn remove_profile_once_exited(owned: &OwnedFirefox, exited: bool) -> bool {
    let exited = exited || {
        let deadline = std::time::Instant::now() + EXIT_AFTER_RELEASE_WAIT;
        loop {
            if !crate::util::process::is_process_alive(owned.pid) {
                break true;
            }
            if std::time::Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    if !exited {
        tracing::warn!(
            "stop: Firefox pid {} released its port but has not exited; keeping {}",
            owned.pid,
            owned.profile.display()
        );
        return false;
    }
    match profile_dir::remove_stopped_profile(owned) {
        ProfileCleanup::Removed(_) => true,
        ProfileCleanup::Skipped(reason) => {
            tracing::warn!(
                "stop: did not remove {} ({})",
                owned.profile.display(),
                reason.as_str()
            );
            false
        }
    }
}

fn emit(cli: &Cli, results: &Value, total: usize) -> Result<(), AppError> {
    let envelope = output::envelope(results, total, &json!({}));
    OutputPipeline::from_cli(cli)?.finalize(&envelope)
}
