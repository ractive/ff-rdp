use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use regex::Regex;
use std::collections::HashSet;
use std::process::Command;

#[derive(ClapArgs)]
pub struct Args {
    /// Git ref to diff against (default: origin/main, fallback: main).
    #[arg(long, default_value = "")]
    since: String,
}

/// A public symbol declaration found in a diff.
#[derive(Debug, PartialEq, Eq)]
pub struct PubDecl {
    pub file: String,
    pub line: usize,
    pub kind: String,
    pub name: String,
}

/// Resolve the effective git ref to diff against.
fn resolve_ref(since: &str) -> String {
    if !since.is_empty() {
        return since.to_owned();
    }
    // Try origin/main first, fall back to main.
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "origin/main"])
        .output();
    match output {
        Ok(o) if o.status.success() => "origin/main".to_owned(),
        _ => "main".to_owned(),
    }
}

/// Return the list of changed `.rs` files in `crates/` since `git_ref`.
pub fn changed_rs_files(git_ref: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args([
            "diff",
            "--name-only",
            &format!("{git_ref}...HEAD"),
            "--",
            "crates/**/*.rs",
        ])
        .output()
        .context("failed to run git diff --name-only")?;

    if !output.status.success() {
        // Fallback: try without glob quoting issues (some shells strip quotes)
        let output2 = Command::new("git")
            .args(["diff", "--name-only", &format!("{git_ref}...HEAD")])
            .output()
            .context("failed to run git diff --name-only (fallback)")?;
        let text = String::from_utf8_lossy(&output2.stdout);
        return Ok(text
            .lines()
            .filter(|l| l.starts_with("crates/") && l.ends_with(".rs"))
            .map(str::to_owned)
            .collect());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Parse a unified diff for a single file and extract new `pub` declarations.
/// Skips files under `tests/` or `benches/` directories.
pub fn extract_pub_decls(file: &str, diff_text: &str) -> Vec<PubDecl> {
    // Skip test/bench files — we only care about library/binary code.
    if file.contains("/tests/") || file.contains("/benches/") || file.ends_with("_test.rs") {
        return vec![];
    }
    // Skip xtask itself.
    if file.contains("crates/xtask/") {
        return vec![];
    }

    // Match `+` lines (added lines in diff) that declare a new pub item.
    // We skip `+++` header lines in the loop body below.
    let pub_re = Regex::new(
        r"^\+\s*pub\s+(?:async\s+)?(fn|struct|enum|trait|mod|type)\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
    .expect("static regex");

    let mut decls = Vec::new();
    // Track current line number in new file from hunk headers.
    let mut new_line: usize = 0;

    for raw_line in diff_text.lines() {
        // Parse hunk header: @@ -a,b +c,d @@
        if raw_line.starts_with("@@") {
            if let Some(new_line_num) = parse_hunk_new_start(raw_line) {
                new_line = new_line_num;
            }
            continue;
        }
        // Skip diff headers: `--- a/file` and `+++ b/file`
        if raw_line.starts_with("---") || raw_line.starts_with("+++") {
            continue;
        }
        // Lines starting with '-' don't advance new file line counter.
        if raw_line.starts_with('-') {
            continue;
        }
        // Advance counter for context and added lines.
        if raw_line.starts_with('+') || raw_line.starts_with(' ') {
            if raw_line.starts_with('+') {
                // Check for pub declaration.
                if let Some(caps) = pub_re.captures(raw_line) {
                    let kind = caps[1].to_owned();
                    let name = caps[2].to_owned();
                    decls.push(PubDecl {
                        file: file.to_owned(),
                        line: new_line,
                        kind,
                        name,
                    });
                }
            }
            new_line += 1;
        }
    }

    decls
}

fn parse_hunk_new_start(hunk_header: &str) -> Option<usize> {
    // Format: @@ -old_start[,old_count] +new_start[,new_count] @@
    let re = Regex::new(r"\+(\d+)(?:,\d+)?\s+@@").expect("static regex");
    re.captures(hunk_header)
        .and_then(|c| c[1].parse::<usize>().ok())
}

/// Search workspace (minus xtask) for uses of `symbol_name`.
/// Returns true if at least one non-test consumer exists.
pub fn has_non_test_consumer(symbol_name: &str, _declaring_file: &str) -> Result<bool> {
    let output = Command::new("rg")
        .args([
            "--type",
            "rust",
            "--no-heading",
            "-n",
            symbol_name,
            "crates/",
        ])
        .output()
        .context("failed to run rg (is ripgrep installed?)")?;

    if !output.status.success() && !output.stdout.is_empty() {
        // rg exits 1 when no matches; treat any other non-success with no stdout
        // as a real failure below.
    }
    // rg returns exit 1 when there are zero matches (which is meaningful: no consumers).
    // Treat exit codes >= 2 as hard errors so missing rg or invocation bugs don't
    // silently let dead symbols pass.
    if let Some(code) = output.status.code()
        && code >= 2
    {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("rg failed (exit {code}) while searching for '{symbol_name}': {stderr}");
    }

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        // Extract the file path from rg output (format: "path:line:content")
        let file_path = line.split(':').next().unwrap_or("");

        // Skip xtask.
        if file_path.contains("crates/xtask/") {
            continue;
        }
        // Skip test files.
        if file_path.contains("/tests/") || file_path.contains("/benches/") {
            continue;
        }
        // Skip lines that look like declarations (not consumers).
        // This filters the declaration line itself even when it lives in the
        // declaring file — so same-file callers below still count as consumers.
        let content_part = line.splitn(3, ':').nth(2).unwrap_or("");
        let trimmed = content_part.trim();
        if trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub struct ")
            || trimmed.starts_with("pub enum ")
            || trimmed.starts_with("pub trait ")
            || trimmed.starts_with("pub mod ")
            || trimmed.starts_with("pub type ")
            || trimmed.starts_with("pub async fn ")
        {
            continue;
        }

        return Ok(true);
    }

    Ok(false)
}

pub fn run(args: Args) -> Result<()> {
    let git_ref = resolve_ref(&args.since);
    let files = changed_rs_files(&git_ref)?;

    if files.is_empty() {
        eprintln!("check-dead-primitives: no changed .rs files in crates/ since {git_ref}");
        return Ok(());
    }

    let mut findings: Vec<String> = Vec::new();
    // Dedup by (file, name) so distinct same-named pub items in different
    // modules are each checked.
    let mut seen: HashSet<(String, String)> = HashSet::new();

    for file in &files {
        let diff_output = Command::new("git")
            .args(["diff", &format!("{git_ref}...HEAD"), "--", file])
            .output()
            .with_context(|| format!("failed to get diff for {file}"))?;

        if !diff_output.status.success() {
            let stderr = String::from_utf8_lossy(&diff_output.stderr);
            anyhow::bail!("git diff failed for {file}: {stderr}");
        }

        let diff_text = String::from_utf8_lossy(&diff_output.stdout);
        let decls = extract_pub_decls(file, &diff_text);

        for decl in decls {
            if !seen.insert((decl.file.clone(), decl.name.clone())) {
                continue;
            }

            let has_consumer = has_non_test_consumer(&decl.name, file)
                .with_context(|| format!("consumer search failed for {}", decl.name))?;
            if !has_consumer {
                findings.push(format!(
                    "{}:{}: pub {} {} has no non-test consumers",
                    decl.file, decl.line, decl.kind, decl.name
                ));
            }
        }
    }

    if findings.is_empty() {
        return Ok(());
    }

    for f in &findings {
        eprintln!("{f}");
    }
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_pub_decls_basic() {
        let diff = r#"@@ -1,3 +1,10 @@
+pub fn hello_world() -> String {
+    "hello".to_owned()
+}
+pub struct MyConfig {
+    pub field: u32,
+}
+pub enum Direction { Up, Down }
+pub trait Runnable { fn run(&self); }
+pub mod utils {}
"#;
        let decls = extract_pub_decls("crates/foo/src/lib.rs", diff);
        assert_eq!(decls.len(), 5);
        assert_eq!(decls[0].name, "hello_world");
        assert_eq!(decls[0].kind, "fn");
        assert_eq!(decls[1].name, "MyConfig");
        assert_eq!(decls[1].kind, "struct");
        assert_eq!(decls[2].name, "Direction");
        assert_eq!(decls[2].kind, "enum");
        assert_eq!(decls[3].name, "Runnable");
        assert_eq!(decls[3].kind, "trait");
        assert_eq!(decls[4].name, "utils");
        assert_eq!(decls[4].kind, "mod");
    }

    #[test]
    fn test_extract_pub_decls_skips_test_files() {
        let diff = r#"@@ -1,3 +1,5 @@
+pub fn test_helper() {}
"#;
        let decls = extract_pub_decls("crates/foo/tests/integration.rs", diff);
        assert!(decls.is_empty(), "should skip test files");
    }

    #[test]
    fn test_extract_pub_decls_skips_xtask() {
        let diff = r#"@@ -1,3 +1,5 @@
+pub fn something() {}
"#;
        let decls = extract_pub_decls("crates/xtask/src/main.rs", diff);
        assert!(decls.is_empty(), "should skip xtask");
    }

    #[test]
    fn test_extract_pub_decls_async_fn() {
        let diff = r#"@@ -1,3 +1,5 @@
+pub async fn fetch_data() -> Result<Vec<u8>> {
"#;
        let decls = extract_pub_decls("crates/foo/src/lib.rs", diff);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].name, "fetch_data");
        assert_eq!(decls[0].kind, "fn");
    }

    #[test]
    fn test_extract_pub_decls_removed_lines_ignored() {
        let diff = r#"@@ -1,5 +1,3 @@
-pub fn old_func() {}
+pub fn new_func() {}
"#;
        let decls = extract_pub_decls("crates/foo/src/lib.rs", diff);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].name, "new_func");
    }

    #[test]
    fn test_parse_hunk_new_start() {
        assert_eq!(parse_hunk_new_start("@@ -1,5 +10,8 @@"), Some(10));
        assert_eq!(parse_hunk_new_start("@@ -1 +1 @@"), Some(1));
        assert_eq!(parse_hunk_new_start("@@ -1,5 +100 @@"), Some(100));
        assert_eq!(parse_hunk_new_start("not a hunk"), None);
    }

    #[test]
    fn test_extract_pub_decls_line_numbers() {
        let diff = r#"@@ -1,3 +5,8 @@
 context line
 another context
+pub fn first_func() {}
 more context
+pub struct SecondStruct {}
"#;
        let decls = extract_pub_decls("crates/foo/src/lib.rs", diff);
        assert_eq!(decls.len(), 2);
        // Hunk starts at new line 5; first_func is line 5+2=7
        assert_eq!(decls[0].name, "first_func");
        assert_eq!(decls[0].line, 7);
        assert_eq!(decls[1].name, "SecondStruct");
        assert_eq!(decls[1].line, 9);
    }
}
