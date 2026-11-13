//! The vendored Readability bundle is byte-for-byte the pinned release.
//!
//! Formerly `cargo run -p xtask -- check-vendored-js` (iter-219); a plain
//! `cargo test` since DEC-059.
//!
//! `crates/ff-rdp-cli/js/readability/` is injected into the live page by
//! `--with-page`. It is committed rather than downloaded so the output is a
//! function of the ff-rdp version alone — which only holds if the committed
//! bytes are the released ones. This recomputes the SHA-256 of every file
//! `VERSION` pins and fails on any mismatch or on a file nobody pinned.
//! Upgrading is deliberate: update the hashes in `VERSION` in the same commit.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const VERSION_FILE: &str = "VERSION";

fn vendor_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("js")
        .join("readability")
}

/// `(file, sha256)` pairs from the `sha256 <hex>  <file>` lines of a manifest.
/// Comments, `key = value` provenance lines and blanks are ignored.
fn parse_pins(manifest: &str) -> Vec<(String, String)> {
    manifest
        .lines()
        .filter_map(|line| line.trim().strip_prefix("sha256 "))
        .map(|rest| {
            let mut parts = rest.split_whitespace();
            let (Some(sha), Some(file)) = (parts.next(), parts.next()) else {
                panic!("{VERSION_FILE}: malformed pin line `sha256 {rest}`");
            };
            assert!(
                sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit()),
                "{VERSION_FILE}: {sha:?} is not a 64-character hex digest"
            );
            (file.to_owned(), sha.to_ascii_lowercase())
        })
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut acc, b| {
            use std::fmt::Write as _;
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

/// Every mismatch, unreadable pinned file, or unpinned file in `dir`.
fn pin_failures(dir: &Path) -> (usize, Vec<String>) {
    let manifest_path = dir.join(VERSION_FILE);
    let manifest = std::fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", manifest_path.display()));
    let pins = parse_pins(&manifest);
    let mut failures = Vec::new();
    for (file, pinned) in &pins {
        let path = dir.join(file);
        match std::fs::read(&path) {
            Err(e) => failures.push(format!("{}: cannot be read ({e})", path.display())),
            Ok(bytes) => {
                let actual = sha256_hex(&bytes);
                if &actual != pinned {
                    failures.push(format!(
                        "{}: SHA-256 mismatch\n    pinned:  {pinned}\n    on disk: {actual}",
                        path.display()
                    ));
                }
            }
        }
    }
    // A file dropped in without a pin is as much a fork as an edited one.
    for entry in std::fs::read_dir(dir).expect("list vendor dir") {
        let entry = entry.expect("dir entry");
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != VERSION_FILE && !name.starts_with('.') && !pins.iter().any(|(f, _)| *f == name) {
            failures.push(format!(
                "{}: present but not pinned in {VERSION_FILE}",
                entry.path().display()
            ));
        }
    }
    (pins.len(), failures)
}

#[test]
fn the_committed_bundle_matches_its_pins() {
    let dir = vendor_dir();
    let (pinned, failures) = pin_failures(&dir);
    assert!(
        pinned > 0,
        "{} pins no files",
        dir.join(VERSION_FILE).display()
    );
    assert!(
        failures.is_empty(),
        "vendored JavaScript does not match its pins:\n  {}\nfix: restore the file \
         from the upstream release, or — for a deliberate upgrade — update the \
         hashes in {}",
        failures.join("\n  "),
        dir.join(VERSION_FILE).display()
    );
}

#[test]
fn a_one_byte_edit_and_an_unpinned_file_both_fail() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.js"), b"x").expect("write");
    std::fs::write(
        dir.path().join(VERSION_FILE),
        format!("version = 1\nsha256 {}  a.js\n", sha256_hex(b"x")),
    )
    .expect("write manifest");
    assert_eq!(pin_failures(dir.path()), (1, Vec::new()));

    std::fs::write(dir.path().join("a.js"), b"x ").expect("edit");
    let (_, failures) = pin_failures(dir.path());
    assert!(failures[0].contains("SHA-256 mismatch"), "{failures:?}");

    std::fs::write(dir.path().join("a.js"), b"x").expect("restore");
    std::fs::write(dir.path().join("smuggled.js"), b"evil").expect("write");
    let (_, failures) = pin_failures(dir.path());
    assert!(failures[0].contains("not pinned"), "{failures:?}");
}

#[test]
fn sha256_matches_the_known_empty_digest() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}
