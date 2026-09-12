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
        .env_remove("CODEX_HOME")
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
    let contract = include_str!("../fixtures/opencode-plugin-contract.txt");
    assert!(contract.contains("=> Promise<Hooks>"));
    assert!(contract.contains("event?: (input: { event: Event }) => Promise<void>"));
    let target = "--opencode";
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

fn codex_home() -> TempDir {
    let home = TempDir::new().expect("tempdir");
    fs::create_dir(home.path().join(".codex")).expect("mkdir");
    fs::write(
        home.path().join(".codex/config.toml"),
        "[features]\nhooks = true\n",
    )
    .expect("config");
    home
}

fn codex_path(home: &Path) -> std::path::PathBuf {
    home.join(".codex/hooks.json")
}

#[test]
fn install_hook_codex_custom_home_controls_gate_and_all_mutations() {
    let home = codex_home();
    let custom = TempDir::new().unwrap();
    let custom_path = custom.path().join("hooks.json");
    let default_path = codex_path(home.path());
    let unrelated = include_str!("../fixtures/codex-hooks-documented.json");
    fs::write(&default_path, unrelated).unwrap();
    let invoke = |args: &[&str]| {
        Command::new(ff_rdp_bin())
            .arg("install-hook")
            .arg("--codex")
            .args(args)
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .env("CODEX_HOME", custom.path())
            .env_remove("RUST_LOG")
            .output()
            .unwrap()
    };
    // An enabled default home must never bypass the active home's missing gate.
    for args in [vec![], vec!["--dry-run"]] {
        let out = invoke(&args);
        assert_eq!(out.status.code(), Some(1), "{}", support::output_note(&out));
        assert!(String::from_utf8_lossy(&out.stdout).contains(custom.path().to_str().unwrap()));
        assert!(!custom_path.exists());
        assert!(!custom.path().join("config.toml").exists());
        assert_eq!(fs::read_to_string(&default_path).unwrap(), unrelated);
    }
    let enabled = "[features]\nhooks = true\n";
    fs::write(custom.path().join("config.toml"), enabled).unwrap();
    // The inverse proves installation reads the custom gate, not the default.
    let disabled = "[features]\nhooks = false\n";
    fs::write(home.path().join(".codex/config.toml"), disabled).unwrap();
    fs::write(&custom_path, unrelated).unwrap();
    assert!(invoke(&["--dry-run"]).status.success());
    assert_eq!(fs::read_to_string(&custom_path).unwrap(), unrelated);
    let first = invoke(&[]);
    assert!(first.status.success(), "{}", support::output_note(&first));
    assert_eq!(results(&first)["action"], "installed");
    let installed = fs::read(&custom_path).unwrap();
    assert_eq!(results(&invoke(&[]))["action"], "no-op");
    assert_eq!(fs::read(&custom_path).unwrap(), installed);
    assert!(invoke(&["--uninstall", "--dry-run"]).status.success());
    assert_eq!(fs::read(&custom_path).unwrap(), installed);
    fs::write(custom.path().join("config.toml"), disabled).unwrap();
    let removed = invoke(&["--uninstall"]);
    assert!(
        removed.status.success(),
        "{}",
        support::output_note(&removed)
    );
    assert_eq!(results(&removed)["action"], "uninstalled");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&custom_path).unwrap()).unwrap(),
        serde_json::from_str::<serde_json::Value>(unrelated).unwrap()
    );
    assert_eq!(
        results(&invoke(&["--uninstall"]))["action"],
        "not-installed"
    );
    assert_eq!(fs::read_to_string(&default_path).unwrap(), unrelated);
    assert_eq!(
        fs::read_to_string(home.path().join(".codex/config.toml")).unwrap(),
        disabled
    );
    assert_eq!(
        fs::read_to_string(custom.path().join("config.toml")).unwrap(),
        disabled
    );
}

#[test]
fn install_hook_codex_empty_custom_home_uses_default() {
    let home = codex_home();
    let output = Command::new(ff_rdp_bin())
        .args(["install-hook", "--codex"])
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("CODEX_HOME", "")
        .env_remove("RUST_LOG")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", support::output_note(&output));
    assert!(codex_path(home.path()).exists());
}

#[test]
fn install_hook_codex_is_idempotent() {
    let home = codex_home();
    let path = codex_path(home.path());
    let first = run(home.path(), &["--codex"]);
    assert!(first.status.success(), "{}", support::output_note(&first));
    assert_eq!(results(&first)["action"], "installed");
    assert!(
        results(&first)["next_step"]
            .as_str()
            .unwrap()
            .contains("/hooks")
    );
    let bytes = fs::read(&path).unwrap();
    // A fixed historical mtime proves the writer wasn't reached on the no-op.
    let old = filetime::FileTime::from_unix_time(1_600_000_000, 0);
    filetime::set_file_mtime(&path, old).unwrap();
    let second = run(home.path(), &["--codex"]);
    assert!(second.status.success(), "{}", support::output_note(&second));
    assert_eq!(results(&second)["action"], "no-op");
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(
        filetime::FileTime::from_last_modification_time(&fs::metadata(&path).unwrap()),
        old
    );
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/codex-hooks-schema.json")).unwrap();
    let installed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    // HooksToml is the value under `hooks`, not the app-server response model.
    assert!(validator.is_valid(&installed["hooks"]), "{installed}");
    let mut broken = installed["hooks"].clone();
    broken["SessionStart"][0]["hooks"][0]["type"] = serde_json::json!("invented");
    assert!(
        !validator.is_valid(&broken),
        "schema control must reject an invalid handler"
    );
}

#[test]
fn install_hook_codex_refuses_without_the_features_gate() {
    for config in [
        None,
        Some(""),
        Some("# [features]\n# hooks = true\n"),
        Some("[features]\nhooks = false\n"),
        Some("[features]\nhooks = 'true'\n"),
        Some("[other.features]\nhooks = true\n"),
        Some("[features\nhooks = true\n"),
        Some("[features]\nhooks=true\nhooks=false\n"),
    ] {
        let home = TempDir::new().unwrap();
        let config_path = home.path().join(".codex/config.toml");
        if let Some(config) = config {
            fs::create_dir_all(config_path.parent().unwrap()).unwrap();
            fs::write(&config_path, config).unwrap();
        }
        for extra in [vec!["--codex"], vec!["--codex", "--dry-run"]] {
            let output = run(home.path(), &extra);
            assert_eq!(
                output.status.code(),
                Some(1),
                "{config:?}: {}",
                support::output_note(&output)
            );
            assert!(!codex_path(home.path()).exists());
            assert_eq!(fs::read_to_string(&config_path).ok().as_deref(), config);
        }
        // Existing hooks must also remain byte-identical on a refused install.
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(codex_path(home.path()), "{\"untouched\":true}\n").unwrap();
        assert_eq!(run(home.path(), &["--codex"]).status.code(), Some(1));
        assert_eq!(
            fs::read_to_string(codex_path(home.path())).unwrap(),
            "{\"untouched\":true}\n"
        );
    }
}

#[test]
fn install_hook_codex_accepts_real_toml_forms() {
    for config in [
        "features.hooks = true\n",
        "features = { hooks = true }\n",
        "[features]\n\"hooks\" = true # opt in\n",
    ] {
        let home = codex_home();
        fs::write(home.path().join(".codex/config.toml"), config).unwrap();
        let output = run(home.path(), &["--codex"]);
        assert!(output.status.success(), "{}", support::output_note(&output));
        assert_eq!(
            fs::read_to_string(home.path().join(".codex/config.toml")).unwrap(),
            config
        );
    }
}

#[test]
fn install_hook_codex_leaves_other_entries_untouched() {
    let home = codex_home();
    let path = codex_path(home.path());
    let original: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/codex-hooks-documented.json")).unwrap();
    fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    assert!(run(home.path(), &["--codex"]).status.success());
    let mut installed: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let groups = installed["hooks"]["SessionStart"].as_array_mut().unwrap();
    let owned = groups.last_mut().unwrap();
    owned["hooks"][0]["command"] = serde_json::json!("/gone/ff-rdp home --hook");
    fs::write(&path, serde_json::to_vec(&installed).unwrap()).unwrap();
    let repaired = run(home.path(), &["--codex"]);
    assert!(
        repaired.status.success(),
        "{}",
        support::output_note(&repaired)
    );
    assert_eq!(results(&repaired)["action"], "repaired");
    assert!(
        run(home.path(), &["--codex", "--uninstall"])
            .status
            .success()
    );
    let remaining: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(remaining, original);
}

#[test]
fn install_hook_codex_uninstall_removes_only_its_entry() {
    let home = codex_home();
    let path = codex_path(home.path());
    let original = serde_json::json!({"hooks":{"SessionStart":[
        {"hooks":[{"type":"command","command":"ff-rdp home --hook"}]},
        {"ff_rdp_managed":"true","hooks":[]}, {"ff_rdp_managed":false,"hooks":[]}
    ]}});
    fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    assert!(run(home.path(), &["--codex"]).status.success());
    fs::write(home.path().join(".codex/config.toml"), "malformed = [").unwrap();
    let output = run(home.path(), &["--codex", "--uninstall"]);
    assert!(output.status.success(), "{}", support::output_note(&output));
    assert_eq!(results(&output)["action"], "uninstalled");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap(),
        original
    );
    assert_eq!(
        results(&run(home.path(), &["--codex", "--uninstall"]))["action"],
        "not-installed"
    );
}

#[test]
fn install_hook_codex_dry_run_and_project_refusal_write_nothing() {
    let home = codex_home();
    let output = run(home.path(), &["--codex", "--dry-run"]);
    assert!(output.status.success(), "{}", support::output_note(&output));
    assert!(!codex_path(home.path()).exists());
    assert_eq!(results(&output)["dry_run"], true);
    assert_eq!(
        run(home.path(), &["--codex", "--project"]).status.code(),
        Some(1)
    );
    assert!(!codex_path(home.path()).exists());
    assert!(run(home.path(), &["--codex"]).status.success());
    let bytes = fs::read(codex_path(home.path())).unwrap();
    let output = run(home.path(), &["--codex", "--uninstall", "--dry-run"]);
    assert!(output.status.success(), "{}", support::output_note(&output));
    assert_eq!(results(&output)["action"], "uninstalled");
    assert_eq!(fs::read(codex_path(home.path())).unwrap(), bytes);
}

#[test]
fn install_hook_codex_refuses_malformed_containers_without_rewrite() {
    for content in [
        "null",
        "[]",
        "{bad",
        "{\"hooks\":null}",
        "{\"hooks\":{\"SessionStart\":{}}}",
    ] {
        let home = codex_home();
        fs::write(codex_path(home.path()), content).unwrap();
        assert_eq!(run(home.path(), &["--codex"]).status.code(), Some(1));
        assert_eq!(
            fs::read_to_string(codex_path(home.path())).unwrap(),
            content
        );
    }
}
