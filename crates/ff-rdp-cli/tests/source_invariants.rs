//! Two source-scanning invariants over `crates/ff-rdp-cli/src/commands/`.
//!
//! Formerly `cargo run -p xtask -- check-source-invariants` (iter-145 and
//! iter-148, merged in iter-162a); a plain `cargo test` since DEC-058.
//!
//! - **error-envelope-paths** — no `eprintln!` followed within a few lines by
//!   `AppError::Exit(N)`. `main` treats that variant as "already printed, just
//!   exit", so a scripted consumer parsing stdout for `--jq '.error_type'` gets
//!   nothing — the bypass that let click-time JS exceptions skip the JSON
//!   error envelope.
//! - **stderr-annotations** — every `eprintln!` under `commands/` carries a
//!   `// stderr-ok: <reason>` comment on its line or within the two above.
//!
//! Both skip everything from the first `#[cfg(test)]` onward.

use std::path::{Path, PathBuf};

const LOOKAHEAD_LINES: usize = 6;
const LOOKBACK_JUSTIFICATION_LINES: usize = 2;
const JUSTIFICATION_MARKER: &str = "// stderr-ok:";

/// Strip everything from the first `#[cfg(test)]` onward — a heuristic, good
/// enough because every file in `commands/` puts its test module last.
fn strip_test_module(src: &str) -> &str {
    src.find("#[cfg(test)]").map_or(src, |idx| &src[..idx])
}

/// True if a `// stderr-ok:` comment sits on line `idx` or the two above it.
fn is_justified(lines: &[&str], idx: usize) -> bool {
    (idx.saturating_sub(LOOKBACK_JUSTIFICATION_LINES)..=idx)
        .filter_map(|i| lines.get(i))
        .any(|l| l.contains(JUSTIFICATION_MARKER))
}

/// 1-based line numbers of unjustified `eprintln!` sites followed by
/// `AppError::Exit(` within [`LOOKAHEAD_LINES`].
fn envelope_bypasses(src: &str) -> Vec<usize> {
    let lines: Vec<&str> = strip_test_module(src).lines().collect();
    (0..lines.len())
        .filter(|&idx| lines[idx].contains("eprintln!") && !is_justified(&lines, idx))
        .filter(|&idx| {
            let end = (idx + 1 + LOOKAHEAD_LINES).min(lines.len());
            lines[idx + 1..end]
                .iter()
                .any(|l| l.contains("AppError::Exit("))
        })
        .map(|idx| idx + 1)
        .collect()
}

/// 1-based line numbers of `eprintln!` sites missing their justification.
fn unannotated_eprintlns(src: &str) -> Vec<usize> {
    let lines: Vec<&str> = strip_test_module(src).lines().collect();
    (0..lines.len())
        .filter(|&idx| lines[idx].contains("eprintln!") && !is_justified(&lines, idx))
        .map(|idx| idx + 1)
        .collect()
}

/// Every `.rs` file under `dir`, recursively, sorted.
fn rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|e| e.expect("dir entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            out.extend(rs_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    out
}

fn commands_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("commands")
}

/// Run `scan` over every file in `commands/` and return `path:line: text`
/// findings, asserting the walk actually found files.
fn scan_commands(scan: fn(&str) -> Vec<usize>) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = rs_files(&commands_dir());
    assert!(
        !files.is_empty(),
        "no .rs files found under {}",
        commands_dir().display()
    );
    let mut findings = Vec::new();
    for path in &files {
        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let lines: Vec<&str> = src.lines().collect();
        for line in scan(&src) {
            let label = path.strip_prefix(root).unwrap_or(path);
            findings.push(format!(
                "{}:{line}: {}",
                label.display(),
                lines.get(line - 1).map_or("", |l| l.trim())
            ));
        }
    }
    findings
}

#[test]
fn error_envelope_paths() {
    let findings = scan_commands(envelope_bypasses);
    assert!(
        findings.is_empty(),
        "error-envelope-paths: {} site(s) print with eprintln! and then return \
         AppError::Exit(N), bypassing the JSON error envelope.\n  Route the error \
         through the standard AppError envelope (AppError::User / AppError::Timeout), \
         or add a `// stderr-ok: <reason>` comment on/above the eprintln! if this \
         one is deliberate.\n  {}",
        findings.len(),
        findings.join("\n  ")
    );
}

#[test]
fn stderr_annotations() {
    let findings = scan_commands(unannotated_eprintlns);
    assert!(
        findings.is_empty(),
        "stderr-annotations: {} eprintln! site(s) under commands/ lack a \
         `// stderr-ok: <reason>` comment on the line or within the two lines \
         above it, classifying why the site legitimately writes to stderr (see \
         kb/iterations/iteration-148-stderr-path-annotations.md).\n  {}",
        findings.len(),
        findings.join("\n  ")
    );
}

#[test]
fn envelope_scan_catches_the_click_rs_shape_and_honours_the_marker() {
    let bad =
        "fn f() {\n    eprintln!(\"error: {}\", msg);\n    return Err(AppError::Exit(1));\n}\n";
    assert_eq!(envelope_bypasses(bad), vec![2]);

    let marked = "fn f() {\n    // stderr-ok: deliberate\n    eprintln!(\"e\");\n    return Err(AppError::Exit(1));\n}\n";
    assert!(envelope_bypasses(marked).is_empty());

    let far = format!(
        "fn f() {{\n    eprintln!(\"d\");\n{}    return Err(AppError::Exit(1));\n}}\n",
        "    let _x = 1;\n".repeat(LOOKAHEAD_LINES)
    );
    assert!(envelope_bypasses(&far).is_empty());
}

#[test]
fn annotation_scan_window_and_test_module_rules() {
    assert_eq!(
        unannotated_eprintlns("fn f() {\n    eprintln!(\"w\");\n}\n"),
        vec![2]
    );
    assert!(unannotated_eprintlns("    eprintln!(\"w\"); // stderr-ok: (b)\n").is_empty());
    assert!(unannotated_eprintlns("// stderr-ok: (b)\nlet a = 1;\neprintln!(\"w\");\n").is_empty());
    // Three lines above is out of the window.
    assert_eq!(
        unannotated_eprintlns("// stderr-ok: (b)\nlet a = 1;\nlet b = 2;\neprintln!(\"w\");\n"),
        vec![4]
    );
    // Marker text inside a string literal is not a comment.
    assert_eq!(
        unannotated_eprintlns("let m = \"stderr-ok: no\";\neprintln!(\"{m}\");\n"),
        vec![2]
    );
    assert!(
        unannotated_eprintlns("fn f() {}\n#[cfg(test)]\nmod t { fn g() { eprintln!(\"x\"); } }\n")
            .is_empty()
    );
}
