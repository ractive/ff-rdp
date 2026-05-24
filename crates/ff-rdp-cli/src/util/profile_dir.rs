//! Resolution of the per-user directory under which ff-rdp creates ephemeral
//! Firefox profiles.
//!
//! # Why not `env::temp_dir()`?
//!
//! `/tmp` (or `%TEMP%`) is typically world-writable.  Even though we name the
//! sub-directory with 16 random bytes, the parent is shared with every other
//! process on the box, so:
//!
//! - On Unix, a colocated same-UID process can race us to plant a `user.js`
//!   symlink that rides our `fs::write` to overwrite an arbitrary file the
//!   user can write.  Mode-0o700 on the profile sub-dir mitigates this but
//!   the parent is still shared.
//! - On multi-user systems, audit logs in `/tmp` are inspectable by other
//!   accounts; profile contents (cookies, prefs) should not live there.
//!
//! `dirs::state_dir()` (XDG `$XDG_STATE_HOME`, typically `~/.local/state`) is
//! the right home for ephemeral state that survives reboots if not pruned.
//! When `state_dir` is unavailable (older macOS, no `$HOME`) we fall back to
//! `data_local_dir` (`~/Library/Application Support` on macOS,
//! `%LOCALAPPDATA%` on Windows).  Both are per-user directories with default
//! permissions that deny other accounts.
//!
//! # Windows ACL story
//!
//! `%LOCALAPPDATA%` is created by Windows with a per-user ACL that grants
//! Full Control to the current SID and to SYSTEM, denying Everyone by
//! inheritance defaults.  Sub-directories created under it inherit those
//! restrictions, so explicit `SetNamedSecurityInfoW` is not required for
//! the threat model described above.
//! See Microsoft's "Default ACLs for user profile folders":
//! <https://learn.microsoft.com/en-us/windows/win32/secauthz/default-acls-for-user-profile-folders>.

use std::path::PathBuf;

use crate::error::AppError;

/// Resolve (and create, mode 0700 on Unix) the per-user root directory under
/// which ff-rdp drops ephemeral Firefox profile sub-directories.
///
/// Resolution order:
/// 1. `dirs::state_dir()` — `$XDG_STATE_HOME` on Linux, falls back to
///    `~/.local/state` when unset.  `None` on macOS / Windows.
/// 2. `dirs::data_local_dir()` — `~/Library/Application Support` on macOS,
///    `%LOCALAPPDATA%` on Windows.
///
/// The chosen base is joined with `ff-rdp/profiles`.  The full path is
/// created with `create_dir_all`; on Unix, the leaf is then chmod'd to
/// `0o700` (the recursive parents are left alone — they already exist with
/// user-default modes).
pub fn secure_profile_root() -> Result<PathBuf, AppError> {
    let base = dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .ok_or_else(|| {
            AppError::User(
                "no per-user state or data directory available — cannot create \
                 a secure Firefox profile root.  Set $XDG_STATE_HOME or $HOME."
                    .to_owned(),
            )
        })?;
    let root = base.join("ff-rdp").join("profiles");

    std::fs::create_dir_all(&root).map_err(|e| {
        AppError::User(format!(
            "failed to create secure profile root {}: {e}",
            root.display()
        ))
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o700);
        std::fs::set_permissions(&root, perms).map_err(|e| {
            AppError::User(format!(
                "failed to set mode 0o700 on profile root {}: {e}",
                root.display()
            ))
        })?;
    }

    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC: `secure_profile_root_mode_0700` — the resolved directory exists,
    /// sits under `dirs::state_dir()` or `data_local_dir()`, and has mode
    /// `0o700` on Unix.
    #[cfg(unix)]
    #[test]
    fn secure_profile_root_mode_0700() {
        use std::os::unix::fs::PermissionsExt;

        let root = secure_profile_root().expect("secure profile root must resolve");
        assert!(root.is_dir(), "expected a directory at {}", root.display());
        let expected_base = dirs::state_dir().or_else(dirs::data_local_dir).unwrap();
        assert!(
            root.starts_with(&expected_base),
            "profile root {} must be under {}",
            root.display(),
            expected_base.display()
        );
        let mode = std::fs::metadata(&root).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o700,
            "profile root must be mode 0o700, found {mode:o}"
        );
    }

    /// AC: `secure_profile_root_windows_per_user` — on Windows the resolved
    /// directory sits under `%LOCALAPPDATA%` and is a valid directory.  We
    /// rely on the inherited default ACL (per-user) for confidentiality.
    #[cfg(windows)]
    #[test]
    fn secure_profile_root_windows_per_user() {
        let root = secure_profile_root().expect("secure profile root must resolve");
        assert!(root.is_dir(), "expected a directory at {}", root.display());
        let local_appdata = dirs::data_local_dir().expect("LOCALAPPDATA must be defined");
        assert!(
            root.starts_with(&local_appdata),
            "profile root {} must be under {}",
            root.display(),
            local_appdata.display()
        );
    }
}
