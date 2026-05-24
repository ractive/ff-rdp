use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use regex::Regex;
use std::process::Command;

#[derive(ClapArgs)]
pub struct Args {
    /// Git ref to diff against (default: origin/main).
    #[arg(long, default_value = "origin/main")]
    since: String,
}

/// Mapping from actor source filename stem → expected kb path(s).
/// If any of the listed kb paths exists and was touched in the diff, the check passes.
const ACTOR_KB_MAP: &[(&str, &[&str])] = &[
    ("device", &["kb/rdp/actors/device.md"]),
    ("dom_walker", &["kb/rdp/actors/walker.md"]),
    ("inspector", &["kb/rdp/actors/inspector.md"]),
    (
        "network",
        &[
            "kb/rdp/actors/network-content.md",
            "kb/rdp/actors/network-event.md",
            "kb/rdp/actors/network-parent.md",
        ],
    ),
    ("object", &["kb/rdp/actors/object.md"]),
    ("page_style", &["kb/rdp/actors/page-style.md"]),
    ("responsive", &["kb/rdp/actors/responsive.md"]),
    ("root", &["kb/rdp/actors/root.md"]),
    ("screenshot", &["kb/rdp/actors/screenshot.md"]),
    (
        "screenshot_content",
        &["kb/rdp/actors/screenshot-content.md"],
    ),
    ("storage", &["kb/rdp/actors/storage.md"]),
    ("string", &["kb/rdp/actors/string.md"]),
    ("tab", &["kb/rdp/actors/tab.md"]),
    ("target", &["kb/rdp/actors/target.md"]),
    ("thread", &["kb/rdp/actors/thread.md"]),
    ("watcher", &["kb/rdp/actors/watcher.md"]),
    ("accessibility", &["kb/rdp/actors/accessibility.md"]),
    ("console", &["kb/rdp/actors/console.md"]),
];

/// Return the list of files changed since `git_ref...HEAD`.
pub fn changed_files(git_ref: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["diff", "--name-only", &format!("{git_ref}...HEAD")])
        .output()
        .context("failed to run git diff --name-only")?;

    if !output.status.success() {
        bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Check whether the given actor .rs file has `// allow-actor-kb-skip:` in its first 20 lines.
///
/// Reads the file at HEAD via `git show HEAD:<path>` so that the check works
/// correctly regardless of the working directory when GIT_WORK_TREE is set.
/// Falls back to a direct filesystem read if git show fails (e.g., new file
/// not yet committed).
pub fn has_skip_annotation(file_path: &str) -> bool {
    // Try `git show HEAD:<path>` first — respects GIT_WORK_TREE.
    let git_out = Command::new("git")
        .args(["show", &format!("HEAD:{file_path}")])
        .output();

    let content = match git_out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            // Fall back to direct filesystem read (e.g., untracked new file).
            let Ok(c) = std::fs::read_to_string(file_path) else {
                return false;
            };
            c
        }
    };

    content
        .lines()
        .take(20)
        .any(|l| l.contains("// allow-actor-kb-skip:"))
}

/// Find the actor stem for a changed file path, if it is an actor source file.
fn actor_stem_for(path: &str) -> Option<&'static str> {
    let re =
        Regex::new(r"^crates/ff-rdp-core/src/actors/([a-z][a-z0-9_]*)\.rs$").expect("static regex");

    let caps = re.captures(path)?;
    let stem = caps.get(1)?.as_str();

    // Return the static stem from our map if it matches.
    ACTOR_KB_MAP
        .iter()
        .find(|(k, _)| *k == stem)
        .map(|(k, _)| *k)
}

pub fn run(args: Args) -> Result<()> {
    let changed = changed_files(&args.since)?;

    // Collect the set of changed kb paths for quick lookup.
    let changed_set: std::collections::HashSet<&str> = changed.iter().map(String::as_str).collect();

    let mut errors: Vec<String> = Vec::new();

    for path in &changed {
        let Some(stem) = actor_stem_for(path) else {
            continue;
        };

        // Check for skip annotation.
        if has_skip_annotation(path) {
            continue;
        }

        // Find expected kb paths.
        let kb_paths = ACTOR_KB_MAP
            .iter()
            .find(|(k, _)| *k == stem)
            .map(|(_, v)| *v)
            .unwrap_or(&[]);

        // Pass if any of the kb paths was touched in the diff.
        let any_kb_touched = kb_paths.iter().any(|kp| changed_set.contains(*kp));
        if !any_kb_touched {
            let paths_str = kb_paths.join(", ");
            errors.push(format!(
                "{path} was changed but no corresponding kb note was updated.\n  \
                 Expected one of: {paths_str}\n  \
                 Add a note or update the existing one, or add \
                 `// allow-actor-kb-skip: <reason>` to the first 20 lines of the file."
            ));
        }
    }

    if errors.is_empty() {
        println!("check-actor-kb-sync: OK");
        Ok(())
    } else {
        for e in &errors {
            eprintln!("  - {e}");
        }
        bail!(
            "check-actor-kb-sync: {} actor(s) changed without kb sync",
            errors.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_stem_for_known_actor() {
        assert_eq!(
            actor_stem_for("crates/ff-rdp-core/src/actors/watcher.rs"),
            Some("watcher")
        );
    }

    #[test]
    fn actor_stem_for_dom_walker() {
        assert_eq!(
            actor_stem_for("crates/ff-rdp-core/src/actors/dom_walker.rs"),
            Some("dom_walker")
        );
    }

    #[test]
    fn actor_stem_for_non_actor() {
        assert!(actor_stem_for("crates/ff-rdp-core/src/transport.rs").is_none());
    }

    #[test]
    fn actor_stem_for_unknown_actor() {
        // An actor file that is not in the mapping should return None.
        assert!(actor_stem_for("crates/ff-rdp-core/src/actors/unknown_thing.rs").is_none());
    }
}
