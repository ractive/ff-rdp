//! iter-158 Theme D — source-scan guard: no live test may silently skip when
//! Firefox cannot be launched.
//!
//! Before iter-158 every live suite in this repo opened with
//!
//! ```ignore
//! let Some(ff) = LiveFirefox::headless_on_random_port() else {
//!     eprintln!("…: <the skip notice>");
//!     return;
//! };
//! ```
//!
//! libtest reports that early `return` as `ok`. Combined with libtest
//! discarding a passing test's stderr, a fully green live run carried **no**
//! evidence of how many of its results had reached Firefox at all — verified on
//! 2026-08-13: zero `LiveFirefox: pid=` lines in a log of 170 passing tests.
//! `live-sweep` counted every one of them as `executed`.
//!
//! This scan keeps the pattern from coming back. It is an ordinary (ungated,
//! Firefox-free) test so it runs on every `cargo test`.

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

/// The two test trees the AC names.
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

/// This file necessarily contains the very strings it forbids, so it excludes
/// itself by name rather than by contorting the needles.
fn is_self(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()) == Some("iter_158_harness_honesty.rs")
}

/// AC `unit_158_no_live_test_skips_on_missing_firefox`: zero occurrences of the
/// skip notice, and zero `else` arms binding `headless_on_random_port` to an
/// `Option`.
#[test]
fn unit_158_no_live_test_skips_on_missing_firefox() {
    // Built from fragments so this test's own source does not contain the
    // literal it forbids — belt to the `is_self` suspenders.
    let skip_notice = format!("Firefox {} available", "not");

    let mut skip_notices: Vec<String> = Vec::new();
    let mut option_binds: Vec<String> = Vec::new();

    for root in scanned_roots() {
        assert!(root.is_dir(), "expected a test tree at {}", root.display());
        for path in rust_files(&root) {
            if is_self(&path) {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (n, line) in src.lines().enumerate() {
                if line.contains(&skip_notice) {
                    skip_notices.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                }
                // `let Some(x) = …headless_on_random_port…` — the Option bind
                // that made the early return possible. Matches both the plain
                // and the `_with_args` helper, and both `LiveFirefox` and
                // `RawFirefox`.
                let t = line.trim_start();
                if t.starts_with("let Some(")
                    && line.contains("headless_on_random_port")
                    && !t.starts_with("///")
                {
                    option_binds.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                }
            }
        }
    }

    assert!(
        skip_notices.is_empty(),
        "a live test that cannot launch Firefox must FAIL, not print a skip notice and \
         return (libtest reports that as `ok`). Offending lines:\n{}",
        skip_notices.join("\n")
    );
    assert!(
        option_binds.is_empty(),
        "`headless_on_random_port` returns the launcher directly since iter-158; an \
         `Option` bind here means the silent-skip pattern is back. Offending lines:\n{}",
        option_binds.join("\n")
    );
}

/// Does `path` sit under a `tests/live/` directory? Checked by path
/// *component*, not by substring — `path.to_string_lossy().contains("tests/live")`
/// looks for a literal forward slash, which never appears in a Windows path
/// (`tests\live\...`), silently zeroing this count on that platform.
fn is_under_tests_live(path: &Path) -> bool {
    let comps: Vec<_> = path
        .components()
        .map(std::path::Component::as_os_str)
        .collect();
    comps.windows(2).any(|w| w[0] == "tests" && w[1] == "live")
}

/// The scan is only meaningful if it is actually looking at the live suites —
/// a mis-resolved root would make the assertions above vacuously true.
#[test]
fn unit_158_source_scan_covers_the_live_suites() {
    let mut live_files = 0usize;
    let mut launcher_calls = 0usize;
    for root in scanned_roots() {
        for path in rust_files(&root) {
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            if is_under_tests_live(&path) {
                live_files += 1;
            }
            launcher_calls += src.matches("headless_on_random_port").count();
        }
    }
    assert!(
        live_files >= 50,
        "expected the consolidated tests/live/ tree (~80 modules); found {live_files}"
    );
    assert!(
        launcher_calls >= 100,
        "expected ~150 launcher call sites across the live suites; found {launcher_calls}"
    );
}
