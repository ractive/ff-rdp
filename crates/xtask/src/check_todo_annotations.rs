use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use regex::Regex;
use std::process::Command;

#[derive(ClapArgs)]
pub struct Args {
    /// Git ref to diff against (default: origin/main, fallback: main).
    #[arg(long, default_value = "")]
    since: String,
}

/// A rejected TODO annotation.
#[derive(Debug, PartialEq, Eq)]
pub struct TodoFinding {
    pub file: String,
    pub line: usize,
    pub content: String,
}

/// Rules for deciding if a TODO line is allowed.
#[derive(Debug)]
struct AllowRules {
    /// github.com/.../issues/N
    github_issue: Regex,
    /// Jira-style: WORD-NNN
    jira: Regex,
    /// explicit allow annotation
    allow_todo: Regex,
    /// TODO/FIXME/XXX keyword
    todo_keyword: Regex,
}

impl AllowRules {
    fn new() -> Self {
        Self {
            github_issue: Regex::new(r"github\.com/[^/]+/[^/]+/issues/\d+").expect("static"),
            jira: Regex::new(r"\b[A-Z]+-\d+\b").expect("static"),
            allow_todo: Regex::new(r"allow-todo:").expect("static"),
            todo_keyword: Regex::new(r"\b(TODO|FIXME|XXX)\b").expect("static"),
        }
    }

    fn is_todo(&self, line: &str) -> bool {
        self.todo_keyword.is_match(line)
    }

    fn is_allowed(&self, line: &str) -> bool {
        self.github_issue.is_match(line)
            || self.jira.is_match(line)
            || self.allow_todo.is_match(line)
    }
}

/// Represents a file's added lines from a diff.
#[derive(Debug)]
pub struct FileDiff {
    pub file: String,
    pub added_lines: Vec<(usize, String)>, // (line_number, content)
}

/// Parse a unified diff into per-file added lines.
pub fn parse_diff_added_lines(diff_text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut current_file: Option<String> = None;
    let mut new_line: usize = 0;

    let hunk_re = Regex::new(r"\+(\d+)(?:,\d+)?\s+@@").expect("static");

    for raw_line in diff_text.lines() {
        if raw_line.starts_with("+++ b/") {
            let file = raw_line.trim_start_matches("+++ b/").to_owned();
            files.push(FileDiff {
                file: file.clone(),
                added_lines: Vec::new(),
            });
            current_file = Some(file);
            new_line = 0;
            continue;
        }
        if raw_line.starts_with("@@") {
            if let Some(caps) = hunk_re.captures(raw_line) {
                new_line = caps[1].parse::<usize>().unwrap_or(0);
            }
            continue;
        }
        if raw_line.starts_with('-') {
            continue;
        }
        if raw_line.starts_with('+') || raw_line.starts_with(' ') {
            if let Some(stripped) = raw_line.strip_prefix('+')
                && let Some(ref file) = current_file
                && let Some(fd) = files.iter_mut().find(|f| &f.file == file)
            {
                fd.added_lines.push((new_line, stripped.to_owned()));
            }
            new_line += 1;
        }
    }

    files
}

/// Check added lines for unannotated TODO/FIXME/XXX.
pub fn check_file_diffs(file_diffs: &[FileDiff]) -> Vec<TodoFinding> {
    let rules = AllowRules::new();
    let mut findings = Vec::new();

    for fd in file_diffs {
        for (line_num, content) in &fd.added_lines {
            if !rules.is_todo(content) {
                continue;
            }
            if rules.is_allowed(content) {
                continue;
            }
            findings.push(TodoFinding {
                file: fd.file.clone(),
                line: *line_num,
                content: content.trim().to_owned(),
            });
        }
    }

    findings
}

fn resolve_ref(since: &str) -> String {
    if !since.is_empty() {
        return since.to_owned();
    }
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "origin/main"])
        .output();
    match output {
        Ok(o) if o.status.success() => "origin/main".to_owned(),
        _ => "main".to_owned(),
    }
}

pub fn run(args: Args) -> Result<()> {
    let git_ref = resolve_ref(&args.since);

    let output = Command::new("git")
        .args(["diff", &format!("{git_ref}...HEAD")])
        .output()
        .context("failed to run git diff")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git diff against {git_ref} failed: {stderr}");
    }

    let diff_text = String::from_utf8_lossy(&output.stdout);
    let file_diffs = parse_diff_added_lines(&diff_text);
    let findings = check_file_diffs(&file_diffs);

    if findings.is_empty() {
        return Ok(());
    }

    eprintln!("check-todo-annotations: unannotated TODO/FIXME/XXX found:");
    eprintln!("  Add a GitHub issue link, Jira ticket (WORD-123), or '// allow-todo: <reason>'");
    eprintln!();
    for f in &findings {
        eprintln!("{}:{}: {}", f.file, f.line, f.content);
    }
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bare_todo_is_rejected() {
        let rules = AllowRules::new();
        assert!(rules.is_todo("// TODO: fix this later"));
        assert!(!rules.is_allowed("// TODO: fix this later"));
    }

    #[test]
    fn test_todo_with_github_link_is_allowed() {
        let rules = AllowRules::new();
        let line = "// TODO: fix this https://github.com/ractive/ff-rdp/issues/42";
        assert!(rules.is_todo(line));
        assert!(rules.is_allowed(line));
    }

    #[test]
    fn test_todo_with_jira_is_allowed() {
        let rules = AllowRules::new();
        let line = "// TODO: fix this PROJ-123";
        assert!(rules.is_todo(line));
        assert!(rules.is_allowed(line));
    }

    #[test]
    fn test_todo_with_allow_todo_annotation_is_allowed() {
        let rules = AllowRules::new();
        let line = "// TODO: defer for later // allow-todo: intentional design choice";
        assert!(rules.is_todo(line));
        assert!(rules.is_allowed(line));
    }

    #[test]
    fn test_fixme_and_xxx_are_also_caught() {
        let rules = AllowRules::new();
        assert!(rules.is_todo("// FIXME: broken"));
        assert!(rules.is_todo("// XXX: hack"));
    }

    #[test]
    fn test_parse_diff_added_lines_basic() {
        let diff = r#"+++ b/crates/foo/src/lib.rs
@@ -1,3 +1,5 @@
 context
+// TODO: bare annotation
+// regular code
"#;
        let file_diffs = parse_diff_added_lines(diff);
        assert_eq!(file_diffs.len(), 1);
        assert_eq!(file_diffs[0].file, "crates/foo/src/lib.rs");
        assert_eq!(file_diffs[0].added_lines.len(), 2);
        assert_eq!(file_diffs[0].added_lines[0].1, "// TODO: bare annotation");
    }

    #[test]
    fn test_check_file_diffs_rejects_bare_todo() {
        let diffs = vec![FileDiff {
            file: "crates/foo/src/lib.rs".to_owned(),
            added_lines: vec![
                (10, "// TODO: fix this".to_owned()),
                (11, "let x = 1;".to_owned()),
            ],
        }];
        let findings = check_file_diffs(&diffs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, 10);
    }

    #[test]
    fn test_check_file_diffs_allows_annotated_todo() {
        let diffs = vec![FileDiff {
            file: "crates/foo/src/lib.rs".to_owned(),
            added_lines: vec![(
                10,
                "// TODO: tracked in https://github.com/ractive/ff-rdp/issues/99".to_owned(),
            )],
        }];
        let findings = check_file_diffs(&diffs);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_todo_in_variable_name_not_matched() {
        let rules = AllowRules::new();
        // "TODO" must be at a word boundary — "todoist" should not match
        assert!(!rules.is_todo("let todoist = fetch();"));
    }
}
