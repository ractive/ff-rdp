//! iter-242 Part B — source-scan guard: every `ff-rdp launch` the live suite
//! spawns must be owned.
//!
//! Iteration 151 audited this seam by searching for *discarded guards*
//! (`ManuallyDrop`, `mem::forget`) and so missed the `launch --replace` class
//! entirely — a Firefox nothing ever owned cannot show up in a search for
//! guards that were thrown away. The meta-lesson its PR review recorded, and
//! the one this scan implements: **enumerate launch sites, not guard sites.**
//!
//! Two mechanical properties are required of every `"launch"` invocation under
//! `crates/ff-rdp-cli/tests/live/`:
//!
//! 1. It is built from `common::ff_rdp_launch_command[_for]()`, never a bare
//!    `Command::new(ff_rdp_bin())`. That helper pre-sets `FF_RDP_LIVE_TEST_NAME`,
//!    which is what makes a leaked profile name the test that spawned it
//!    (`.ff-rdp-owner-test`) instead of reading `spawned by unknown test` —
//!    the attribution iteration 151 Theme A was supposed to deliver suite-wide
//!    and delivered only for `common::LiveFirefox`.
//! 2. The enclosing function binds an RAII owner for the resulting PID —
//!    `common::FirefoxGuard` or `common::LiveFirefox` — or hands one back to
//!    its caller in its return type. A function that parses a PID out of
//!    `launch`'s JSON and then asserts on it, with nothing that reaps that PID
//!    on unwind, leaks one Firefox per failing run.
//!
//! This is a source scan, not a live test: it is Firefox-free and runs on
//! every `cargo test`. It cannot prove a guard is constructed *before* the
//! next assertion — only a reviewer can — but it does prove no launch site is
//! missing an owner outright, which is the failure iteration 151 shipped.

use std::path::{Path, PathBuf};

/// Every `.rs` file under `crates/ff-rdp-cli/tests/live/`.
fn live_test_files() -> Vec<PathBuf> {
    let live_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/live");
    let mut out = Vec::new();
    let mut stack = vec![live_root];
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

/// `true` if `"launch"` appears on `line` as a real argument token rather than
/// inside a line comment.
///
/// Deliberately crude — a doc comment mentioning `"launch"` is the only case
/// that matters here, and every occurrence this misclassifies would only make
/// the scan stricter, never quieter.
fn mentions_launch_arg(line: &str) -> bool {
    let code = match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    };
    code.contains("\"launch\"")
}

/// The 0-based index of the `fn` line enclosing `line_idx`, if any.
///
/// Walks back to the nearest line whose first non-whitespace token starts a
/// function (`fn`, `pub fn`, `async fn`, …). Closures are not functions here:
/// a closure body's launch site belongs to the `fn` that contains it, which is
/// also where its guard has to live.
fn enclosing_fn(lines: &[&str], line_idx: usize) -> Option<usize> {
    (0..=line_idx).rev().find(|&i| {
        let t = lines[i].trim_start();
        t.starts_with("fn ") || t.starts_with("pub fn ") || t.starts_with("async fn ")
    })
}

/// The source text of the function starting at `fn_line`, up to (but not
/// including) the next function at the same or shallower indentation.
fn fn_body<'a>(lines: &[&'a str], fn_line: usize) -> Vec<&'a str> {
    let indent = lines[fn_line].len() - lines[fn_line].trim_start().len();
    let mut out = vec![lines[fn_line]];
    for line in &lines[fn_line + 1..] {
        let trimmed = line.trim_start();
        let this_indent = line.len() - trimmed.len();
        let starts_fn = trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("async fn ");
        if starts_fn && this_indent <= indent {
            break;
        }
        out.push(line);
    }
    out
}

/// Names that constitute an RAII owner for a launched Firefox PID: either the
/// function binds one, or its signature hands one to its caller.
///
/// `guard_launched_firefox` is the preferred form — it extracts the PID and
/// binds the guard in one step, so no fallible parsing sits between the spawn
/// and the reaper (iter-242 Theme B).
const OWNERSHIP_TOKENS: [&str; 3] = ["FirefoxGuard", "LiveFirefox", "guard_launched_firefox"];

/// Launch sites that are deliberately unowned, each with the reason.
///
/// Kept empty on purpose. An entry here is a standing leak, so adding one is a
/// decision to be argued in review, not a way to quiet the scan — the two
/// entries iteration 242 would have needed (`live_90`'s `launch_on_port` and
/// `live_142_disk_growth`'s `launch_headless`) were fixed instead.
const ALLOWED_UNOWNED: [(&str, &str); 0] = [];

fn is_allowed(file: &str, reason_out: &mut Option<&'static str>) -> bool {
    for (name, reason) in ALLOWED_UNOWNED {
        if name == file {
            *reason_out = Some(reason);
            return true;
        }
    }
    false
}

/// AC (iter-242 Part B): no `"launch"` invocation under `tests/live/` is
/// spawned without an owner-test marker, and none sits in a function with no
/// RAII owner for the PID it produces.
#[test]
fn iter_242_every_live_launch_site_is_owned_and_attributed() {
    let mut violations: Vec<String> = Vec::new();

    for path in live_test_files() {
        let file = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_owned();
        let src = std::fs::read_to_string(&path).expect("read live test source");
        let lines: Vec<&str> = src.lines().collect();

        for (idx, line) in lines.iter().enumerate() {
            if !mentions_launch_arg(line) {
                continue;
            }
            let Some(fn_line) = enclosing_fn(&lines, idx) else {
                violations.push(format!(
                    "{file}:{}: `\"launch\"` outside any function",
                    idx + 1
                ));
                continue;
            };
            let body = fn_body(&lines, fn_line);
            let body_text = body.join("\n");

            // Property 1: routed through the marker-setting helper. A file
            // whose launch args are assembled in one place and handed to a
            // local spawn helper satisfies this through that helper, so the
            // whole file is searched rather than the enclosing function only.
            if !src.contains("ff_rdp_launch_command") {
                violations.push(format!(
                    "{file}:{}: spawns `launch` without `common::ff_rdp_launch_command()` — a \
                     profile leaked here would record `spawned by unknown test`",
                    idx + 1
                ));
            }

            // Property 2: an RAII owner is bound in, or returned by, the
            // enclosing function.
            //
            // Four live modules carry their own private `LiveFirefox` clone
            // (`live_oneway`, `live_target_destroyed`, `live_cross_actor`,
            // `live_61l`). Their `fn launch() -> Self` is itself the RAII
            // owner — the launched PID becomes a field of a type with a
            // `Drop` — so a constructor that hands `Self` back to its caller
            // in a file that implements `Drop` counts as owned.
            let hands_back_self =
                lines[fn_line].contains("-> Self") || lines[fn_line].contains("-> Option<Self>");
            let owned = OWNERSHIP_TOKENS.iter().any(|t| body_text.contains(t))
                || (hands_back_self && src.contains("impl Drop for"));
            let mut reason = None;
            if !owned && !is_allowed(&file, &mut reason) {
                violations.push(format!(
                    "{file}:{}: `\"launch\"` in `{}` binds no FirefoxGuard/LiveFirefox — the \
                     PID it starts is owned by nothing and survives a panicking assertion",
                    idx + 1,
                    lines[fn_line].trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "unowned or unattributed live launch sites ({}):\n  {}",
        violations.len(),
        violations.join("\n  ")
    );
}

/// The scan is only worth anything if it actually sees the launch sites. A
/// refactor that renames the argument, moves the tree, or breaks
/// `enclosing_fn` would otherwise turn this file into a test that passes by
/// finding nothing.
#[test]
fn iter_242_launch_site_scan_sees_the_launch_sites() {
    let mut sites = 0usize;
    for path in live_test_files() {
        let src = std::fs::read_to_string(&path).expect("read live test source");
        sites += src.lines().filter(|l| mentions_launch_arg(l)).count();
    }
    assert!(
        sites >= 20,
        "expected the live tree to hold at least 20 `\"launch\"` invocations, found {sites} — \
         the scan is probably looking at the wrong tree or matching the wrong token"
    );
}
