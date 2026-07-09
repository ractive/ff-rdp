//! CI check: ensure no `.rs` source files under `src/` use bare `String` for
//! well-known actor-ID field names.
//!
//! The scanner walks all `.rs` files under `src/` recursively and looks for
//! patterns like `actor: String` or `console_actor: String` (see
//! `ACTOR_FIELD_NAMES`).  It does **not** distinguish `pub` vs private fields —
//! any occurrence of a named actor-ID field typed as `String` is flagged.
//!
//! A small allowlist (`EXEMPT_FILE_SUFFIXES`) excludes files that legitimately
//! keep `String` actor fields: `error.rs` (RdpError/ProtocolError carry actor
//! names in error messages) and `thread.rs` (SourceInfo.actor is a source URL,
//! not an actor handle).
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

/// Return the (1-indexed line number, line) pairs that belong to **production**
/// code — i.e. everything NOT inside a `#[cfg(test)]`-gated item or block.
///
/// This handles both shapes that appear in this crate:
/// - a module-level `#[cfg(test)] mod tests { … }` (the trailing test module), and
/// - an inline `#[cfg(test)] { … }` block or `#[cfg(test)] fn helper() { … }`
///   *within* a production function (e.g. `transport.rs::trace_raw_enabled`).
///
/// When a `#[cfg(test)]` attribute is seen, the scanner tracks brace depth from
/// the next `{` and skips lines until that block closes, then resumes.  A bare
/// `#[cfg(test)]` on a `use`/`static`/`const` with no following `{` on its own
/// item is skipped for that single item line.
fn production_lines(content: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut lines = content.lines().enumerate().peekable();

    while let Some((idx, line)) = lines.next() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            // Skip forward until we open a brace, counting depth, then skip
            // until the matching close.  If the gated item has no brace before
            // its terminating `;` (e.g. `static X: … = …;`), we only skip lines
            // up to and including that item.
            let mut depth: i32 = 0;
            let mut opened = false;
            // Include the attribute line and everything in its item.
            let mut cur = Some((idx, line));
            while let Some((_, l)) = cur {
                for ch in l.chars() {
                    match ch {
                        '{' => {
                            depth += 1;
                            opened = true;
                        }
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                if opened && depth <= 0 {
                    break; // block closed
                }
                if !opened && l.trim_end().ends_with(';') {
                    break; // braceless gated item (single statement)
                }
                cur = lines.next();
            }
            continue;
        }
        out.push((idx, line.to_string()));
    }
    out
}

/// iter-102 Theme C (AC `unit_no_production_expect_in_core`): no production
/// (non-test) `.rs` source under `src/` may call `.expect(`.  Per the project's
/// error-handling rules (`CLAUDE.md`: "No `.unwrap()` / `.expect()` outside of
/// tests"), production code must use `anyhow::Context`/`?` and typed errors.
///
/// The three sites this iteration removed were:
/// - `transport.rs` `actor_send_oneway` (built the packet Map directly)
/// - `screenshot.rs` `ScreenshotArgsExt::to_args_value` (returns `Result`)
/// - `screenshot_content.rs` `capture` fallback (restructured the loop)
#[test]
fn unit_no_production_expect_in_core() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let src_dir = Path::new(manifest_dir).join("src");
    assert!(src_dir.exists(), "src/ directory not found at {src_dir:?}");

    let rs_files = collect_rs_files(&src_dir);
    let mut violations: Vec<String> = Vec::new();

    for file in &rs_files {
        let content = match std::fs::read_to_string(file) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: could not read {file:?}: {e}");
                continue;
            }
        };

        for (line_no, line) in production_lines(&content) {
            let trimmed = line.trim_start();
            // Skip comment and doc-comment lines (doc examples may mention
            // `.expect(` illustratively).
            if trimmed.starts_with("//") {
                continue;
            }
            if line.contains(".expect(") {
                violations.push(format!(
                    "{}:{}: production `.expect(` — use `?`/`ok_or_else`/typed error instead",
                    file.display(),
                    line_no + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found production `.expect(` in ff-rdp-core/src/ (must be test-only):\n{}",
        violations.join("\n")
    );
}
