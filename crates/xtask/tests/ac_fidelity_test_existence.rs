//! iter-66 regression test for `tools/ralph-loop/scripts/ac-fidelity-check.sh`.
//!
//! AC: feeding a fake AC `- [x] nonexistent_test: …` to the script must return
//! a non-zero exit code (i.e. the script rejects test slugs that don't resolve
//! to an `fn <slug>` anywhere in the workspace).  This guards against the
//! iter-61w failure mode where four ACs were ticked without writing the
//! regression tests.

use std::io::Write;
use std::process::Command;

fn repo_root() -> std::path::PathBuf {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("git rev-parse");
    assert!(out.status.success(), "git rev-parse failed");
    let s = String::from_utf8(out.stdout).expect("utf-8");
    std::path::PathBuf::from(s.trim())
}

fn write_temp_plan(body: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new()
        .prefix("ac-fidelity-test-")
        .suffix(".md")
        .tempfile()
        .expect("create temp file");
    f.write_all(body.as_bytes()).expect("write plan");
    f.flush().expect("flush plan");
    f
}

fn run_script(plan_path: &std::path::Path) -> std::process::Output {
    let root = repo_root();
    let script = root.join("tools/ralph-loop/scripts/ac-fidelity-check.sh");
    Command::new("bash")
        .arg(&script)
        .arg("--plan")
        .arg(plan_path)
        // Use a no-op range so the diff is empty — we want to isolate the
        // test-existence heuristic, not whatever happens to be on the branch.
        .arg("--range")
        .arg("HEAD..HEAD")
        .current_dir(&root)
        .output()
        .expect("run ac-fidelity-check.sh")
}

#[test]
fn ac_fidelity_rejects_nonexistent_test_slug() {
    // Use a slug that is exceedingly unlikely to collide with any real test
    // name in the workspace.  Adding `xyzzy_iter66` as a guard.
    let body = "---\ntitle: fake\n---\n\n## Acceptance Criteria\n\n- [x] test_nonexistent_xyzzy_iter66_guard: must reject\n";
    let plan = write_temp_plan(body);
    let out = run_script(plan.path());
    assert!(
        !out.status.success(),
        "script must reject nonexistent test slug — exit was 0\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("test_nonexistent_xyzzy_iter66_guard"),
        "failure message should name the missing slug; got: {stdout}"
    );
}

#[test]
fn ac_fidelity_accepts_existing_test_slug() {
    // `test_refstore_capped` exists in crates/ff-rdp-cli/src/daemon/server.rs.
    // The strengthened script should accept this even with an empty diff,
    // because the pre-existing fn satisfies the test-existence check.
    let body = "---\ntitle: real\n---\n\n## Acceptance Criteria\n\n- [x] test_refstore_capped: cap holds at MAX_REFS\n";
    let plan = write_temp_plan(body);
    let out = run_script(plan.path());
    assert!(
        out.status.success(),
        "script must accept existing test slug — exit was non-zero\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn ac_fidelity_skip_flag_disables_existence_check() {
    // With --skip-test-existence, a nonexistent slug should NOT fail the AC
    // (the later heuristics may still fail it for lack of diff evidence — and
    // that's acceptable; we just verify the existence check is skipped).
    let body = "---\ntitle: skip\n---\n\n## Acceptance Criteria\n\n- [x] test_nonexistent_skip_xyzzy: dummy\n";
    let plan = write_temp_plan(body);
    let root = repo_root();
    let script = root.join("tools/ralph-loop/scripts/ac-fidelity-check.sh");
    let out = Command::new("bash")
        .arg(&script)
        .arg("--plan")
        .arg(plan.path())
        .arg("--range")
        .arg("HEAD..HEAD")
        .arg("--skip-test-existence")
        .current_dir(&root)
        .output()
        .expect("run ac-fidelity-check.sh");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("no `fn test_nonexistent_skip_xyzzy` exists"),
        "--skip-test-existence must suppress the existence-check error; stdout: {stdout}"
    );
}
