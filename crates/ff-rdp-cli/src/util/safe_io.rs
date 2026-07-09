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
//! # Windows reparse-point inspection (iter-77 / M-4)
//!
//! The Windows implementation now opens the **parent directory** with
//! `CreateFileW(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)`
//! and inspects the reparse tag via `DeviceIoControl(FSCTL_GET_REPARSE_POINT)`
//! before writing.  A parent whose reparse tag is
//! `IO_REPARSE_TAG_SYMLINK` or `IO_REPARSE_TAG_MOUNT_POINT` is refused with
//! [`SafeIoError::ReparsePointRejected`] — substantially narrowing the iter-44
//! TOCTOU window flagged by the M-4 review.
//!
//! This is **not** fully race-free: the implementation still performs a
//! check-then-open on the parent path rather than holding a handle and
//! opening relative to it.  A determined local attacker who can swap the
//! parent between our `reparse_tag_of` call and the subsequent `CreateFileW`
//! could still bypass the check.  A race-free version would require opening
//! the parent directory, retaining the handle, and then opening the child
//! via `NtCreateFile` with a relative `OBJECT_ATTRIBUTES` — out of scope for
//! iter-77.
//!
//! Application-layer reparse tags (Windows Store `AppExecutionAlias` etc.)
//! are *not* a redirect vector and would break legitimate workflows; only
//! `SYMLINK` and `MOUNT_POINT` are rejected.
//!
//! The Unix path remains race-free via `O_NOFOLLOW`.
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

// iter-105 Theme D: the Windows path performs reparse-point inspection via FFI
// (`CreateFileW`, `DeviceIoControl`, `CloseHandle`, `GetLastError`).  The crate
// default is `unsafe_code = "deny"`; this narrow, file-scoped allowance keeps
// the audited, `// SAFETY:`-documented Windows FFI compiling while the rest of
// the crate still denies unsafe.
#![allow(unsafe_code)]

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

    /// The destination's parent directory carries an `IO_REPARSE_TAG_SYMLINK`
    /// or `IO_REPARSE_TAG_MOUNT_POINT` reparse tag (Windows only); writing
    /// would follow the reparse point to a possibly attacker-controlled
    /// location.  Application-layer reparse tags (AppExecutionAlias etc.)
    /// are not flagged.
    ///
    /// Only constructed on Windows builds; carried in the cross-platform
    /// enum so callers can match exhaustively without `cfg` shenanigans.
    #[allow(dead_code)]
    #[error(
        "refusing to write under reparse point at '{}' (tag=0x{tag:x})",
        path.display()
    )]
    ReparsePointRejected { path: PathBuf, tag: u32 },

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
    refuse_redirecting_reparse_parent(path)?;
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
    refuse_redirecting_reparse_parent(path)?;
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
// Windows reparse-point inspection (iter-77 / M-4)
// ---------------------------------------------------------------------------

/// Microsoft-defined reparse tag for an NTFS symbolic link.
#[cfg(windows)]
pub const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;
/// Microsoft-defined reparse tag for an NTFS mount point (a.k.a. junction).
#[cfg(windows)]
pub const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;

/// Read the reparse tag of `path` on Windows.
///
/// Returns `Ok(Some(tag))` when the path is a reparse point, `Ok(None)`
/// when it is a regular file/directory, and an `io::Error` on a system
/// call failure (e.g. missing file).
///
/// Uses `CreateFileW(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)`
/// followed by `DeviceIoControl(FSCTL_GET_REPARSE_POINT)`.
#[cfg(windows)]
pub fn reparse_tag_of(path: &Path) -> std::io::Result<Option<u32>> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_NOT_A_REPARSE_POINT, GetLastError, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    // FSCTL_GET_REPARSE_POINT = 0x900a8
    const FSCTL_GET_REPARSE_POINT: u32 = 0x0009_00a8;
    // MAXIMUM_REPARSE_DATA_BUFFER_SIZE
    const MAXIMUM_REPARSE_DATA_BUFFER_SIZE: usize = 16 * 1024;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: `wide` is a NUL-terminated UTF-16 string for the lifetime of
    // the call.  `CreateFileW` returns a raw HANDLE or INVALID_HANDLE_VALUE
    // on failure; we close it via `CloseHandle` on every exit path.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            0, // dwDesiredAccess: 0 is sufficient for metadata + DeviceIoControl
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }

    // 8-byte reparse header: u32 ReparseTag, u16 ReparseDataLength, u16 Reserved.
    let mut buf = vec![0u8; MAXIMUM_REPARSE_DATA_BUFFER_SIZE];
    let mut bytes_returned: u32 = 0;
    // SAFETY: `buf` outlives the call; `handle` is a valid open HANDLE.
    let ok = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_GET_REPARSE_POINT,
            std::ptr::null(),
            0,
            buf.as_mut_ptr().cast(),
            buf.len() as u32,
            &raw mut bytes_returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        let code = unsafe { GetLastError() };
        // SAFETY: `handle` is valid.
        unsafe {
            CloseHandle(handle);
        }
        if code == ERROR_NOT_A_REPARSE_POINT {
            return Ok(None);
        }
        return Err(std::io::Error::from_raw_os_error(code as i32));
    }
    // SAFETY: `handle` is valid.
    unsafe {
        CloseHandle(handle);
    }

    if bytes_returned < 4 {
        return Ok(None);
    }
    let tag = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    Ok(Some(tag))
}

/// Reject `path` when its parent directory carries a redirecting reparse tag.
#[cfg(windows)]
fn refuse_redirecting_reparse_parent(path: &Path) -> Result<(), SafeIoError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    match reparse_tag_of(parent) {
        Ok(Some(tag)) if tag == IO_REPARSE_TAG_SYMLINK || tag == IO_REPARSE_TAG_MOUNT_POINT => {
            Err(SafeIoError::ReparsePointRejected {
                path: parent.to_path_buf(),
                tag,
            })
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(SafeIoError::Io(e)),
    }
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

    /// AC: `safe_io_rejects_mount_point_windows` — when the parent
    /// directory is an `IO_REPARSE_TAG_MOUNT_POINT` (junction), `safe_write`
    /// returns [`SafeIoError::ReparsePointRejected`] and never opens a file
    /// underneath the redirected location.  Gated on `FF_RDP_LIVE_TESTS=1`
    /// because creating a junction requires admin or Developer Mode.
    #[cfg(windows)]
    #[test]
    fn safe_io_rejects_mount_point_windows() {
        if std::env::var_os("FF_RDP_LIVE_TESTS").is_none() {
            eprintln!("skipping: set FF_RDP_LIVE_TESTS=1 to run (needs admin/DevMode for mklink)");
            return;
        }
        use std::process::Command;
        let dir = TempDir::new().expect("tempdir");
        let real_target = dir.path().join("real_target");
        std::fs::create_dir_all(&real_target).expect("mkdir real_target");
        let junction = dir.path().join("junction");

        // Use `cmd /c mklink /J <link> <target>` to create an NTFS junction
        // (mount-point reparse tag).  No admin needed for /J on most systems.
        let status = Command::new("cmd")
            .args([
                "/c",
                "mklink",
                "/J",
                junction.to_str().unwrap(),
                real_target.to_str().unwrap(),
            ])
            .status();
        match status {
            Ok(s) if s.success() => {}
            other => {
                eprintln!("skipping: mklink failed ({other:?})");
                return;
            }
        }

        // Confirm the parent is detected as a mount point.
        let tag = reparse_tag_of(&junction).expect("reparse_tag_of");
        assert_eq!(tag, Some(IO_REPARSE_TAG_MOUNT_POINT));

        // Attempt to write under the junction — must be refused.
        let target_file = junction.join("stolen.bin");
        let err = safe_write(&target_file, b"evil").expect_err("must reject");
        assert!(
            matches!(err, SafeIoError::ReparsePointRejected { .. }),
            "expected ReparsePointRejected, got {err:?}"
        );

        // A non-reparse sibling write must succeed.
        let regular_dir = dir.path().join("regular");
        std::fs::create_dir_all(&regular_dir).expect("mkdir regular");
        let regular_file = regular_dir.join("ok.bin");
        safe_write(&regular_file, b"ok").expect("regular write must succeed");
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
