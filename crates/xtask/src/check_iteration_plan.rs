use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(ClapArgs)]
pub struct Args {
    /// Path to the iteration plan markdown file.
    path: PathBuf,
}

/// The frontmatter fields we care about for validation.
#[derive(Debug, Deserialize, Default)]
pub struct PlanFrontmatter {
    #[serde(default)]
    pub status: Option<String>,
    /// first_call_sites: list of {primitive, site} entries.
    #[serde(default)]
    pub first_call_sites: Option<Vec<HashMap<String, String>>>,
    /// dogfood_path: either a scalar string or a multiline block scalar.
    #[serde(default)]
    pub dogfood_path: Option<String>,
    /// dogfood_script: sibling .sh file that is run by check-dogfood-script.
    #[serde(default)]
    pub dogfood_script: Option<String>,
}

/// Result of parsing a plan file.
#[derive(Debug)]
pub struct ParsedPlan {
    pub frontmatter: PlanFrontmatter,
    pub body: String,
}

/// Parse frontmatter and body from a markdown file.
pub fn parse_plan(content: &str) -> Result<ParsedPlan> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return Ok(ParsedPlan {
            frontmatter: PlanFrontmatter::default(),
            body: content.to_owned(),
        });
    }

    // Find the closing `---`
    let after_open = &content[3..];
    let close_pos = after_open
        .find("\n---")
        .context("unterminated YAML frontmatter (no closing ---)")?;

    let yaml_text = &after_open[..close_pos];
    let body_start = close_pos + 4; // skip "\n---"
    let body = after_open
        .get(body_start..)
        .unwrap_or("")
        .trim_start_matches('\n')
        .to_owned();

    let frontmatter: PlanFrontmatter =
        serde_norway::from_str(yaml_text).context("failed to parse YAML frontmatter")?;

    Ok(ParsedPlan { frontmatter, body })
}

/// Validate a parsed plan.
///
/// Returns `(findings, warnings)`:
/// - `findings` are hard failures — any non-empty list means the plan is invalid.
/// - `warnings` are advisory messages that do not cause a hard failure.
pub fn validate_plan(plan: &ParsedPlan) -> (Vec<String>, Vec<String>) {
    let mut findings = Vec::new();
    let mut warnings = Vec::new();
    let valid_statuses = ["planned", "in-progress", "in-review", "done"];

    // Validate status field.
    match &plan.frontmatter.status {
        None => findings.push(format!(
            "frontmatter missing required field: status (must be {})",
            valid_statuses.join("|")
        )),
        Some(s) if !valid_statuses.contains(&s.as_str()) => findings.push(format!(
            "frontmatter status '{}' is not one of: {}",
            s,
            valid_statuses.join(", ")
        )),
        _ => {}
    }

    // Check if the plan body introduces new pub symbols.
    let introduces_pub = body_introduces_pub_symbols(&plan.body);

    if introduces_pub {
        // Validate first_call_sites.
        match &plan.frontmatter.first_call_sites {
            None => {
                findings.push(
                    "plan body mentions pub symbols but first_call_sites is missing or empty; \
                     add first_call_sites: [{primitive: '...', site: '...'}] to frontmatter"
                        .to_owned(),
                );
            }
            Some(v) if v.is_empty() => {
                findings.push(
                    "plan body mentions pub symbols but first_call_sites is missing or empty; \
                     add first_call_sites: [{primitive: '...', site: '...'}] to frontmatter"
                        .to_owned(),
                );
            }
            Some(entries) => {
                // Validate each entry has `primitive` and `site` keys.
                for (i, entry) in entries.iter().enumerate() {
                    if !entry.contains_key("primitive") {
                        findings.push(format!(
                            "first_call_sites[{}] is missing required key: primitive",
                            i
                        ));
                    }
                    if !entry.contains_key("site") {
                        findings.push(format!(
                            "first_call_sites[{}] is missing required key: site",
                            i
                        ));
                    }
                }
            }
        }
    }

    // Validate dogfood — required as dogfood_path (frontmatter or body section)
    // OR dogfood_script frontmatter key.
    let has_dogfood_path_frontmatter = plan
        .frontmatter
        .dogfood_path
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    let has_dogfood_script = plan
        .frontmatter
        .dogfood_script
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    let has_dogfood_section = plan.body.lines().any(|l| {
        let lower = l.to_lowercase();
        lower.starts_with("## dogfood") || lower.starts_with("# dogfood")
    });

    let has_dogfood_path = has_dogfood_path_frontmatter || has_dogfood_section;

    if !has_dogfood_path && !has_dogfood_script {
        findings.push(
            "missing dogfood_path: add a dogfood_path frontmatter key, a ## Dogfood path \
             section, or a dogfood_script frontmatter key pointing to a sibling .sh file"
                .to_owned(),
        );
    }

    if has_dogfood_path && has_dogfood_script {
        warnings.push(
            "both dogfood_path and dogfood_script are set; dogfood_script will be used by \
             check-dogfood-script, dogfood_path is now redundant"
                .to_owned(),
        );
    }

    (findings, warnings)
}

/// Returns true if the body text contains patterns suggesting new pub symbols
/// are being introduced (e.g., the plan describes implementing `pub fn ...`).
fn body_introduces_pub_symbols(body: &str) -> bool {
    let re = Regex::new(r"\bpub\s+(fn|struct|enum|trait|mod)\b").expect("static regex");
    re.is_match(body)
}

pub fn run(args: Args) -> Result<()> {
    let content = std::fs::read_to_string(&args.path)
        .with_context(|| format!("failed to read {:?}", args.path))?;

    let plan = parse_plan(&content)?;
    let (findings, warnings) = validate_plan(&plan);

    for w in &warnings {
        eprintln!("check-iteration-plan: warn: {w}");
    }

    if findings.is_empty() {
        println!("check-iteration-plan: OK");
        return Ok(());
    }

    eprintln!(
        "check-iteration-plan: {} finding(s) in {:?}",
        findings.len(),
        args.path
    );
    for f in &findings {
        eprintln!("  - {f}");
    }
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_minimal_plan(extras: &str) -> String {
        format!(
            "---\ntitle: \"Test Plan\"\nstatus: planned\ntype: iteration\n{extras}---\n\n# Body\n"
        )
    }

    #[test]
    fn test_parse_plan_minimal() {
        let content = make_minimal_plan("");
        let plan = parse_plan(&content).unwrap();
        assert_eq!(plan.frontmatter.status.as_deref(), Some("planned"));
    }

    #[test]
    fn test_parse_plan_no_frontmatter() {
        let content = "# Just a heading\n\nSome body.";
        let plan = parse_plan(content).unwrap();
        assert!(plan.frontmatter.status.is_none());
        assert!(plan.body.contains("Just a heading"));
    }

    #[test]
    fn test_validate_plan_valid_minimal() {
        let content = "---\nstatus: planned\ndogfood_path: \"ff-rdp --help\"\n---\n\n# Body\n";
        let plan = parse_plan(content).unwrap();
        let (findings, _warnings) = validate_plan(&plan);
        assert!(findings.is_empty(), "unexpected findings: {findings:?}");
    }

    #[test]
    fn test_validate_plan_missing_status() {
        let content = "---\ntitle: test\ndogfood_path: x\n---\n# Body\n";
        let plan = parse_plan(content).unwrap();
        let (findings, _warnings) = validate_plan(&plan);
        assert!(
            findings.iter().any(|f| f.contains("status")),
            "expected status finding"
        );
    }

    #[test]
    fn test_validate_plan_invalid_status() {
        let content = "---\nstatus: in_progress\ndogfood_path: x\n---\n# Body\n";
        let plan = parse_plan(content).unwrap();
        let (findings, _warnings) = validate_plan(&plan);
        assert!(
            findings.iter().any(|f| f.contains("in_progress")),
            "expected invalid status finding"
        );
    }

    #[test]
    fn test_validate_plan_pub_symbols_without_call_sites() {
        let content = "---\nstatus: planned\ndogfood_path: \"ff-rdp --help\"\n---\n\nThis plan adds `pub fn new_feature()` to the codebase.\n";
        let plan = parse_plan(content).unwrap();
        let (findings, _warnings) = validate_plan(&plan);
        assert!(
            findings.iter().any(|f| f.contains("first_call_sites")),
            "expected first_call_sites finding, got: {findings:?}"
        );
    }

    #[test]
    fn test_validate_plan_pub_symbols_with_valid_call_sites() {
        let content = "---\nstatus: planned\ndogfood_path: \"ff-rdp --help\"\nfirst_call_sites:\n  - primitive: my_crate::NewFeature\n    site: crates/ff-rdp-cli/src/main.rs:42\n---\n\nThis plan adds `pub fn new_feature()` to the codebase.\n";
        let plan = parse_plan(content).unwrap();
        let (findings, _warnings) = validate_plan(&plan);
        assert!(
            !findings.iter().any(|f| f.contains("first_call_sites")),
            "should not flag first_call_sites when valid: {findings:?}"
        );
    }

    #[test]
    fn test_validate_plan_missing_dogfood_path() {
        let content = "---\nstatus: planned\n---\n\n# Body without dogfood\n";
        let plan = parse_plan(content).unwrap();
        let (findings, _warnings) = validate_plan(&plan);
        assert!(
            findings.iter().any(|f| f.contains("dogfood_path")),
            "expected dogfood_path finding"
        );
    }

    #[test]
    fn test_validate_plan_dogfood_section_in_body() {
        let content = "---\nstatus: planned\n---\n\n## Dogfood path\n\nff-rdp screenshot --url https://example.com\n";
        let plan = parse_plan(content).unwrap();
        let (findings, _warnings) = validate_plan(&plan);
        assert!(
            !findings.iter().any(|f| f.contains("dogfood_path")),
            "should accept dogfood section in body"
        );
    }

    #[test]
    fn test_validate_plan_call_site_missing_keys() {
        let content = "---\nstatus: planned\ndogfood_path: x\nfirst_call_sites:\n  - primitive: foo::Bar\n---\n\nAdds `pub struct NewThing`.\n";
        let plan = parse_plan(content).unwrap();
        let (findings, _warnings) = validate_plan(&plan);
        assert!(
            findings.iter().any(|f| f.contains("site")),
            "expected missing 'site' key finding"
        );
    }

    #[test]
    fn test_validate_plan_dogfood_script_alone_sufficient() {
        // dogfood_script alone (no dogfood_path) should satisfy the dogfood requirement.
        let content =
            "---\nstatus: planned\ndogfood_script: iteration-99-test.dogfood.sh\n---\n\n# Body\n";
        let plan = parse_plan(content).unwrap();
        let (findings, warnings) = validate_plan(&plan);
        assert!(
            !findings.iter().any(|f| f.contains("dogfood")),
            "dogfood_script alone should satisfy requirement, got findings: {findings:?}"
        );
        assert!(
            warnings.is_empty(),
            "no warnings expected when only dogfood_script set: {warnings:?}"
        );
    }

    #[test]
    fn test_validate_plan_both_dogfood_path_and_script_emits_warning() {
        // Both present: no hard finding, but a warning.
        let content = "---\nstatus: planned\ndogfood_path: \"ff-rdp --help\"\ndogfood_script: iter.dogfood.sh\n---\n\n# Body\n";
        let plan = parse_plan(content).unwrap();
        let (findings, warnings) = validate_plan(&plan);
        assert!(
            !findings.iter().any(|f| f.contains("dogfood")),
            "both present should not produce a hard finding: {findings:?}"
        );
        assert!(
            warnings.iter().any(|w| w.contains("dogfood_script")),
            "expected warning about both being set: {warnings:?}"
        );
    }

    #[test]
    fn test_validate_plan_neither_dogfood_produces_finding() {
        // Neither dogfood_path nor dogfood_script → hard finding as before.
        let content = "---\nstatus: planned\n---\n\n# Body without dogfood\n";
        let plan = parse_plan(content).unwrap();
        let (findings, _warnings) = validate_plan(&plan);
        assert!(
            findings.iter().any(|f| f.contains("dogfood")),
            "expected dogfood finding when neither field set: {findings:?}"
        );
    }
}
