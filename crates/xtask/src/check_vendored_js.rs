//! `xtask check-vendored-js` — pin the vendored third-party JavaScript
//! (iter-219 Theme A).
//!
//! `crates/ff-rdp-cli/js/readability/` holds Mozilla's Readability bundle,
//! which ff-rdp injects into the live page so `--with-page` can tell the
//! article apart from the site chrome. It is committed rather than downloaded
//! at runtime for two reasons — the output of `--with-page` must be a function
//! of the ff-rdp version alone, and a debugging CLI has no business executing
//! freshly-downloaded code in a user's browser session.
//!
//! That trade only holds if the committed bytes are the bytes the upstream
//! release shipped. This gate recomputes the SHA-256 of every file named in
//! `VERSION` and fails on any mismatch, so touching the minified bundle —
//! whether to "just fix one thing" or by an editor stripping a trailing
//! newline — is a red check rather than a silent fork of somebody else's
//! parser.
//!
//! Upgrading is deliberate: download the release, minify, update the hashes in
//! `VERSION`, commit. The diff a reviewer sees names the version it moved to.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use sha2::{Digest, Sha256};

/// Directory of the vendored bundle, relative to the workspace root.
const VENDOR_DIR: &str = "crates/ff-rdp-cli/js/readability";

/// The manifest inside that directory: `key = value` lines plus
/// `sha256 <hex>  <filename>` lines.
const VERSION_FILE: &str = "VERSION";

#[derive(ClapArgs)]
pub struct Args {
    /// Directory holding VERSION and the pinned files
    #[arg(long, value_name = "PATH")]
    pub dir: Option<PathBuf>,
}

/// The workspace root, found by walking up from this crate's manifest dir.
fn workspace_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    while dir.pop() {
        if dir.join("Cargo.lock").exists() {
            return dir;
        }
    }
    PathBuf::from(".")
}

/// One pinned file: its name and the SHA-256 `VERSION` claims for it.
#[derive(Debug, PartialEq, Eq)]
pub struct Pin {
    pub file: String,
    pub sha256: String,
}

/// Parse the `sha256 <hex>  <file>` lines out of a `VERSION` manifest.
///
/// Everything else — comments, `key = value` provenance lines, blank lines —
/// is ignored, so the manifest stays readable by a human first.
pub fn parse_pins(manifest: &str) -> Result<Vec<Pin>> {
    let mut pins = Vec::new();
    for (n, line) in manifest.lines().enumerate() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("sha256 ") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let (Some(sha256), Some(file)) = (parts.next(), parts.next()) else {
            bail!("{VERSION_FILE} line {}: expected `sha256 <hex>  <file>`", n + 1);
        };
        if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!(
                "{VERSION_FILE} line {}: {sha256:?} is not a 64-character hex digest",
                n + 1
            );
        }
        pins.push(Pin {
            file: file.to_owned(),
            sha256: sha256.to_ascii_lowercase(),
        });
    }
    if pins.is_empty() {
        bail!("{VERSION_FILE} pins no files: expected at least one `sha256 <hex>  <file>` line");
    }
    Ok(pins)
}

/// Hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut acc, b| {
            use std::fmt::Write as _;
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

pub fn run(args: Args) -> Result<()> {
    let dir = args
        .dir
        .unwrap_or_else(|| workspace_root().join(VENDOR_DIR));
    let manifest_path = dir.join(VERSION_FILE);
    let manifest = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let pins = parse_pins(&manifest)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;

    let mut failures: Vec<String> = Vec::new();
    for pin in &pins {
        let path = dir.join(&pin.file);
        match std::fs::read(&path) {
            Err(e) => failures.push(format!("{}: cannot be read ({e})", path.display())),
            Ok(bytes) => {
                let actual = sha256_hex(&bytes);
                if actual != pin.sha256 {
                    failures.push(format!(
                        "{}: SHA-256 mismatch\n    pinned:   {}\n    on disk:  {actual}",
                        path.display(),
                        pin.sha256
                    ));
                }
            }
        }
    }

    // A file dropped into the directory without a pin is as much a fork as an
    // edited one — it would be `include_str!`-able and reviewed by nobody.
    let extras = unpinned_files(&dir, &pins)?;
    for extra in &extras {
        failures.push(format!(
            "{}: present but not pinned in {VERSION_FILE}",
            dir.join(extra).display()
        ));
    }

    if failures.is_empty() {
        println!(
            "check-vendored-js: {} file(s) match {}",
            pins.len(),
            manifest_path.display()
        );
        return Ok(());
    }
    bail!(
        "vendored JavaScript does not match its pins:\n  {}\n\
         fix: restore the file from the upstream release, or — for a deliberate \
         upgrade — update the hashes in {}",
        failures.join("\n  "),
        manifest_path.display()
    );
}

/// Files in `dir` that no pin names (excluding `VERSION` itself).
fn unpinned_files(dir: &Path, pins: &[Pin]) -> Result<Vec<String>> {
    let mut extras = Vec::new();
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("failed to list {}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == VERSION_FILE || name.starts_with('.') {
            continue;
        }
        if !pins.iter().any(|p| p.file == name) {
            extras.push(name);
        }
    }
    extras.sort();
    Ok(extras)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "\
# a comment
package = @mozilla/readability
version = 0.6.0

sha256 0000000000000000000000000000000000000000000000000000000000000001  Readability.min.js
sha256 0000000000000000000000000000000000000000000000000000000000000002  LICENSE
";

    #[test]
    fn unit_219_pins_are_parsed_and_comments_ignored() {
        let pins = parse_pins(MANIFEST).expect("manifest parses");
        assert_eq!(pins.len(), 2);
        assert_eq!(pins[0].file, "Readability.min.js");
        assert_eq!(pins[1].sha256.len(), 64);
    }

    #[test]
    fn unit_219_a_manifest_with_no_pins_is_an_error() {
        let err = parse_pins("version = 0.6.0\n").expect_err("no pins must fail");
        assert!(err.to_string().contains("pins no files"), "{err}");
    }

    #[test]
    fn unit_219_a_short_digest_is_rejected() {
        let err = parse_pins("sha256 abc  Readability.js\n").expect_err("short digest must fail");
        assert!(err.to_string().contains("hex digest"), "{err}");
    }

    #[test]
    fn unit_219_sha256_matches_the_known_empty_digest() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    /// AC `check-vendored-js fails on a one-byte edit and passes on the
    /// committed tree`, in unit form: the same directory passes, then fails
    /// after one byte is appended.
    #[test]
    fn unit_219_one_byte_edit_flips_the_verdict() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("Readability.min.js");
        std::fs::write(&file, b"function Readability() {}").expect("write");
        let digest = sha256_hex(b"function Readability() {}");
        std::fs::write(
            dir.path().join(VERSION_FILE),
            format!("version = 0.6.0\nsha256 {digest}  Readability.min.js\n"),
        )
        .expect("write manifest");

        run(Args {
            dir: Some(dir.path().to_path_buf()),
        })
        .expect("the untouched tree must pass");

        std::fs::write(&file, b"function Readability() {} ").expect("edit");
        let err = run(Args {
            dir: Some(dir.path().to_path_buf()),
        })
        .expect_err("a one-byte edit must fail the gate");
        assert!(err.to_string().contains("SHA-256 mismatch"), "{err}");
    }

    /// A file nobody pinned is a fork too — `include_str!` would happily ship
    /// it.
    #[test]
    fn unit_219_an_unpinned_file_fails_the_gate() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.js"), b"x").expect("write");
        std::fs::write(
            dir.path().join(VERSION_FILE),
            format!("sha256 {}  a.js\n", sha256_hex(b"x")),
        )
        .expect("write manifest");
        run(Args {
            dir: Some(dir.path().to_path_buf()),
        })
        .expect("baseline passes");

        std::fs::write(dir.path().join("smuggled.js"), b"evil").expect("write");
        let err = run(Args {
            dir: Some(dir.path().to_path_buf()),
        })
        .expect_err("an unpinned file must fail");
        assert!(err.to_string().contains("not pinned"), "{err}");
    }

    /// The committed tree must be green — this is the gate running against
    /// itself, which is what makes the other tests worth anything.
    #[test]
    fn unit_219_the_committed_bundle_matches_its_pins() {
        run(Args { dir: None }).expect("the committed vendored bundle must match VERSION");
    }
}
