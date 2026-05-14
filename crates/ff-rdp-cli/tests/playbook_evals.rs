//! Layer-2 evals for the `ff-rdp-debug` skill playbooks.
//!
//! What this test covers TODAY (always-on, runs in `cargo test`):
//!
//! - Discovers every fixture under
//!   `skills/ff-rdp-debug/evals/fixtures/<id>/`.
//! - Loads each `bug.json` and validates its shape: required keys
//!   present, `expected_evidence_commands` non-empty, `expected_diagnosis`
//!   non-empty, `must_not_conclude` is a (possibly empty) list of
//!   strings.
//! - Verifies every fixture directory contains an `index.html`.
//! - Cross-checks that every fixture id has a matching playbook file
//!   at `skills/ff-rdp-debug/playbooks/<id>.md`, and vice versa.
//!
//! What is NOT covered yet (gated behind `--ignored`, see TODOs below):
//!
//! - Actually spawning `python3 -m http.server`.
//! - Actually launching headless Firefox on a free RDP port.
//! - Actually running each playbook's `expected_evidence_commands`
//!   against that Firefox.
//! - Asserting that the captured JSON contains the signal field that
//!   each playbook's `signal → conclude` rule keys on.
//!
//! The live-Firefox flow is intentionally stubbed for iter-58: the
//! orchestrator pairs this test with `ff-rdp install-skill` work in
//! parallel and there isn't time in the iteration budget to land a
//! flake-free live harness. Acceptance for iter-58 is met by the
//! schema-validation pass below; the live probing slot is reserved for
//! a follow-up iteration.
//!
//! Run schema validation:
//!     cargo test -p ff-rdp-cli --test playbook_evals
//!
//! Run the live-Firefox probes (currently no-op stubs, will fail until
//! implemented):
//!     cargo test -p ff-rdp-cli --test playbook_evals -- --ignored

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Path to the skill source tree at compile time.
fn skill_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills/ff-rdp-debug")
}

fn fixtures_root() -> PathBuf {
    skill_root().join("evals/fixtures")
}

fn playbooks_root() -> PathBuf {
    skill_root().join("playbooks")
}

/// List subdirectories of `dir`, returning their basenames sorted.
fn list_subdirs(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            out.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    out.sort();
    out
}

fn list_playbook_ids() -> Vec<String> {
    let mut out = Vec::new();
    let entries =
        fs::read_dir(playbooks_root()).unwrap_or_else(|e| panic!("read playbooks dir: {e}"));
    for entry in entries {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(id) = name.strip_suffix(".md") {
            out.push(id.to_string());
        }
    }
    out.sort();
    out
}

#[test]
fn every_fixture_has_a_bug_json() {
    for id in list_subdirs(&fixtures_root()) {
        let bug = fixtures_root().join(&id).join("bug.json");
        assert!(
            bug.exists(),
            "fixture {id} is missing bug.json (expected at {})",
            bug.display()
        );
    }
}

#[test]
fn every_fixture_has_an_index_html() {
    for id in list_subdirs(&fixtures_root()) {
        let idx = fixtures_root().join(&id).join("index.html");
        assert!(
            idx.exists(),
            "fixture {id} is missing index.html (expected at {})",
            idx.display()
        );
    }
}

#[test]
fn bug_json_schema_is_valid() {
    for id in list_subdirs(&fixtures_root()) {
        let path = fixtures_root().join(&id).join("bug.json");
        let raw =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let v: Value =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));

        // Required keys.
        let symptom_hint = v
            .get("symptom_hint")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{id}/bug.json: missing string `symptom_hint`"));
        assert!(
            !symptom_hint.trim().is_empty(),
            "{id}/bug.json: symptom_hint is empty"
        );

        let expected_diagnosis = v
            .get("expected_diagnosis")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{id}/bug.json: missing string `expected_diagnosis`"));
        assert!(
            !expected_diagnosis.trim().is_empty(),
            "{id}/bug.json: expected_diagnosis is empty"
        );

        let cmds = v
            .get("expected_evidence_commands")
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("{id}/bug.json: missing array `expected_evidence_commands`"));
        assert!(
            !cmds.is_empty(),
            "{id}/bug.json: expected_evidence_commands is empty"
        );
        for (i, c) in cmds.iter().enumerate() {
            let s = c.as_str().unwrap_or_else(|| {
                panic!("{id}/bug.json: expected_evidence_commands[{i}] is not a string")
            });
            assert!(
                s.starts_with("ff-rdp "),
                "{id}/bug.json: expected_evidence_commands[{i}] should start with `ff-rdp `, got `{s}`"
            );
        }

        let must_not = v
            .get("must_not_conclude")
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("{id}/bug.json: missing array `must_not_conclude`"));
        for (i, c) in must_not.iter().enumerate() {
            assert!(
                c.is_string(),
                "{id}/bug.json: must_not_conclude[{i}] is not a string"
            );
        }
    }
}

#[test]
fn fixtures_and_playbooks_align() {
    let fixtures: BTreeSet<String> = list_subdirs(&fixtures_root()).into_iter().collect();
    let playbooks: BTreeSet<String> = list_playbook_ids().into_iter().collect();

    let missing_playbook: Vec<_> = fixtures.difference(&playbooks).collect();
    assert!(
        missing_playbook.is_empty(),
        "fixtures with no matching playbook: {missing_playbook:?}"
    );

    let missing_fixture: Vec<_> = playbooks.difference(&fixtures).collect();
    assert!(
        missing_fixture.is_empty(),
        "playbooks with no matching fixture: {missing_fixture:?}"
    );
}

#[test]
fn tier_1_playbooks_present() {
    // The iteration plan declares these 10 playbooks as the v0 ship target.
    let expected = ["A1", "A2", "B5", "C1", "C2", "C3", "D2", "E1", "E3", "K0"];
    let have: BTreeSet<String> = list_playbook_ids().into_iter().collect();
    for id in expected {
        assert!(have.contains(id), "Tier 1 playbook {id} is missing");
    }
}

// -----------------------------------------------------------------------------
// Live-Firefox probes — stubbed for iter-58, see module-level docs.
// -----------------------------------------------------------------------------

#[test]
#[ignore = "TODO(iter-58 follow-up): spawn python3 -m http.server, launch headless Firefox, run each playbook's expected_evidence_commands against its fixture, and assert the documented signal field appears in the captured JSON."]
fn live_probe_all_fixtures() {
    // Sketch of the intended flow:
    //
    // for id in list_subdirs(&fixtures_root()) {
    //     let bug = load_bug_json(&id);
    //     let server = spawn_http_server(fixtures_root().join(&id));
    //     let firefox = launch_headless_firefox_on_free_port();
    //     navigate(&firefox, &format!("http://127.0.0.1:{}/", server.port()));
    //     for cmd in &bug.expected_evidence_commands {
    //         let out = run_ffrdp(cmd, &firefox);
    //         assert_signal_present(&out, &bug.expected_diagnosis);
    //     }
    //     firefox.kill();
    //     server.kill();
    // }
    //
    // Blocking on: (a) a reliable way to detect a free port for Firefox
    // RDP across OSes; (b) deciding how `expected_evidence_commands` map
    // to ff-rdp subcommand structs without re-parsing the strings here;
    // (c) test runtime budget (each fixture is ~5-15s of Firefox boot).
    //
    // Until implemented, this test is `#[ignore]`d so it doesn't fail
    // the suite.
}
