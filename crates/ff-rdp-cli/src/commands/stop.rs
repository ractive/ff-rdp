//! `ff-rdp stop` — stop a Firefox that ff-rdp launched and remove its
//! temporary profile. A Firefox launched with a user `--profile` is proved
//! through its ownership record (`profile_dir::create_ownership_record`);
//! `stop` removes that record and keeps the user's directory.
//!
//! # Safety
//!
//! ff-rdp runs with the caller's rights, so the threat is not privilege
//! escalation but ff-rdp being steered — by an agent, a stale file, a recycled
//! PID or a marker planted by **another account** (rule 3) — into signalling a
//! process it did not start. Every rule fails closed:
//!
//! Out of scope: a process running as the **same user**. It can write a
//! marker plus a start token naming the user's own desktop Firefox into the
//! trusted root, and `stop --all` would then kill it — but such a process can
//! already `kill` that Firefox directly, so the gate grants it nothing new.
//! Binding ownership to the live process's command line (`-profile <dir>`)
//! and signalling through a pidfd/process handle would close this and the
//! reverify→signal window; see the `#344 follow-up` line in `kb/backlog.md`.
//!
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
    // Both forms only ever act on this machine; a remote --host is a request
    // ff-rdp cannot honour, so it is refused rather than ignored.
    if !crate::connection_meta::is_loopback(&cli.host) {
        return Err(AppError::User(format!(
            "`ff-rdp stop` only stops a Firefox on this machine; --host {} is not a loopback \
             address",
            cli.host
        )));
    }
    if all {
        run_all(cli, dry_run)
    } else {
        run_port(cli, dry_run)
    }
}

/// `ff-rdp stop` on `--port`.
fn run_port(cli: &Cli, dry_run: bool) -> Result<(), AppError> {
    let port = cli.port;
    let listener = match crate::port_owner::find_listener(port) {
        Ok(Some(listener)) => listener,
        Ok(None) if !crate::port_owner::is_port_in_use(port) => {
            return emit(cli, &not_stopped(port, dry_run, None), 1);
        }
        Err(lookup_error) if !crate::port_owner::is_port_in_use(port) => {
            return emit(cli, &not_stopped(port, dry_run, Some(&lookup_error)), 1);
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
            "profile": owned.browser_profile().display().to_string(),
            "profile_removed": false,
            "profile_would_be_removed": profile_dir::profile_would_be_removed(&owned),
            "dry_run": true,
        });
        return emit(cli, &results, 1);
    }

    let report = replace::stop_owned_firefox(&owned, Some(port));
    let profile_removed = finish_stop(&owned, &report).map_err(AppError::User)?;
    let results = json!({
        "stopped": true,
        "pid": owned.pid,
        "port": port,
        "profile": owned.browser_profile().display().to_string(),
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
                    "profile": owned.browser_profile().display().to_string(),
                    "profile_removed": false,
                    "profile_would_be_removed": profile_dir::profile_would_be_removed(owned),
                });
            }
            let report = replace::stop_owned_firefox(owned, None);
            match finish_stop(owned, &report) {
                Ok(profile_removed) => json!({
                    "stopped": true,
                    "pid": owned.pid,
                    "profile": owned.browser_profile().display().to_string(),
                    "profile_removed": profile_removed,
                }),
                Err(error) => {
                    any_failed = true;
                    json!({
                        "stopped": false,
                        "pid": owned.pid,
                        "profile": owned.browser_profile().display().to_string(),
                        "profile_removed": false,
                        "error": error,
                    })
                }
            }
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

/// Turn a ladder report into the outcome `stop` reports: `Ok(profile_removed)`
/// only once the process has actually exited, `Err(message)` otherwise — a
/// released port alone is never reported as `stopped: true`. The profile is
/// removed only after exit, never while Firefox might still be writing it.
fn finish_stop(owned: &OwnedFirefox, report: &replace::StopReport) -> Result<bool, String> {
    if !report.released {
        return Err(report.message.clone());
    }
    let exited = report.exited
        || replace::wait_for_exit(
            crate::util::process::is_process_alive,
            owned.pid,
            EXIT_AFTER_RELEASE_WAIT,
        );
    if !exited {
        return Err(format!(
            "Firefox (pid {}) released its port but is still running after SIGTERM+SIGKILL; \
             its profile {} was kept. Check it with `ps -p {}`.",
            owned.pid,
            owned.browser_profile().display(),
            owned.pid
        ));
    }
    Ok(remove_stopped_profile(owned))
}

/// Remove an exited instance's managed profile. For a Firefox launched with a
/// user `--profile` only its ownership record goes; the user's directory is
/// kept, so the answer is always `false`.
fn remove_stopped_profile(owned: &OwnedFirefox) -> bool {
    match profile_dir::remove_stopped_profile(owned) {
        ProfileCleanup::Removed(_) => owned.user_profile().is_none(),
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

/// The `stopped: false` result for a port nothing listens on (the connect
/// probe found it free). `lookup_error` is set when the port-owner lookup
/// itself failed: the port is still free, so there is nothing to stop and the
/// exit stays 0, but the reason says the owner could not be checked rather
/// than claiming a clean "nothing is listening".
fn not_stopped(port: u16, dry_run: bool, lookup_error: Option<&str>) -> Value {
    let reason = match lookup_error {
        None => format!("nothing is listening on port {port}"),
        Some(error) => format!(
            "the owner of port {port} could not be checked ({error}); the port accepts no \
             connections, so nothing was stopped. Run `ff-rdp doctor` to investigate."
        ),
    };
    json!({
        "stopped": false,
        "pid": Value::Null,
        "port": port,
        "profile": Value::Null,
        "profile_removed": false,
        "dry_run": dry_run,
        "reason": reason,
    })
}

fn emit(cli: &Cli, results: &Value, total: usize) -> Result<(), AppError> {
    let envelope = output::envelope(results, total, &json!({}));
    OutputPipeline::from_cli(cli)?.finalize(&envelope)
}

#[cfg(test)]
mod tests {
    use super::not_stopped;

    #[test]
    fn free_port_with_no_listener_says_nothing_is_listening() {
        let v = not_stopped(6123, false, None);
        assert_eq!(v["stopped"], false);
        assert_eq!(v["reason"], "nothing is listening on port 6123");
    }

    #[test]
    fn failed_owner_lookup_says_unchecked_and_points_at_doctor() {
        let v = not_stopped(6123, true, Some("lsof not found"));
        assert_eq!(v["stopped"], false);
        assert_eq!(v["dry_run"], true);
        let reason = v["reason"].as_str().expect("reason");
        assert!(!reason.contains("nothing is listening"), "{reason}");
        assert!(reason.contains("could not be checked"), "{reason}");
        assert!(reason.contains("lsof not found"), "{reason}");
        assert!(reason.contains("ff-rdp doctor"), "{reason}");
    }
}
