//! CI check: ensure no public struct fields use bare `String` for actor IDs.
//!
//! We look for patterns like `actor: String` or `actor_id: String` in the
//! `actors/` and `fronts/` subdirectories of `src/` — places where untyped
//! actor handles would be a regression.  Error types (`error.rs`) and the
//! registry module intentionally keep some `String` fields for protocol error
//! messages and are excluded.
//!
//! Run automatically as part of `cargo test --workspace`.

use std::path::Path;

/// Field names that must NOT appear as bare `String` fields in actor structs.
const ACTOR_FIELD_NAMES: &[&str] = &[
    "actor",
    "actor_id",
    "console_actor",
    "consoleActor",
    "thread_actor",
    "inspector_actor",
    "watcher_actor",
    "screenshot_actor",
    "accessibility_actor",
    "responsive_actor",
];

/// Files that are exempted from this check (error variants, raw protocol structs).
const EXEMPT_FILE_SUFFIXES: &[&str] = &[
    "error.rs",  // RdpError/ProtocolError use actor: String in error messages
    "thread.rs", // SourceInfo.actor is a source URL, not an actor handle
];

/// Patterns we look for (field_name: String).
fn forbidden_patterns() -> Vec<String> {
    ACTOR_FIELD_NAMES
        .iter()
        .flat_map(|name| [format!("{name}: String"), format!("{name}:String")])
        .collect()
}

fn collect_rs_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(collect_rs_files(&path));
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out
}

fn is_exempt(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    EXEMPT_FILE_SUFFIXES
        .iter()
        .any(|suffix| path_str.ends_with(suffix))
}

#[test]
fn no_bare_string_actor_ids_in_src() {
    // Locate the `src/` directory relative to this test file's manifest dir.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let src_dir = Path::new(manifest_dir).join("src");

    assert!(
        src_dir.exists(),
        "src/ directory not found at {src_dir:?} — adjust CARGO_MANIFEST_DIR usage"
    );

    let patterns = forbidden_patterns();
    let rs_files = collect_rs_files(&src_dir);

    let mut violations: Vec<String> = Vec::new();

    for file in &rs_files {
        if is_exempt(file) {
            continue;
        }

        let content = match std::fs::read_to_string(file) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: could not read {file:?}: {e}");
                continue;
            }
        };

        for (line_no, line) in content.lines().enumerate() {
            // Skip comment lines.
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }

            for pattern in &patterns {
                if line.contains(pattern.as_str()) {
                    violations.push(format!(
                        "{}:{}: found `{pattern}` — use `ActorId` instead of `String`",
                        file.display(),
                        line_no + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found bare String actor ID fields in ff-rdp-core/src/:\n{}",
        violations.join("\n")
    );
}
