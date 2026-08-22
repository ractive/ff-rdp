//! iter-179 Theme A — source-scan guard: a live assertion that reports a
//! subprocess's `stderr` must report its `stdout` too.
//!
//! `ff-rdp` is a JSON-on-stdout tool. Its error envelopes go to **stdout**, not
//! stderr, so a test that panics with
//!
//! ```ignore
//! assert!(out.status.success(), "… exited non-zero — stderr: {stderr}");
//! ```
//!
//! ships a message with nothing after the colon. Three iterations have now paid
//! for this individually: iteration 169 fixed it for `live_158`, iteration 172
//! fixed the sibling case for `live_160`'s daemon reason, and iteration 179
//! found it a third time in `live_62_page_map_index` — where it hid an
//! `assert_network` envelope carrying `diagnostics.events_in_buffer: 0`, the
//! single most informative fact about that failure, behind an empty string.
//!
//! Rather than fix a fourth instance later, this scan makes the whole class
//! non-recurring. It is an ordinary (ungated, Firefox-free) test, so it runs on
//! every `cargo test`.

use std::path::{Path, PathBuf};

/// Recursively collect every `.rs` file under `root`.
fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// The test trees this scan covers.
fn scanned_roots() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ff-rdp-cli has a parent")
        .to_path_buf();
    vec![
        crates.join("ff-rdp-cli/tests"),
        crates.join("ff-rdp-core/tests"),
    ]
}

/// This file necessarily discusses the pattern it forbids, so it excludes
/// itself by name.
fn is_self(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()) == Some("iter_179_harness_stdout_evidence.rs")
}

/// The macros whose argument list is a panic message.
const PANIC_MACROS: [&str; 4] = ["assert!(", "assert_eq!(", "assert_ne!(", "panic!("];

/// One macro invocation, as source text, with the 1-based line it starts on.
struct Invocation {
    line: usize,
    text: String,
}

/// Extract every `assert!` / `assert_eq!` / `assert_ne!` / `panic!` invocation
/// from `src`, balanced across lines.
///
/// Parenthesis depth is tracked outside string literals only, so a `)` inside a
/// message (`"… (CSP still blocking?)"`) does not truncate the invocation and
/// hide the rest of its arguments from the scan.
fn panic_invocations(src: &str) -> Vec<Invocation> {
    let bytes: Vec<char> = src.chars().collect();
    // Precompute the 1-based line number of every character index.
    let mut line_of = Vec::with_capacity(bytes.len());
    let mut line = 1usize;
    for &c in &bytes {
        line_of.push(line);
        if c == '\n' {
            line += 1;
        }
    }

    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let Some(open) = PANIC_MACROS.iter().find_map(|needle| {
            let n: Vec<char> = needle.chars().collect();
            if i + n.len() <= bytes.len() && bytes[i..i + n.len()] == n[..] {
                // `debug_assert!` / `xyz_assert!` must not match `assert!(`.
                let prev_ok = i == 0 || !(bytes[i - 1].is_alphanumeric() || bytes[i - 1] == '_');
                prev_ok.then_some(i + n.len())
            } else {
                None
            }
        }) else {
            i += 1;
            continue;
        };

        let start_line = line_of[i];
        let mut j = open;
        let mut depth = 1usize;
        let mut in_str = false;
        let mut escaped = false;
        while j < bytes.len() && depth > 0 {
            let c = bytes[j];
            if in_str {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_str = false;
                }
            } else {
                match c {
                    '"' => in_str = true,
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
            }
            j += 1;
        }
        out.push(Invocation {
            line: start_line,
            text: bytes[i..j.min(bytes.len())].iter().collect(),
        });
        i = open;
    }
    out
}

/// AC `unit_179_no_assertion_reports_stderr_without_stdout`: every panic
/// message that names `stderr` also names `stdout`.
#[test]
fn unit_179_no_assertion_reports_stderr_without_stdout() {
    let mut offenders: Vec<String> = Vec::new();
    let mut scanned = 0usize;

    for root in scanned_roots() {
        assert!(root.is_dir(), "expected a test tree at {}", root.display());
        for path in rust_files(&root) {
            if is_self(&path) {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            for inv in panic_invocations(&src) {
                scanned += 1;
                if inv.text.contains("stderr") && !inv.text.contains("stdout") {
                    let head: String = inv.text.chars().take(120).collect();
                    offenders.push(format!(
                        "{}:{}: {}",
                        path.display(),
                        inv.line,
                        head.replace('\n', " ")
                    ));
                }
            }
        }
    }

    assert!(
        scanned >= 500,
        "the scan must actually reach the assertions: only {scanned} panic-macro \
         invocations found across the test trees"
    );
    assert!(
        offenders.is_empty(),
        "ff-rdp writes its error envelopes to STDOUT, so a failure message that \
         interpolates only `stderr` ships empty. Every assertion naming `stderr` must \
         name `stdout` too. {} offending invocation(s):\n{}",
        offenders.len(),
        offenders.join("\n")
    );
}
