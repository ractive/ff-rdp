use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::path::PathBuf;

#[derive(ClapArgs)]
pub struct Args {
    /// Path to the iteration plan markdown file.
    pub plan: PathBuf,
}

/// Extract the iteration number from a plan filename like `iteration-85-slug.md`.
fn extract_iteration_number(plan: &std::path::Path) -> Option<u32> {
    let stem = plan.file_stem()?.to_str()?;
    // Expected prefix: "iteration-N-"
    let rest = stem.strip_prefix("iteration-")?;
    let end = rest.find('-').unwrap_or(rest.len());
    rest[..end].parse().ok()
}

pub fn run(args: Args) -> Result<()> {
    run_inner(args, false)
}

/// Inner implementation. When `force` is true the `FF_RDP_LIVE_TESTS` guard is
/// bypassed — used by unit tests to avoid depending on the environment.
pub fn run_inner(args: Args, force: bool) -> Result<()> {
    if !force && std::env::var("FF_RDP_LIVE_TESTS").as_deref() != Ok("1") {
        println!("check-dogfood-script: SKIP (FF_RDP_LIVE_TESTS not set)");
        return Ok(());
    }

    // Parse frontmatter to find dogfood_script.
    let content = std::fs::read_to_string(&args.plan)
        .with_context(|| format!("failed to read {:?}", args.plan))?;

    let plan = crate::check_iteration_plan::parse_plan(&content)
        .with_context(|| format!("failed to parse plan {:?}", args.plan))?;

    let script_name = match plan.frontmatter.dogfood_script.as_deref() {
        None | Some("") => {
            println!("check-dogfood-script: SKIP (no dogfood_script field in plan)");
            return Ok(());
        }
        Some(s) => s,
    };

    // Resolve the script path relative to the plan's parent directory.
    let plan_dir = args
        .plan
        .parent()
        .with_context(|| format!("plan path has no parent dir: {:?}", args.plan))?;
    let script_path = plan_dir.join(script_name);

    if !script_path.exists() {
        anyhow::bail!(
            "check-dogfood-script: FAIL (script does not exist: {:?})",
            script_path
        );
    }

    // Extract iteration number to determine the sentinel path.
    let iter_num = extract_iteration_number(&args.plan).with_context(|| {
        format!(
            "could not extract iteration number from plan filename: {:?}",
            args.plan
        )
    })?;

    run_script(&script_path, iter_num)
}

#[cfg(unix)]
fn run_script(script_path: &std::path::Path, iter_num: u32) -> Result<()> {
    let sentinel = PathBuf::from(format!("/tmp/ff-rdp-iter-{iter_num}-dogfood-ok"));

    // Pre-clean: remove any stale sentinel.
    if sentinel.exists() {
        std::fs::remove_file(&sentinel)
            .with_context(|| format!("failed to remove stale sentinel {:?}", sentinel))?;
    }

    // Run the script with bash.  Pass the script path as an OsStr to avoid
    // lossy UTF-8 conversion on platforms where paths can be non-UTF-8.
    let status = std::process::Command::new("bash")
        .arg("-euo")
        .arg("pipefail")
        .arg(script_path)
        .status()
        .with_context(|| format!("failed to invoke bash for {:?}", script_path))?;

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        anyhow::bail!("check-dogfood-script: FAIL (script exited with code {code})");
    }

    if !sentinel.exists() {
        anyhow::bail!(
            "check-dogfood-script: FAIL (missing sentinel {:?} after script succeeded)",
            sentinel
        );
    }

    println!("check-dogfood-script: OK");
    Ok(())
}

#[cfg(not(unix))]
fn run_script(_script_path: &std::path::Path, _iter_num: u32) -> Result<()> {
    // bash is not guaranteed on Windows; the CI gate runs on ubuntu-latest only.
    println!("check-dogfood-script: SKIP (bash invocation not supported on this platform)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::TempDir;

    /// Write a minimal plan file with the given extra frontmatter into `dir`.
    fn write_plan(dir: &TempDir, name: &str, extra_fm: &str) -> PathBuf {
        let path = dir.path().join(name);
        let content = format!(
            "---\ntitle: \"Test Plan\"\nstatus: planned\ntype: iteration\n{extra_fm}---\n\n# Body\n"
        );
        std::fs::write(&path, content).unwrap();
        path
    }

    /// Write an executable shell script into `dir`.
    fn write_script(dir: &TempDir, name: &str, body: &str) -> PathBuf {
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "#!/usr/bin/env bash").unwrap();
        writeln!(f, "{body}").unwrap();
        // Mark executable on unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    #[cfg(unix)]
    fn xtask_check_dogfood_script_smoke() {
        // Happy path: script exits 0 AND writes the sentinel.
        let dir = TempDir::new().unwrap();
        let plan_path = write_plan(
            &dir,
            "iteration-99-smoke.md",
            "dogfood_script: smoke.dogfood.sh\n",
        );
        write_script(
            &dir,
            "smoke.dogfood.sh",
            "touch /tmp/ff-rdp-iter-99-dogfood-ok",
        );

        // Pre-clean sentinel in case a prior run left it.
        let _ = std::fs::remove_file("/tmp/ff-rdp-iter-99-dogfood-ok");

        let result = run_inner(Args { plan: plan_path }, true);
        assert!(result.is_ok(), "expected success, got: {result:?}");
        assert!(
            std::path::Path::new("/tmp/ff-rdp-iter-99-dogfood-ok").exists(),
            "sentinel should exist after successful run"
        );
        // Clean up.
        let _ = std::fs::remove_file("/tmp/ff-rdp-iter-99-dogfood-ok");
    }

    #[test]
    #[cfg(unix)]
    fn xtask_check_dogfood_script_missing_sentinel() {
        // Script exits 0 but does NOT write the sentinel → run_script returns
        // an error via anyhow::bail!, which the xtask binary propagates as a
        // non-zero exit code.  We invoke a child process to observe the exit
        // code end-to-end (rather than asserting on run_inner's Result).
        let dir = TempDir::new().unwrap();
        let plan_path = write_plan(
            &dir,
            "iteration-98-no-sentinel.md",
            "dogfood_script: no-sentinel.dogfood.sh\n",
        );
        write_script(
            &dir,
            "no-sentinel.dogfood.sh",
            "# intentionally no sentinel",
        );

        // Pre-clean sentinel.
        let _ = std::fs::remove_file("/tmp/ff-rdp-iter-98-dogfood-ok");

        // run_script returns an error on failure, which the xtask binary turns
        // into a non-zero exit. Spawn cargo run to observe the binary's exit
        // code (current_exe inside the test is the test runner, not xtask).
        let output = std::process::Command::new("cargo")
            .args(["run", "-q", "-p", "xtask", "--"])
            .env("FF_RDP_LIVE_TESTS", "1")
            .args(["check-dogfood-script", plan_path.to_str().unwrap()])
            .output()
            .unwrap();

        // Should have exited non-zero (missing sentinel).
        assert!(
            !output.status.success(),
            "expected failure when sentinel is missing"
        );
    }

    #[test]
    fn xtask_check_dogfood_script_no_field_skip() {
        // Plan with no dogfood_script field → SKIP, exit 0.
        let dir = TempDir::new().unwrap();
        let plan_path = write_plan(
            &dir,
            "iteration-97-no-script.md",
            "dogfood_path: \"ff-rdp --help\"\n",
        );

        let result = run_inner(Args { plan: plan_path }, true);
        assert!(result.is_ok(), "expected SKIP success: {result:?}");
    }

    #[test]
    fn xtask_extract_iteration_number() {
        let p = std::path::Path::new("iteration-85-dogfood-57-carryovers.md");
        assert_eq!(extract_iteration_number(p), Some(85));

        let p2 = std::path::Path::new("iteration-1-foo.md");
        assert_eq!(extract_iteration_number(p2), Some(1));

        let p3 = std::path::Path::new("not-an-iteration.md");
        assert_eq!(extract_iteration_number(p3), None);
    }
}
