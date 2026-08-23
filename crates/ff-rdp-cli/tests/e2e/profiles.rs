//! Tests for the `profiles` subcommand's root resolution (iter-188 Theme B).
//!
//! `profiles list` and `profiles prune` both resolve
//! `util::profile_dir::secure_profile_root()`, which — until iter-188 — was
//! the one per-user path that ignored `$FF_RDP_HOME`, while
//! `daemon/registry.rs` and `daemon_record.rs` both honoured it and both
//! documented it as "the same convention". Setting the override therefore
//! produced a *split* state directory. These tests pin the override down for
//! every command that reads the root, without needing a Firefox.

use super::support;

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

/// `profiles list` reports the overridden root, and creates it.
#[test]
fn profiles_list_root_follows_ff_rdp_home() {
    let home = tempfile::tempdir().expect("tempdir");

    let output = std::process::Command::new(ff_rdp_bin())
        .args(["profiles", "list"])
        .env("FF_RDP_HOME", home.path())
        .output()
        .expect("spawn");

    assert!(
        output.status.success(),
        "profiles list must succeed under an overridden home; {}",
        support::output_note(&output)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("profiles list must emit a JSON envelope on stdout");
    let path = json["results"]["path"]
        .as_str()
        .expect("profiles list JSON must expose results.path");

    let expected = home.path().join("ff-rdp").join("profiles");
    assert_eq!(
        std::path::Path::new(path),
        expected,
        "profiles list must report the overridden root"
    );
    assert!(
        expected.is_dir(),
        "the overridden root must have been created at {}",
        expected.display()
    );
}

/// `profiles prune --all` operates inside the overridden root — it removes
/// managed directories there, and cannot see (let alone remove) anything in
/// another home. This is what makes the live suite's global
/// "no ff-rdp-managed profile is running anywhere" precondition satisfiable
/// while other tests run their own browsers (iter-188 Theme C).
#[test]
fn profiles_prune_is_scoped_to_ff_rdp_home() {
    let mine = tempfile::tempdir().expect("tempdir");
    let theirs = tempfile::tempdir().expect("tempdir");

    let my_root = mine.path().join("ff-rdp").join("profiles");
    let their_root = theirs.path().join("ff-rdp").join("profiles");
    let my_profile = my_root.join("ff-rdp-profile-0000000000000001");
    let their_profile = their_root.join("ff-rdp-profile-0000000000000002");
    std::fs::create_dir_all(&my_profile).expect("seed managed profile");
    std::fs::create_dir_all(&their_profile).expect("seed foreign managed profile");

    let output = std::process::Command::new(ff_rdp_bin())
        .args(["profiles", "prune", "--all"])
        .env("FF_RDP_HOME", mine.path())
        .output()
        .expect("spawn");

    assert!(
        output.status.success(),
        "profiles prune --all must succeed under an overridden home; {}",
        support::output_note(&output)
    );
    assert!(
        !my_profile.exists(),
        "prune --all must remove {} in the overridden root",
        my_profile.display()
    );
    assert!(
        their_profile.exists(),
        "prune --all must not reach into another home's root ({})",
        their_profile.display()
    );
}
