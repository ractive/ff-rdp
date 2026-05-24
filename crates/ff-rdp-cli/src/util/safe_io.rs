//! Safe file-write helpers that refuse to follow symlinks at the destination.
//!
//! [`safe_write`] and [`safe_create`] open the target path with `O_NOFOLLOW`
//! on Unix (or an explicit `is_symlink` pre-check on Windows) so that a
//! pre-positioned symlink at the destination cannot redirect the write to an
//! attacker-controlled location.
//!
//! [`ensure_within_root`] canonicalizes a path and verifies it is a descendant
//! of the supplied root, protecting against `..`-traversal when `--output-root`
//! is set.
//!
//! # Threat model
//!
//! `O_NOFOLLOW` rejects symlinks **at the final path component only**;
//! intermediate symlinks in the path are still followed. This is sufficient
//! for the realistic threat (a race-pre-positioned symlink at the destination).
//! Deep traversal via `openat2(RESOLVE_NO_SYMLINKS)` is out of scope; see the
//! design notes in `kb/iterations/iteration-65-safe-write-and-path-traversal-hardening.md`.
//!
//! # Windows caveat
//!
//! The Windows implementation pre-checks `path.is_symlink()` and then opens
//! normally, so a TOCTOU window exists between the check and the open. A
//! determined attacker on the same machine could swap in a symlink/reparse
//! point during that window. Hardening this to be race-free would require
//! Win32 `CreateFileW` with `FILE_FLAG_OPEN_REPARSE_POINT` plus a post-open
//! reparse-tag inspection; that is tracked as follow-up work and is not in
//! scope for iter-65. The Unix path is race-free via `O_NOFOLLOW`.
//!
//! # Note on `install_skill` and `recorder`
//!
//! `install_skill.rs` derives its write destinations by joining a resolved
//! install-root with an embedded or disk-local relative path — the root is
//! determined from `~/.claude/skills/` or the git root, not from caller input.
//! `recorder.rs` writes only to XDG state paths and the caller-supplied output
//! file (which `record start` passes through). Neither poses the same
//! symlink-at-destination risk as the screenshot/index/auto_consent writers
//! (which accept arbitrary user-supplied paths), so they are left unchanged.

use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Errors returned by the safe-IO helpers.
#[derive(Error, Debug)]
pub enum SafeIoError {
    /// The destination path is a symlink; the write was refused.
    #[error("refusing to write through symlink: {}", path.display())]
    SymlinkRefused { path: PathBuf },

    /// The resolved destination escapes the declared output root.
    #[error(
        "output path '{}' escapes --output-root '{}'",
        path.display(),
        root.display()
    )]
    OutsideRoot { path: PathBuf, root: PathBuf },

    /// An underlying I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// safe_write
// ---------------------------------------------------------------------------

/// Write `bytes` to `path`, refusing to follow a symlink at the destination.
///
/// - **Unix**: opens with `O_NOFOLLOW | O_CREAT | O_WRONLY | O_TRUNC`.
/// - **Windows**: pre-checks `path.is_symlink()` then opens normally.
///
/// On success the file is created (or truncated) and `bytes` are written.
pub fn safe_write(path: &Path, bytes: &[u8]) -> Result<(), SafeIoError> {
    let mut file = open_no_follow_write(path)?;
    file.write_all(bytes)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// safe_create
// ---------------------------------------------------------------------------

/// Create `path` exclusively (fails if it already exists), refusing symlinks.
///
/// - **Unix**: opens with `O_NOFOLLOW | O_CREAT | O_EXCL | O_WRONLY`.
/// - **Windows**: pre-checks `path.is_symlink()` then uses `create_new(true)`.
///
/// Returns the open [`File`] so the caller can write incrementally.
pub fn safe_create(path: &Path) -> Result<File, SafeIoError> {
    open_no_follow_excl(path)
}

// ---------------------------------------------------------------------------
// ensure_within_root
// ---------------------------------------------------------------------------

/// Canonicalize `path` and verify it is a descendant of `root`.
///
/// When `path` does not yet exist (common for output files), the parent
/// directory is canonicalized and the final component is appended.
///
/// Returns the canonical form of `path` on success, or
/// [`SafeIoError::OutsideRoot`] if it escapes `root`.
pub fn ensure_within_root(path: &Path, root: &Path) -> Result<PathBuf, SafeIoError> {
    let canonical_root = root.canonicalize().map_err(|e| {
        SafeIoError::Io(std::io::Error::new(
            e.kind(),
            format!("root '{}': {e}", root.display()),
        ))
    })?;

    // Canonicalize the deepest existing ancestor (so callers can target a
    // not-yet-created subdirectory like the default `.ffrdp/page-map.json`),
    // then logically join the remaining components, resolving `.` and `..`
    // without ever escaping that anchor.
    let canonical_path = if path.exists() {
        path.canonicalize()?
    } else {
        canonicalize_via_existing_ancestor(path)?
    };

    if canonical_path.starts_with(&canonical_root) {
        Ok(canonical_path)
    } else {
        Err(SafeIoError::OutsideRoot {
            path: canonical_path,
            root: canonical_root,
        })
    }
}

/// Canonicalize the deepest existing ancestor of `path`, then logically join
/// the remaining components — resolving `.` and `..` against that anchor so
/// traversal attempts still surface as paths outside `root`.
fn canonicalize_via_existing_ancestor(path: &Path) -> Result<PathBuf, SafeIoError> {
    let mut anchor = path.to_path_buf();
    let mut trailing: Vec<std::ffi::OsString> = Vec::new();
    while !anchor.exists() {
        let name = anchor.file_name().map(std::ffi::OsString::from);
        if !anchor.pop() || name.is_none() {
            // Reached the root without finding any existing ancestor — fall
            // back to the current working directory.
            anchor = std::env::current_dir()?;
            break;
        }
        trailing.push(name.expect("file_name set above"));
    }
    let mut resolved = anchor.canonicalize().map_err(|e| {
        SafeIoError::Io(std::io::Error::new(
            e.kind(),
            format!("ancestor '{}': {e}", anchor.display()),
        ))
    })?;
    for component in trailing.into_iter().rev() {
        match component.as_os_str().to_str() {
            Some(".") => {}
            Some("..") => {
                resolved.pop();
            }
            _ => resolved.push(&component),
        }
    }
    Ok(resolved)
}

// ---------------------------------------------------------------------------
// Platform-specific open helpers
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn open_no_follow_write(path: &Path) -> Result<File, SafeIoError> {
    use std::os::unix::fs::OpenOptionsExt as _;

    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| {
            // ELOOP / EMLINK is what Linux/macOS return for O_NOFOLLOW on a symlink.
            if e.raw_os_error()
                .is_some_and(|c| c == libc::ELOOP || c == libc::EMLINK)
            {
                SafeIoError::SymlinkRefused {
                    path: path.to_path_buf(),
                }
            } else {
                SafeIoError::Io(e)
            }
        })?;
    Ok(file)
}

#[cfg(unix)]
fn open_no_follow_excl(path: &Path) -> Result<File, SafeIoError> {
    use std::os::unix::fs::OpenOptionsExt as _;

    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| {
            if e.raw_os_error()
                .is_some_and(|c| c == libc::ELOOP || c == libc::EMLINK)
            {
                SafeIoError::SymlinkRefused {
                    path: path.to_path_buf(),
                }
            } else {
                SafeIoError::Io(e)
            }
        })?;
    Ok(file)
}

#[cfg(windows)]
fn open_no_follow_write(path: &Path) -> Result<File, SafeIoError> {
    if path.is_symlink() {
        return Err(SafeIoError::SymlinkRefused {
            path: path.to_path_buf(),
        });
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    Ok(file)
}

#[cfg(windows)]
fn open_no_follow_excl(path: &Path) -> Result<File, SafeIoError> {
    if path.is_symlink() {
        return Err(SafeIoError::SymlinkRefused {
            path: path.to_path_buf(),
        });
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    Ok(file)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // safe_write tests
    // -----------------------------------------------------------------------

    /// safe_write_rejects_symlink: writing through a pre-existing symlink returns
    /// Err(SafeIoError::SymlinkRefused).
    #[test]
    fn safe_write_rejects_symlink() {
        let dir = TempDir::new().expect("tempdir");
        let real_file = dir.path().join("real.txt");
        std::fs::write(&real_file, b"real").expect("create real");
        let link = dir.path().join("link.txt");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_file, &link).expect("symlink");
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&real_file, &link).expect("symlink");

        let result = safe_write(&link, b"evil");
        assert!(
            matches!(result, Err(SafeIoError::SymlinkRefused { .. })),
            "expected SymlinkRefused, got {result:?}"
        );
    }

    /// safe_write_succeeds_on_regular_file: write to a fresh path, then
    /// overwrite — both must succeed.
    #[test]
    fn safe_write_succeeds_on_regular_file() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("output.bin");

        safe_write(&path, b"first write").expect("first write");
        let contents = std::fs::read(&path).expect("read after first");
        assert_eq!(contents, b"first write");

        safe_write(&path, b"overwrite").expect("overwrite");
        let contents = std::fs::read(&path).expect("read after overwrite");
        assert_eq!(contents, b"overwrite");
    }

    // -----------------------------------------------------------------------
    // safe_create tests
    // -----------------------------------------------------------------------

    /// safe_create_rejects_existing: O_EXCL semantics — fails when file exists.
    #[test]
    fn safe_create_rejects_existing() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("exclusive.bin");
        std::fs::write(&path, b"exists").expect("create");

        let result = safe_create(&path);
        assert!(result.is_err(), "expected error on existing file, got Ok");
    }

    /// safe_create succeeds when the file does not yet exist.
    #[test]
    fn safe_create_succeeds_on_fresh_path() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("new.bin");

        let mut file = safe_create(&path).expect("safe_create");
        file.write_all(b"hello").expect("write");
        drop(file);

        let contents = std::fs::read(&path).expect("read");
        assert_eq!(contents, b"hello");
    }

    // -----------------------------------------------------------------------
    // ensure_within_root tests
    // -----------------------------------------------------------------------

    /// ensure_within_root_rejects_traversal: a `..` traversal is rejected.
    #[test]
    fn ensure_within_root_rejects_traversal() {
        let dir = TempDir::new().expect("tempdir");
        let root = dir.path().join("maps");
        // Create a sibling directory so canonicalize can resolve the parent traversal.
        let sibling = dir.path().join("secret");
        std::fs::create_dir_all(&root).expect("mkdir maps");
        std::fs::create_dir_all(&sibling).expect("mkdir secret");

        // Path that traverses out of root via `..` into a sibling directory.
        // `maps/../secret/creds.txt` → resolves to `<tmp>/secret/creds.txt`,
        // which is outside `<tmp>/maps`.
        let escaped = root.join("..").join("secret").join("creds.txt");

        let result = ensure_within_root(&escaped, &root);
        assert!(
            matches!(result, Err(SafeIoError::OutsideRoot { .. })),
            "expected OutsideRoot, got {result:?}"
        );
    }

    /// ensure_within_root accepts a child path whose parent does not yet exist
    /// (regression: default `.ffrdp/page-map.json` when `.ffrdp/` is missing).
    #[test]
    fn ensure_within_root_accepts_missing_parent() {
        let dir = TempDir::new().expect("tempdir");
        let root = dir.path().to_path_buf();
        // Neither subdir nor file exists yet.
        let nested = root.join("subdir").join("page-map.json");
        let canonical =
            ensure_within_root(&nested, &root).expect("missing parent should be accepted");
        assert!(
            canonical.starts_with(root.canonicalize().expect("canon root")),
            "canonical path {canonical:?} should start with the root"
        );
    }

    /// ensure_within_root still rejects traversal even when intermediate
    /// directories don't exist.
    #[test]
    fn ensure_within_root_rejects_traversal_with_missing_parent() {
        let dir = TempDir::new().expect("tempdir");
        let root = dir.path().join("maps");
        let sibling = dir.path().join("secret");
        std::fs::create_dir_all(&root).expect("mkdir maps");
        std::fs::create_dir_all(&sibling).expect("mkdir secret");
        // Traversal where the leaf path (and its parent) don't exist yet.
        let escaped = root
            .join("nope")
            .join("..")
            .join("..")
            .join("secret")
            .join("creds.txt");
        let result = ensure_within_root(&escaped, &root);
        assert!(
            matches!(result, Err(SafeIoError::OutsideRoot { .. })),
            "expected OutsideRoot, got {result:?}"
        );
    }

    /// ensure_within_root accepts a legitimate path inside the root.
    #[test]
    fn ensure_within_root_accepts_child() {
        let dir = TempDir::new().expect("tempdir");
        let root = dir.path().join("maps");
        std::fs::create_dir_all(&root).expect("mkdir maps");

        // The file doesn't need to exist — parent is canonicalized and name appended.
        let child = root.join("output.json");
        let canonical = ensure_within_root(&child, &root).expect("should be accepted");
        assert!(
            canonical.starts_with(root.canonicalize().expect("canon root")),
            "canonical path {canonical:?} should start with the root"
        );
    }
}
