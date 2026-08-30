//! E2E tests for `ff-rdp install-hook` (iter-212 Theme B).
//!
//! `HOME` and `USERPROFILE` are both redirected at a temp dir for every run —
//! see `install_skill.rs` for why both are needed (iter-108: `dirs::home_dir()`
//! ignores `HOME` on Windows, so these tests would otherwise write into the
//! developer's real `~/.claude/settings.json`).

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use super::support;

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

fn run(home: &Path, extra: &[&str]) -> std::process::Output {
    Command::new(ff_rdp_bin())
        .args(["install-hook"])
        .args(extra)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("RUST_LOG")
        .output()
        .expect("spawn ff-rdp")
}

fn settings_path(home: &Path) -> std::path::PathBuf {
    home.join(".claude").join("settings.json")
}

fn results(out: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice::<serde_json::Value>(&out.stdout)
        .expect("stdout must be JSON")
        .get("results")
        .cloned()
        .expect("an envelope with results")
}

/// AC `install_hook_is_idempotent`: two installs produce a byte-identical
/// `settings.json`, and the second reports a no-op.
#[test]
fn e2e_212_install_hook_is_idempotent() {
    let home = TempDir::new().expect("tempdir");
    let path = settings_path(home.path());

    let first = run(home.path(), &["--claude"]);
    assert!(first.status.success(), "{}", support::output_note(&first));
    assert_eq!(results(&first)["action"], "installed");
    let after_first = fs::read(&path).expect("settings.json written");

    let second = run(home.path(), &["--claude"]);
    assert!(second.status.success(), "{}", support::output_note(&second));
    assert_eq!(
        results(&second)["action"],
        "no-op",
        "a second install must report that it changed nothing"
    );
    let after_second = fs::read(&path).expect("settings.json still there");
    assert_eq!(
        after_first, after_second,
        "the file must be byte-identical after a second install"
    );

    // …and there is exactly one entry, not two.
    let settings: serde_json::Value =
        serde_json::from_slice(&after_second).expect("settings.json must be JSON");
    let groups = settings["hooks"]["SessionStart"]
        .as_array()
        .expect("SessionStart array");
    assert_eq!(groups.len(), 1, "{settings}");
    assert_eq!(groups[0]["ff_rdp_managed"], serde_json::json!(true));
    assert!(
        groups[0]["hooks"][0]["command"]
            .as_str()
            .is_some_and(|c| c.ends_with("home --hook")),
        "the hook must run the trimmed home view: {settings}"
    );
}

/// AC `install_hook_repairs_a_moved_binary_path`: a stale absolute path is
/// rewritten and every unrelated hook survives.
#[test]
fn e2e_212_install_hook_repairs_a_moved_binary_path() {
    let home = TempDir::new().expect("tempdir");
    let path = settings_path(home.path());
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(
        &path,
        serde_json::to_string_pretty(&serde_json::json!({
            "model": "opus",
            "hooks": {
                "SessionStart": [
                    {"hooks": [{"type": "command", "command": "echo mine"}]},
                    {
                        "hooks": [{"type": "command", "command": "/gone/ff-rdp home --hook"}],
                        "ff_rdp_managed": true
                    }
                ],
                "PreToolUse": [{"hooks": [{"type": "command", "command": "guard.sh"}]}]
            }
        }))
        .expect("serialize"),
    )
    .expect("seed settings.json");

    let out = run(home.path(), &["--claude"]);
    assert!(out.status.success(), "{}", support::output_note(&out));
    assert_eq!(results(&out)["action"], "repaired");

    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("read")).expect("JSON");
    let groups = settings["hooks"]["SessionStart"]
        .as_array()
        .expect("SessionStart array");
    assert_eq!(groups.len(), 2, "repair must not duplicate: {settings}");
    assert_eq!(groups[0]["hooks"][0]["command"], "echo mine");
    assert!(
        groups[1]["hooks"][0]["command"]
            .as_str()
            .is_some_and(|c| !c.starts_with("/gone/")),
        "the stale path must be gone: {settings}"
    );
    assert_eq!(
        settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "guard.sh"
    );
    assert_eq!(settings["model"], "opus");
}

/// AC `install_hook_uninstall_removes_only_its_entry`.
#[test]
fn e2e_212_uninstall_removes_only_the_managed_entry() {
    let home = TempDir::new().expect("tempdir");
    let path = settings_path(home.path());
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(
        &path,
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"echo mine"}]}]}}"#,
    )
    .expect("seed");

    assert!(run(home.path(), &["--claude"]).status.success());
    let out = run(home.path(), &["--claude", "--uninstall"]);
    assert!(out.status.success(), "{}", support::output_note(&out));
    assert_eq!(results(&out)["action"], "uninstalled");

    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("read")).expect("JSON");
    let groups = settings["hooks"]["SessionStart"]
        .as_array()
        .expect("SessionStart array");
    assert_eq!(groups.len(), 1, "{settings}");
    assert_eq!(groups[0]["hooks"][0]["command"], "echo mine");

    // A second uninstall is a clean no-op, not an error.
    let again = run(home.path(), &["--claude", "--uninstall"]);
    assert!(again.status.success(), "{}", support::output_note(&again));
    assert_eq!(results(&again)["action"], "not-installed");
}

/// `--dry-run` must print the entry and touch nothing.
#[test]
fn e2e_212_dry_run_writes_nothing() {
    let home = TempDir::new().expect("tempdir");
    let out = run(home.path(), &["--claude", "--dry-run"]);
    assert!(out.status.success(), "{}", support::output_note(&out));
    let results = results(&out);
    assert_eq!(results["dry_run"], serde_json::json!(true));
    assert_eq!(results["action"], "installed");
    assert!(
        results["entry"]["hooks"][0]["command"]
            .as_str()
            .is_some_and(|c| c.ends_with("home --hook")),
        "the entry it would write must be shown: {results}"
    );
    assert!(
        !settings_path(home.path()).exists(),
        "--dry-run must not create the settings file"
    );
}

/// A target with no verified file format refuses rather than writing an entry
/// that would look installed and never fire.
#[test]
fn e2e_212_unsupported_targets_refuse_and_write_nothing() {
    for target in ["--codex", "--opencode"] {
        let home = TempDir::new().expect("tempdir");
        let out = run(home.path(), &[target]);
        assert!(
            !out.status.success(),
            "{target} must exit non-zero: {}",
            support::output_note(&out)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("not supported yet"),
            "the refusal must say why: {stdout}"
        );
        assert!(
            !settings_path(home.path()).exists(),
            "{target} must not write a Claude settings file"
        );
    }
}

/// No target flag at all is a usage error, not a silent default to Claude.
#[test]
fn e2e_212_a_target_flag_is_required() {
    let home = TempDir::new().expect("tempdir");
    let out = run(home.path(), &[]);
    assert!(!out.status.success(), "{}", support::output_note(&out));
    assert!(
        !settings_path(home.path()).exists(),
        "nothing may be written without a target"
    );
}

/// A settings file that is not JSON must be refused, never rewritten — it is
/// the user's configuration and there is no safe merge.
#[test]
fn e2e_212_malformed_settings_are_refused_not_clobbered() {
    let home = TempDir::new().expect("tempdir");
    let path = settings_path(home.path());
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    let original = "{ this is not json\n";
    fs::write(&path, original).expect("seed");

    let out = run(home.path(), &["--claude"]);
    assert!(!out.status.success(), "{}", support::output_note(&out));
    assert_eq!(
        fs::read_to_string(&path).expect("read"),
        original,
        "the file must be left exactly as it was"
    );
}
