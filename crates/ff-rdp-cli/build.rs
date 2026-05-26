/// Build script for `ff-rdp-cli`.
///
/// Embeds git provenance (`FF_RDP_BUILD_VERSION_SHA`, `FF_RDP_BUILD_DATE`) into
/// the binary at build time so `ff-rdp --version` can display them.
///
/// Safety-net: when `.git` is absent (crates.io tarball, offline build) or when
/// `CARGO_FF_RDP_FORCE_NO_GIT=1` is set, empty strings are emitted — the runtime
/// then falls back to the bare `CARGO_PKG_VERSION`.
///
/// CI/CD path: set `GIT_COMMIT` and `GIT_COMMIT_DATE` env vars before building
/// to bypass the `git` shell-out (useful in reproducible / hermetic builds).
use std::process::Command;

fn main() {
    // Rerun when HEAD or any ref changes (covers new commits and branch switches).
    // Derive the git directory via `git rev-parse --git-dir` so this works in
    // worktrees (where `.git` is a file, not a directory) and in repos where the
    // git dir is not at `.git/`.  Fall back to the hardcoded paths if git is
    // unavailable.
    if let Some(git_dir) = git_output(&["rev-parse", "--git-dir"]) {
        println!("cargo:rerun-if-changed={git_dir}/HEAD");
        println!("cargo:rerun-if-changed={git_dir}/refs/");
    } else {
        println!("cargo:rerun-if-changed=.git/HEAD");
        println!("cargo:rerun-if-changed=.git/refs/");
    }
    // Env-var-based overrides also trigger a rebuild when they change.
    println!("cargo:rerun-if-env-changed=GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=GIT_COMMIT_DATE");
    println!("cargo:rerun-if-env-changed=CARGO_FF_RDP_FORCE_NO_GIT");

    // Force-no-git escape hatch: used in unit tests and offline/tarball builds.
    if std::env::var_os("CARGO_FF_RDP_FORCE_NO_GIT").is_some() {
        emit_empty();
        return;
    }

    // CI path: pre-set env vars win over the git shell-out.
    let sha_from_env = std::env::var("GIT_COMMIT").ok();
    let date_from_env = std::env::var("GIT_COMMIT_DATE").ok();

    if let (Some(sha), Some(date)) = (sha_from_env, date_from_env) {
        println!("cargo:rustc-env=FF_RDP_BUILD_VERSION_SHA={sha}");
        println!("cargo:rustc-env=FF_RDP_BUILD_DATE={date}");
        return;
    }

    // Development / local build: shell out to git.
    let sha = git_output(&["rev-parse", "--short=12", "HEAD"]);
    let date = git_output(&["show", "-s", "--format=%cs", "HEAD"]);

    match (sha, date) {
        (Some(sha), Some(date)) => {
            let dirty = git_is_dirty();
            let sha = if dirty { format!("{sha}+dirty") } else { sha };
            println!("cargo:rustc-env=FF_RDP_BUILD_VERSION_SHA={sha}");
            println!("cargo:rustc-env=FF_RDP_BUILD_DATE={date}");
        }
        _ => {
            // git unavailable or not a git repo — emit empty strings.
            emit_empty();
        }
    }
}

/// Emit empty provenance strings (tarball / no-git fallback).
fn emit_empty() {
    println!("cargo:rustc-env=FF_RDP_BUILD_VERSION_SHA=");
    println!("cargo:rustc-env=FF_RDP_BUILD_DATE=");
}

/// Run `git <args>` and return the trimmed stdout on success.
fn git_output(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let s = s.trim().to_owned();
    if s.is_empty() { None } else { Some(s) }
}

/// Return `true` when the working tree has uncommitted changes.
fn git_is_dirty() -> bool {
    Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .is_some_and(|out| out.status.success() && !out.stdout.is_empty())
}
