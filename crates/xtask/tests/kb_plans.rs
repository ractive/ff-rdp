//! Lints over `kb/iterations/iteration-*.md` (formerly `xtask
//! check-iteration-plan`, shrunk to a test by DEC-059).
//!
//! 1. No two plans share an iteration id. The id is `<digits><letters>` up to
//!    the first `-`, so `162a` and `162b` are distinct while two `61b` collide.
//!    The historical 44 and 73 pairs are exempt by exact file-name pair, so a
//!    third plan claiming either number still fails.
//! 2. Frontmatter `status` is one of `planned | in-progress | in-review | done
//!    | obsolete`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use regex::Regex;

const STATUSES: &[&str] = &["planned", "in-progress", "in-review", "done", "obsolete"];

/// Grandfathered collisions (iteration 187): terminal plans cited by wikilink.
const LEGACY_COLLISIONS: &[&[&str]] = &[
    &[
        "iteration-44-github-setup-guide.md",
        "iteration-44-public-release.md",
    ],
    &[
        "iteration-73-hyalo-schema-for-iteration-plans.md",
        "iteration-73-spec-fidelity-gates.md",
    ],
];

/// `(file name, contents)` of every iteration plan, sorted by name.
fn plans() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("kb")
        .join("iterations");
    let mut out: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|e| e.expect("dir entry").path())
        .filter_map(|p: PathBuf| {
            let name = p.file_name()?.to_str()?.to_owned();
            (name.starts_with("iteration-") && p.extension().is_some_and(|e| e == "md"))
                .then(|| (name, std::fs::read_to_string(&p).expect("read plan")))
        })
        .collect();
    out.sort();
    assert!(
        !out.is_empty(),
        "no iteration-*.md plans found in {}",
        dir.display()
    );
    out
}

#[test]
fn no_two_plans_share_an_iteration_number() {
    let id_re = Regex::new(r"^iteration-([0-9]+[a-z]*)-.+\.md$").expect("static regex");
    let mut by_id: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, _) in plans() {
        if let Some(caps) = id_re.captures(&name) {
            by_id.entry(caps[1].to_owned()).or_default().push(name);
        }
    }
    let dups: Vec<String> = by_id
        .into_iter()
        .filter(|(_, names)| {
            names.len() > 1
                && !LEGACY_COLLISIONS.contains(
                    &names
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .as_slice(),
                )
        })
        .map(|(id, names)| format!("iteration {id}: {}", names.join(", ")))
        .collect();
    assert!(
        dups.is_empty(),
        "duplicate iteration numbers — rename one plan to a free number (`ls kb/iterations/`) \
         or give a deliberate sibling a letter suffix (`iteration-<N>b-<slug>.md`):\n  {}",
        dups.join("\n  ")
    );
}

#[test]
fn every_plan_has_a_valid_status() {
    let mut bad = Vec::new();
    for (name, content) in plans() {
        // Frontmatter must be opened *and* closed: an unterminated block is
        // what made plan 84 invisible to hyalo (iter-234).
        let Some(frontmatter) = content
            .trim_start()
            .strip_prefix("---")
            .and_then(|rest| rest.find("\n---").map(|end| &rest[..end]))
        else {
            bad.push(format!(
                "{name}: no frontmatter block delimited by `---` lines"
            ));
            continue;
        };
        let status = frontmatter
            .lines()
            .find_map(|l| l.strip_prefix("status:"))
            .map(|s| {
                let value = s.split(" #").next().unwrap_or(s);
                value.trim().trim_matches(['"', '\''])
            });
        match status {
            Some(s) if STATUSES.contains(&s) => {}
            other => bad.push(format!("{name}: status {other:?}")),
        }
    }
    assert!(
        bad.is_empty(),
        "plan frontmatter `status` must be one of {}:\n  {}",
        STATUSES.join(" | "),
        bad.join("\n  ")
    );
}
