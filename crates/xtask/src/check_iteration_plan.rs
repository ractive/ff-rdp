use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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
    // `obsolete` is a real terminal state distinct from `done` — a plan that was
    // superseded or abandoned rather than delivered (3 such plans exist). It is
    // NOT a synonym for `done`, so it is accepted here rather than normalized
    // away. `completed` deliberately is NOT accepted: it was a synonym for
    // `done` that the merge workflow used to write, and the 142 plans carrying
    // it were normalized to `done` so the vocabulary has one word per state.
    let valid_statuses = ["planned", "in-progress", "in-review", "done", "obsolete"];

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

/// Matches an iteration plan file name and captures its iteration id.
///
/// The capture is `<digits>` with an optional trailing letter run, anchored so
/// that the character after the id must be `-`. That boundary is what keeps
/// `iteration-162a-*.md` and `iteration-162b-*.md` — deliberate sibling plans —
/// from being read as two plans numbered 162. It also means `iteration-61b-*` and
/// `iteration-61c-*` are distinct ids, while two files both claiming `61b` still
/// collide. The letter run is `*` rather than `?` because `iteration-61aa-*.md`
/// exists: a single-letter pattern would silently exempt it from the check. The `.md` suffix requirement excludes `.dogfood.sh` sidecars, which
/// share a plan's stem (`iteration-96-profile-leak-cleanup.dogfood.sh`).
fn plan_file_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^iteration-([0-9]+[a-z]*)-.+\.md$").expect("static regex"))
}

/// Extract the iteration id from a plan file name, or `None` if the name is not
/// an iteration plan (`_template.md`, `stability-roadmap.md`, an arbitrary
/// scratch file). A `None` here is not an error — it simply means uniqueness is
/// not a meaningful question for this file.
fn plan_id_from_file_name(file_name: &str) -> Option<String> {
    plan_file_re()
        .captures(file_name)
        .map(|caps| caps[1].to_owned())
}

/// Duplicate iteration numbers that already exist in `kb/iterations/` and are
/// deliberately kept, recorded as an explicit exemption.
///
/// Disposition (iteration 187, 2026-08-23): all four plans below are terminal
/// (`done` or `obsolete`) and are cited by `[[wikilink]]` from kb notes and from
/// merged PR bodies. Renumbering them would break inbound links to no benefit,
/// so the two historical collisions are grandfathered rather than fixed — and
/// the check itself is NOT weakened to accommodate them.
///
/// The exemption is keyed on the *exact set of file names*, not on the number.
/// A third plan claiming 44 or 73 still fails, because the colliding set would no
/// longer match the recorded pair.
const LEGACY_COLLISIONS: &[(&str, &[&str])] = &[
    (
        "44",
        &[
            "iteration-44-github-setup-guide.md",
            "iteration-44-public-release.md",
        ],
    ),
    (
        "73",
        &[
            "iteration-73-hyalo-schema-for-iteration-plans.md",
            "iteration-73-spec-fidelity-gates.md",
        ],
    ),
];

/// True if `names` (sorted) is exactly a recorded historical collision for `id`.
fn is_legacy_collision(id: &str, names: &[String]) -> bool {
    LEGACY_COLLISIONS.iter().any(|(legacy_id, legacy_names)| {
        *legacy_id == id && names.len() == legacy_names.len() && {
            let mut expected: Vec<&str> = legacy_names.to_vec();
            expected.sort_unstable();
            names.iter().zip(expected).all(|(a, b)| a == b)
        }
    })
}

/// Check that `target` is the only plan claiming its iteration id among
/// `candidates`.
///
/// Pure: `candidates` is the already-collected list of sibling `*.md` paths, so
/// this is unit-testable without touching the filesystem. A candidate whose file
/// name equals the target's is treated as the target itself, so a plan can never
/// collide with its own copy in a second scanned directory.
fn duplicate_id_findings(target: &Path, candidates: &[PathBuf]) -> Vec<String> {
    let Some(target_name) = target.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return Vec::new();
    };
    let Some(id) = plan_id_from_file_name(&target_name) else {
        // Not a `iteration-<id>-<slug>.md` name — uniqueness does not apply.
        return Vec::new();
    };

    let mut colliding: Vec<&PathBuf> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for cand in candidates {
        let Some(name) = cand.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if name == target_name || seen.contains(&name) {
            continue;
        }
        if plan_id_from_file_name(&name).as_deref() == Some(id.as_str()) {
            seen.push(name);
            colliding.push(cand);
        }
    }

    if colliding.is_empty() {
        return Vec::new();
    }

    let mut all_names: Vec<String> = seen.clone();
    all_names.push(target_name.clone());
    all_names.sort();
    if is_legacy_collision(&id, &all_names) {
        return Vec::new();
    }

    let others = colliding
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join("\n      ");
    vec![format!(
        "duplicate iteration number {id}: this plan shares its number with another plan.\n    \
         this plan: {}\n    also claiming iteration-{id}:\n      {others}\n    \
         Pick a free number (`ls kb/iterations/`) and rename this file, or — if the two plans are \
         deliberately paired — give one a letter suffix (`iteration-{id}b-<slug>.md`).",
        target.display()
    )]
}

/// Collect the `*.md` files that `target`'s iteration id must be unique against:
/// every plan in the target's own directory, plus every plan in the repository's
/// `kb/iterations/` registry when that is a different directory.
///
/// Both are scanned because the registry — not the directory a file happens to
/// sit in — is what owns iteration numbers: a draft written to a scratch path
/// still collides with a filed plan. Missing or unreadable directories are
/// skipped silently; a path outside `kb/iterations/` must be validated, not
/// rejected, or the ralph-loop preflight would red-line on it.
fn collect_sibling_plans(target: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = target.parent() {
        // `parent` is empty for a bare file name like `iteration-9-x.md`.
        let parent = if parent.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            parent.to_path_buf()
        };
        dirs.push(parent);
    }
    if let Some(registry) = repo_iterations_dir() {
        dirs.push(registry);
    }

    let mut canonical_seen: Vec<PathBuf> = Vec::new();
    let mut out: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        let canonical = dir.canonicalize().unwrap_or_else(|_| dir.clone());
        if canonical_seen.contains(&canonical) {
            continue;
        }
        canonical_seen.push(canonical);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// The repository's `kb/iterations/` directory, resolved from the current working
/// directory via git. Returns `None` outside a git checkout or when the directory
/// does not exist, so the uniqueness check degrades to a same-directory scan
/// rather than failing.
fn repo_iterations_dir() -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8(output.stdout).ok()?;
    let dir = PathBuf::from(root.trim()).join("kb").join("iterations");
    dir.is_dir().then_some(dir)
}

pub fn run(args: Args) -> Result<()> {
    let content = std::fs::read_to_string(&args.path)
        .with_context(|| format!("failed to read {:?}", args.path))?;

    let plan = parse_plan(&content)?;
    let (mut findings, warnings) = validate_plan(&plan);

    // Uniqueness is a property of the file, not of its contents, so it is checked
    // here rather than inside `validate_plan`.
    findings.extend(duplicate_id_findings(
        &args.path,
        &collect_sibling_plans(&args.path),
    ));

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

    // --- iteration-number uniqueness (iteration 187) ---

    fn p(name: &str) -> PathBuf {
        PathBuf::from("kb/iterations").join(name)
    }

    #[test]
    fn plan_id_plain_number() {
        assert_eq!(
            plan_id_from_file_name("iteration-186-launch-records-leak.md").as_deref(),
            Some("186")
        );
    }

    #[test]
    fn plan_id_letter_suffix_is_its_own_id() {
        // 162a and 162b are deliberate siblings, not a collision.
        assert_eq!(
            plan_id_from_file_name("iteration-162a-discipline-removal.md").as_deref(),
            Some("162a")
        );
        assert_eq!(
            plan_id_from_file_name("iteration-162b-ac-fidelity-shrink.md").as_deref(),
            Some("162b")
        );
    }

    #[test]
    fn plan_id_handles_a_multi_letter_suffix() {
        // `iteration-61aa-claim-miss-hard-gate.md` is real. A single-letter
        // pattern would classify it as "not a plan" and skip it silently.
        assert_eq!(
            plan_id_from_file_name("iteration-61aa-claim-miss-hard-gate.md").as_deref(),
            Some("61aa")
        );
        let target = p("iteration-61aa-claim-miss-hard-gate.md");
        assert!(
            duplicate_id_findings(&target, &[p("iteration-61a-other.md")]).is_empty(),
            "61aa must not collide with 61a"
        );
        assert!(
            !duplicate_id_findings(&target, &[p("iteration-61aa-other.md")]).is_empty(),
            "two plans claiming 61aa must collide"
        );
    }

    #[test]
    fn plan_id_ignores_non_plan_names() {
        assert_eq!(plan_id_from_file_name("_template.md"), None);
        assert_eq!(plan_id_from_file_name("stability-roadmap.md"), None);
        // A sidecar shell script shares the stem but is not a plan.
        assert_eq!(
            plan_id_from_file_name("iteration-96-profile-leak-cleanup.dogfood.sh"),
            None
        );
        // No slug after the number.
        assert_eq!(plan_id_from_file_name("iteration-96.md"), None);
    }

    #[test]
    fn duplicate_id_is_reported_with_both_paths_and_the_number() {
        let target = p("iteration-186-launch-records-leak.md");
        let candidates = vec![
            p("iteration-185-main-red.md"),
            p("iteration-186-something-else-entirely.md"),
            p("iteration-187-uniqueness.md"),
        ];
        let findings = duplicate_id_findings(&target, &candidates);
        assert_eq!(findings.len(), 1, "expected one finding: {findings:?}");
        let f = &findings[0];
        assert!(f.contains("duplicate iteration number 186"), "{f}");
        assert!(f.contains("iteration-186-launch-records-leak.md"), "{f}");
        assert!(
            f.contains("iteration-186-something-else-entirely.md"),
            "{f}"
        );
    }

    #[test]
    fn a_plan_never_collides_with_itself() {
        // The target is included in the scanned directory listing, and the same
        // file name can turn up twice when two directories are scanned.
        let target = p("iteration-187-uniqueness.md");
        let candidates = vec![
            target.clone(),
            PathBuf::from("/tmp/iteration-187-uniqueness.md"),
        ];
        assert!(
            duplicate_id_findings(&target, &candidates).is_empty(),
            "a plan must not collide with itself"
        );
    }

    #[test]
    fn letter_suffixed_siblings_are_not_duplicates() {
        let target = p("iteration-162a-discipline-removal.md");
        let candidates = vec![
            p("iteration-162a-discipline-removal.md"),
            p("iteration-162b-ac-fidelity-shrink.md"),
            p("iteration-162-nonexistent-plain.md"),
        ];
        assert!(
            duplicate_id_findings(&target, &candidates).is_empty(),
            "162a must not collide with 162b or 162"
        );
    }

    #[test]
    fn identical_letter_suffixes_do_collide() {
        let target = p("iteration-61b-recorder-cli-wiring.md");
        let candidates = vec![p("iteration-61b-something-else.md")];
        let findings = duplicate_id_findings(&target, &candidates);
        assert!(
            findings
                .iter()
                .any(|f| f.contains("duplicate iteration number 61b")),
            "two plans claiming 61b must collide: {findings:?}"
        );
    }

    #[test]
    fn dogfood_sidecars_are_not_counted() {
        let target = p("iteration-96-profile-leak-cleanup.md");
        let candidates = vec![
            p("iteration-96-profile-leak-cleanup.md"),
            p("iteration-96-profile-leak-cleanup.dogfood.sh"),
        ];
        assert!(
            duplicate_id_findings(&target, &candidates).is_empty(),
            "a .dogfood.sh sidecar is not a second plan"
        );
    }

    #[test]
    fn legacy_44_and_73_collisions_are_exempt() {
        for (a, b) in [
            (
                "iteration-44-github-setup-guide.md",
                "iteration-44-public-release.md",
            ),
            (
                "iteration-73-hyalo-schema-for-iteration-plans.md",
                "iteration-73-spec-fidelity-gates.md",
            ),
        ] {
            assert!(
                duplicate_id_findings(&p(a), &[p(b)]).is_empty(),
                "{a} / {b} is a recorded historical collision and must not fail"
            );
            assert!(
                duplicate_id_findings(&p(b), &[p(a)]).is_empty(),
                "the exemption must hold from either side"
            );
        }
    }

    #[test]
    fn a_third_plan_claiming_a_legacy_number_still_fails() {
        // The exemption is keyed on the exact recorded pair, so it does not
        // become a permanent licence to reuse 44.
        let target = p("iteration-44-brand-new-plan.md");
        let candidates = vec![
            p("iteration-44-github-setup-guide.md"),
            p("iteration-44-public-release.md"),
        ];
        let findings = duplicate_id_findings(&target, &candidates);
        assert!(
            findings
                .iter()
                .any(|f| f.contains("duplicate iteration number 44")),
            "a new 44 must still fail: {findings:?}"
        );
    }

    #[test]
    fn non_plan_paths_are_handled_not_rejected() {
        // A path outside kb/iterations that is not named like a plan must be
        // silently accepted — the ralph-loop hands arbitrary paths to this check.
        assert!(duplicate_id_findings(&PathBuf::from("/tmp/notes.md"), &[]).is_empty());
        assert!(duplicate_id_findings(&PathBuf::from("kb/iterations"), &[]).is_empty());
        assert!(duplicate_id_findings(&PathBuf::from("/"), &[]).is_empty());
        assert!(duplicate_id_findings(&PathBuf::from(""), &[]).is_empty());
    }

    #[test]
    fn the_real_kb_iterations_directory_has_no_unexempted_duplicates() {
        // Non-regression: every plan filed in this repo must pass the check the
        // moment it lands. If this fails, two plans share a number — renumber the
        // newer one rather than widening LEGACY_COLLISIONS.
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("kb")
            .join("iterations");
        let entries: Vec<PathBuf> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
                .collect(),
            Err(e) => panic!("cannot read {}: {e}", dir.display()),
        };
        assert!(!entries.is_empty(), "no plans found in {}", dir.display());
        let mut failures: Vec<String> = Vec::new();
        for plan in &entries {
            failures.extend(duplicate_id_findings(plan, &entries));
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
