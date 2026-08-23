//! Live tests for iteration 175 — a launch that fails after creating its
//! profile directory must not leave that directory behind.
//!
//! See `kb/iterations/iteration-175-failed-launch-leaks-unmarked-profile-dir.md`.
//!
//! `build_command` creates the managed profile directory and writes its
//! `user.js` **before** Firefox is spawned. Every failure between those two
//! points used to `return Err` straight past the directory: it stayed on disk
//! with (pre-iter-171) no owner marker at all, fell through to the iter-96
//! mtime heuristic, and survived the default seven-day gate. Twenty such
//! directories were found under the real profile root by iteration 171, and
//! eight more by iteration 175.
//!
//! The unit tests in `commands/launch.rs` cover the individual error paths with
//! stubbed hooks. This file covers what they cannot: a *real* Firefox process,
//! a real profile root, and the real `--launch-timeout` failure path — which on
//! `main` leaves a real directory behind.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_175 -- --include-ignored

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::common::{LiveFirefox, ff_rdp_bin, ff_rdp_launch_command, live_tests_enabled, pid_alive};

/// Duplicated from `src/util/profile_dir.rs` — this crate ships no `[lib]`
/// target for an integration test to import the constant from, which is why
/// `live_96`, `live_151` and `live_168` all carry their own copy too.
const OWNER_PID_MARKER: &str = ".ff-rdp-owner-pid";

/// Ask the CLI itself where the managed profile root is, rather than
/// re-deriving the platform rules here (dogfooding `profiles list`, and the
/// only way a test can be sure it is looking at the same directory `launch`
/// writes into).
fn profile_root() -> PathBuf {
    let out = std::process::Command::new(ff_rdp_bin())
        .args(["profiles", "list"])
        .output()
        .expect("`ff-rdp profiles list` must run");
    assert!(
        out.status.success(),
        "`ff-rdp profiles list` failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("profiles list must emit JSON");
    let path = json["results"]["path"]
        .as_str()
        .expect("profiles list must report results.path");
    PathBuf::from(path)
}

/// Basenames of every `ff-rdp-profile-*` directory currently under `root`.
fn managed_profiles(root: &Path) -> BTreeSet<String> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return BTreeSet::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.starts_with("ff-rdp-profile-").then_some(name)
        })
        .collect()
}

/// Whether `dir` names an owner process that is still alive.
///
/// A directory left by another live test running concurrently is owned by a
/// running Firefox; the directory this test is hunting is owned by a process
/// that is already dead (or carries no marker at all, pre-iter-171). That is
/// the distinction that makes the assertion below robust in a shared profile
/// root instead of a flake generator.
fn has_live_owner(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join(OWNER_PID_MARKER))
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .is_some_and(pid_alive)
}

/// AC 2, live: `--launch-timeout 0` makes the debug-port wait fail on the very
/// first poll. `launch` kills the Firefox it started and returns an error —
/// and must take the profile directory it created with it.
///
/// Fails on `main`, where the directory survives with a marker naming the
/// Firefox that was just killed.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and a real Firefox"]
fn live_175_failed_launch_leaves_no_profile_dir() {
    if !live_tests_enabled() {
        eprintln!("skipping: FF_RDP_LIVE_TESTS != 1");
        return;
    }

    let root = profile_root();
    let before = managed_profiles(&root);

    // A port nothing is listening on, so the pre-spawn occupancy check passes
    // and the launch gets far enough to create a profile.
    let port = 7900 + (std::process::id() % 90) as u16;
    let out = ff_rdp_launch_command()
        .args([
            "launch",
            "--headless",
            "--debug-port",
            &port.to_string(),
            "--launch-timeout",
            "0",
        ])
        .output()
        .expect("`ff-rdp launch` must run");

    assert!(
        !out.status.success(),
        "--launch-timeout 0 must fail the launch; stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        stderr.contains("did not open debug port"),
        "expected the port-deadline error, got: {stderr}"
    );

    let after = managed_profiles(&root);
    let leaked: Vec<String> = after
        .difference(&before)
        .filter(|name| !has_live_owner(&root.join(name)))
        .cloned()
        .collect();

    assert!(
        leaked.is_empty(),
        "iter-175: a launch that failed waiting for the debug port left {} profile \
         director{} behind under {}: {:?}",
        leaked.len(),
        if leaked.len() == 1 { "y" } else { "ies" },
        root.display(),
        leaked
    );
}

/// The other direction, and the reason the guard is disarmed rather than
/// simply absent: a launch that *succeeds* must keep its profile directory,
/// with an owner marker naming the Firefox now using it.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and a real Firefox"]
fn live_175_successful_launch_keeps_its_profile_dir() {
    if !live_tests_enabled() {
        eprintln!("skipping: FF_RDP_LIVE_TESTS != 1");
        return;
    }

    let ff = LiveFirefox::headless_on_random_port();
    let root = profile_root();

    let owned: Vec<PathBuf> = managed_profiles(&root)
        .into_iter()
        .map(|name| root.join(name))
        .filter(|dir| {
            std::fs::read_to_string(dir.join(OWNER_PID_MARKER))
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                == Some(ff.pid())
        })
        .collect();

    assert_eq!(
        owned.len(),
        1,
        "a running Firefox (pid {}) must still own exactly one profile directory under {}, \
         found {owned:?}",
        ff.pid(),
        root.display()
    );
    assert!(
        owned[0].join("user.js").exists(),
        "the surviving profile must still hold its user.js: {}",
        owned[0].display()
    );
}
