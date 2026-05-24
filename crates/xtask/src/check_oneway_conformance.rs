use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use regex::Regex;

#[derive(ClapArgs)]
pub struct Args {
    /// Path to the ff-rdp-core source directory to audit.
    /// Defaults to `crates/ff-rdp-core/src`.
    #[arg(long)]
    core_src: Option<PathBuf>,
}

/// Build the set of method names declared `oneway: true` in the Firefox spec
/// files under `devtools/shared/specs/*.js`.
fn collect_oneway_methods(specs_dir: &std::path::Path) -> Result<HashSet<String>> {
    // Regex to match: `methodName: { oneway: true` (with optional whitespace).
    // We look for any identifier followed by `: {` and then `oneway: true`
    // within the next few hundred characters.
    let method_re = Regex::new(r"(\w+)\s*:\s*\{[^}]*\boneway\s*:\s*true").unwrap();

    let mut names = HashSet::new();

    let entries = std::fs::read_dir(specs_dir)
        .with_context(|| format!("failed to read specs dir: {}", specs_dir.display()))?;

    for entry in entries {
        let entry = entry.with_context(|| "failed to read dir entry")?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("js") {
            continue;
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        for cap in method_re.captures_iter(&content) {
            names.insert(cap[1].to_owned());
        }
    }

    Ok(names)
}

/// Find all `actor_request(transport, ..., "<method>", ...)` call sites in the
/// Rust source that match a method name in `oneway_methods`.
///
/// Returns a `Vec<(file, line, method)>` for each offending call site.
fn find_bad_actor_requests(
    core_src: &std::path::Path,
    oneway_methods: &HashSet<String>,
) -> Result<Vec<(PathBuf, usize, String)>> {
    // Match: actor_request(<anything>, "<method>", where method is in the set.
    // The pattern captures the quoted string in the third argument position.
    let call_re = Regex::new(r#"actor_request\s*\([^,]+,\s*[^,]+,\s*"([^"]+)""#).unwrap();

    let mut hits = Vec::new();

    collect_rs_files(core_src, &mut |path, content| {
        for (line_idx, line) in content.lines().enumerate() {
            if let Some(cap) = call_re.captures(line) {
                let method = &cap[1];
                if oneway_methods.contains(method) {
                    hits.push((path.to_owned(), line_idx + 1, method.to_owned()));
                }
            }
        }
    })?;

    Ok(hits)
}

/// Walk `dir` recursively, calling `f` for every `.rs` file.
fn collect_rs_files(
    dir: &std::path::Path,
    f: &mut impl FnMut(&std::path::Path, &str),
) -> Result<()> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("failed to read dir: {}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, f)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            f(&path, &content);
        }
    }
    Ok(())
}

pub fn run(args: Args) -> Result<()> {
    // Resolve the Firefox source root — same logic as check_firefox_refs.
    let firefox_root = match std::env::var("FF_RDP_FIREFOX_PATH") {
        Ok(path) => {
            let root = PathBuf::from(&path);
            if !root.exists() {
                bail!(
                    "FF_RDP_FIREFOX_PATH={:?} does not exist. \
                     Point it at the root of your Firefox checkout.",
                    root
                );
            }
            root
        }
        Err(_) => {
            // Try the well-known default before giving up.
            let default = std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|h| PathBuf::from(h).join("devel").join("firefox"));

            match default {
                Some(p) if p.exists() => p,
                _ => {
                    println!(
                        "check-oneway-conformance: skipped \
                         (FF_RDP_FIREFOX_PATH is unset and ~/devel/firefox not found)"
                    );
                    return Ok(());
                }
            }
        }
    };

    let specs_dir = firefox_root.join("devtools").join("shared").join("specs");
    if !specs_dir.exists() {
        bail!(
            "check-oneway-conformance: Firefox specs dir not found: {}",
            specs_dir.display()
        );
    }

    let oneway_methods = collect_oneway_methods(&specs_dir)
        .with_context(|| "failed to collect oneway methods from Firefox specs")?;

    if oneway_methods.is_empty() {
        println!(
            "check-oneway-conformance: no oneway methods found in {}",
            specs_dir.display()
        );
        return Ok(());
    }

    let core_src = args
        .core_src
        .unwrap_or_else(|| PathBuf::from("crates/ff-rdp-core/src"));

    if !core_src.exists() {
        bail!(
            "check-oneway-conformance: core src dir not found: {} \
             (run from the workspace root or pass --core-src)",
            core_src.display()
        );
    }

    let bad_calls = find_bad_actor_requests(&core_src, &oneway_methods)?;

    if bad_calls.is_empty() {
        println!(
            "check-oneway-conformance: OK — {} oneway methods checked, \
             no actor_request calls found for any of them",
            oneway_methods.len()
        );
        Ok(())
    } else {
        for (path, line, method) in &bad_calls {
            eprintln!(
                "  {}:{}: actor_request used for oneway method {:?} — \
                 use actor_send / actor_send_oneway instead",
                path.display(),
                line,
                method
            );
        }
        bail!(
            "check-oneway-conformance: {} call(s) use actor_request for \
             spec-declared oneway methods",
            bad_calls.len()
        );
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use tempfile::TempDir;

    fn write_file(dir: &std::path::Path, name: &str, content: &str) {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    /// AC: `check_oneway_conformance_catches_regression` — a synthetic
    /// source file containing an `actor_request` call for a spec-oneway
    /// method must cause the check to exit 1.
    #[test]
    fn check_oneway_conformance_catches_regression() {
        let tmp = TempDir::new().unwrap();

        // Create a fake specs dir with one oneway method.
        let specs_dir = tmp.path().join("specs");
        write_file(
            &specs_dir,
            "watcher.js",
            r#"
types.addDictType("WatcherActor", {
  unwatchResources: {
    oneway: true,
    request: { resourceTypes: Arg(0, "array:string") },
  },
});
"#,
        );

        // Verify we can parse the oneway method.
        let methods = collect_oneway_methods(&specs_dir).unwrap();
        assert!(
            methods.contains("unwatchResources"),
            "should detect unwatchResources as oneway, got: {methods:?}"
        );

        // Create a fake Rust source that calls actor_request with that method.
        let core_src = tmp.path().join("core_src");
        write_file(
            &core_src,
            "actors/watcher.rs",
            r#"
fn bad() {
    actor_request(transport, watcher_actor.as_ref(), "unwatchResources", Some(&params))
}
"#,
        );

        // The check should detect the regression.
        let bad_calls = find_bad_actor_requests(&core_src, &methods).unwrap();
        assert_eq!(
            bad_calls.len(),
            1,
            "expected 1 bad call, got: {bad_calls:?}"
        );
        assert_eq!(bad_calls[0].2, "unwatchResources");
    }

    #[test]
    fn check_oneway_conformance_passes_when_no_bad_calls() {
        let tmp = TempDir::new().unwrap();

        let specs_dir = tmp.path().join("specs");
        write_file(
            &specs_dir,
            "watcher.js",
            r#"
unwatchResources: {
  oneway: true,
  request: {},
}
"#,
        );

        let methods = collect_oneway_methods(&specs_dir).unwrap();
        assert!(methods.contains("unwatchResources"));

        // Source file uses actor_send instead — no regression.
        let core_src = tmp.path().join("core_src");
        write_file(
            &core_src,
            "actors/watcher.rs",
            r#"
fn good() {
    actor_send(transport, watcher_actor.as_ref(), "unwatchResources", Some(&params))
}
"#,
        );

        let bad_calls = find_bad_actor_requests(&core_src, &methods).unwrap();
        assert!(
            bad_calls.is_empty(),
            "expected no bad calls, got: {bad_calls:?}"
        );
    }
}
