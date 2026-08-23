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

/// The test trees this scan covers: both live tiers, the e2e (mock-server)
/// tier, and the core crate's own tests.
///
/// `crates/ff-rdp-cli/tests/e2e/` has its own `support` module rather than
/// the live tier's `common`, so its `output_note` equivalent lives in
/// `tests/e2e/support/mod.rs` rather than `tests/common/mod.rs`. Iteration
/// 179 fixed the two live roots; iteration 182 added `tests/e2e` here after
/// fixing its 236 offending invocations (see
/// [[iteration-182-e2e-tier-stdout-evidence]]).
fn scanned_roots() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ff-rdp-cli has a parent")
        .to_path_buf();
    vec![
        crates.join("ff-rdp-cli/tests/live"),
        crates.join("ff-rdp-cli/tests/e2e"),
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

/// Per-character classification of Rust source.
///
/// The scan needs to know which characters are *live code*, because a `)` in a
/// comment or a string literal must not close a macro invocation. Getting this
/// wrong is not a harmless approximation: an invocation whose end is
/// mis-detected runs on into the following statements, picks up an unrelated
/// `stdout` mention, and the offender it was supposed to catch is silently
/// skipped. The live tree currently holds 210 raw-string literals and 406
/// comment lines containing a double quote, so all three cases are live.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lex {
    /// Ordinary code — parens and macro names count here.
    Code,
    /// Inside a comment or a literal — nothing counts.
    Skip,
}

/// Classify every character of `chars` as [`Lex::Code`] or [`Lex::Skip`].
///
/// Handles line comments, nested block comments, normal string literals with
/// backslash escapes, raw strings (`r"…"`, `r#"…"#`, any hash count) and char
/// literals. Lifetimes (`'static`) are deliberately *not* treated as char
/// literals — a bare `'` followed by an identifier is left as code.
fn classify(chars: &[char]) -> Vec<Lex> {
    let mut out = vec![Lex::Code; chars.len()];
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();

        // Line comment.
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                out[i] = Lex::Skip;
                i += 1;
            }
            continue;
        }
        // Block comment, nested.
        if c == '/' && next == Some('*') {
            let mut depth = 1usize;
            out[i] = Lex::Skip;
            out[i + 1] = Lex::Skip;
            i += 2;
            while i < chars.len() && depth > 0 {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    out[i] = Lex::Skip;
                    out[i + 1] = Lex::Skip;
                    i += 2;
                    continue;
                }
                if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    out[i] = Lex::Skip;
                    out[i + 1] = Lex::Skip;
                    i += 2;
                    continue;
                }
                out[i] = Lex::Skip;
                i += 1;
            }
            continue;
        }
        // Raw string: r, then any number of #, then ".
        if c == 'r' {
            let mut j = i + 1;
            let mut hashes = 0usize;
            while chars.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if chars.get(j) == Some(&'"') {
                // `r` and the opening delimiter are not code.
                out[i..=j].fill(Lex::Skip);
                let mut k = j + 1;
                loop {
                    if k >= chars.len() {
                        break;
                    }
                    if chars[k] == '"' {
                        let closes = (1..=hashes).all(|h| chars.get(k + h) == Some(&'#'));
                        if closes {
                            out[k..=(k + hashes).min(chars.len() - 1)].fill(Lex::Skip);
                            k += hashes + 1;
                            break;
                        }
                    }
                    out[k] = Lex::Skip;
                    k += 1;
                }
                i = k;
                continue;
            }
        }
        // Normal string literal.
        if c == '"' {
            out[i] = Lex::Skip;
            let mut k = i + 1;
            while k < chars.len() {
                if chars[k] == '\\' {
                    out[k] = Lex::Skip;
                    if k + 1 < chars.len() {
                        out[k + 1] = Lex::Skip;
                    }
                    k += 2;
                    continue;
                }
                out[k] = Lex::Skip;
                if chars[k] == '"' {
                    k += 1;
                    break;
                }
                k += 1;
            }
            i = k;
            continue;
        }
        // Char literal — `'x'` or `'\n'`, but never a lifetime.
        if c == '\'' {
            let is_escaped = next == Some('\\');
            let close_at = if is_escaped { i + 3 } else { i + 2 };
            if chars.get(close_at) == Some(&'\'') {
                out[i..=close_at].fill(Lex::Skip);
                i = close_at + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Extract every `assert!` / `assert_eq!` / `assert_ne!` / `panic!` invocation
/// from `src`, balanced across lines.
///
/// Both the macro-name search and the parenthesis counting run only over
/// [`Lex::Code`] characters, so neither a `)` inside a message
/// (`"… (CSP still blocking?)"`) nor a commented-out `assert!(` can throw the
/// scan off.
fn panic_invocations(src: &str) -> Vec<Invocation> {
    let chars: Vec<char> = src.chars().collect();
    let lex = classify(&chars);

    // Precompute the 1-based line number of every character index.
    let mut line_of = Vec::with_capacity(chars.len());
    let mut line = 1usize;
    for &c in &chars {
        line_of.push(line);
        if c == '\n' {
            line += 1;
        }
    }

    let mut out = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if lex[i] != Lex::Code {
            i += 1;
            continue;
        }
        let Some(open) = PANIC_MACROS.iter().find_map(|needle| {
            let n: Vec<char> = needle.chars().collect();
            if i + n.len() <= chars.len() && chars[i..i + n.len()] == n[..] {
                // `debug_assert!` / `xyz_assert!` must not match `assert!(`.
                let prev_ok = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
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
        while j < chars.len() && depth > 0 {
            if lex[j] == Lex::Code {
                match chars[j] {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
            }
            j += 1;
        }
        out.push(Invocation {
            line: start_line,
            text: chars[i..j.min(chars.len())].iter().collect(),
        });
        i = open;
    }
    out
}

/// The lexer is the load-bearing part of this guard, so pin it against the
/// three shapes that would otherwise produce silent false negatives.
#[test]
fn unit_179_lexer_ignores_parens_in_comments_strings_and_raw_strings() {
    // A `)` in a line comment must not close the invocation early — if it did,
    // the `stdout` on the following line would be missed and this would be
    // reported as an offender.
    let src = r#"
fn f() {
    assert!(
        ok, // a stray ) in a comment
        "stderr: {}", note(&out.stdout)
    );
}
"#;
    let invs = panic_invocations(src);
    assert_eq!(invs.len(), 1, "expected exactly one invocation");
    assert!(invs[0].text.contains("stdout"), "{}", invs[0].text);

    // A raw string holding an unbalanced paren must not unbalance the scan.
    let src = r##"
fn f() {
    assert!(x, "stderr {}", r#"unbalanced ( paren"#);
    let after_stdout = 1;
}
"##;
    let invs = panic_invocations(src);
    assert_eq!(invs.len(), 1);
    assert!(
        !invs[0].text.contains("after_stdout"),
        "the invocation ran past its own closing paren: {}",
        invs[0].text
    );

    // A commented-out macro is not an invocation at all.
    let src = "fn f() {\n    // assert!(nope, \"stderr\");\n    let x = 1;\n}\n";
    assert!(panic_invocations(src).is_empty(), "{src}");
}

/// Does this invocation report `stderr` without reporting `stdout`?
///
/// `output_note` carries both streams by construction, so it satisfies the rule
/// without the literal word `stdout` appearing at the call site.
fn is_offender(text: &str) -> bool {
    text.contains("stderr") && !text.contains("stdout") && !text.contains("output_note(")
}

/// Positive control. Without this, any bug that made the scan return nothing —
/// a mis-resolved root, a lexer that swallows every invocation — would present
/// as a clean pass, which is the exact failure mode this file exists to
/// prevent elsewhere.
#[test]
fn unit_179_the_scan_actually_flags_a_known_offender() {
    let bad = r#"
fn f() {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "run failed — stderr: {stderr}");
}
"#;
    let invs = panic_invocations(bad);
    assert_eq!(invs.len(), 1, "expected one invocation in the fixture");
    assert!(
        is_offender(&invs[0].text),
        "the known-bad shape must be flagged: {}",
        invs[0].text
    );

    // …and the two accepted repairs must both clear it.
    let with_stdout = r#"assert!(ok, "failed — stdout={stdout} stderr={stderr}");"#;
    assert!(!is_offender(&panic_invocations(with_stdout)[0].text));

    let with_note = r#"assert!(ok, "failed — {}", crate::common::output_note(&out));"#;
    let inv = &panic_invocations(with_note)[0].text;
    assert!(
        !is_offender(inv),
        "output_note must satisfy the rule: {inv}"
    );
}

/// AC `unit_179_no_assertion_reports_stderr_without_stdout`: every panic
/// message that names `stderr` also names `stdout`.
#[test]
fn unit_179_no_assertion_reports_stderr_without_stdout() {
    let mut offenders: Vec<String> = Vec::new();
    let mut per_root: Vec<(String, usize)> = Vec::new();

    for root in scanned_roots() {
        assert!(root.is_dir(), "expected a test tree at {}", root.display());
        let mut root_scanned = 0usize;
        for path in rust_files(&root) {
            if is_self(&path) {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            for inv in panic_invocations(&src) {
                root_scanned += 1;
                if is_offender(&inv.text) {
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
        per_root.push((root.display().to_string(), root_scanned));
    }

    // Per-tier floors, not just a global one. A single combined floor (the
    // iter-179 original: `scanned >= 1200`) can hide a per-tier regression:
    // one tree could lose most of its invocations to a lexer desync while
    // another tree's volume papers over it in the sum. Measured on this
    // branch (iteration 182, after widening scanned_roots to include e2e):
    // live 1290, e2e 1258, core 168 — each floor sits ~10% below its
    // measured count, tight enough that a desync swallowing a meaningful
    // fraction of a tree's invocations still trips its own assertion, not
    // just the global total.
    const MIN_PER_ROOT: [(&str, usize); 3] = [
        ("ff-rdp-cli/tests/live", 1150),
        ("ff-rdp-cli/tests/e2e", 1120),
        ("ff-rdp-core/tests", 150),
    ];
    for (label, min) in MIN_PER_ROOT {
        let found = per_root
            .iter()
            .find(|(name, _)| name.ends_with(label))
            .unwrap_or_else(|| panic!("scanned_roots must include a root ending in {label}"));
        assert!(
            found.1 >= min,
            "the scan must actually reach the assertions in {label}: only {} panic-macro \
             invocations found (expected at least {min}) — per-root counts: {per_root:?}",
            found.1
        );
    }
    assert!(
        offenders.is_empty(),
        "ff-rdp writes its error envelopes to STDOUT, so a failure message that \
         interpolates only `stderr` ships empty. Every assertion naming `stderr` must \
         name `stdout` too. {} offending invocation(s):\n{}",
        offenders.len(),
        offenders.join("\n")
    );
}
