#[cfg(target_os = "macos")]
pub(crate) mod child_fds;
pub(crate) mod process;
pub mod profile_dir;
pub mod safe_io;
pub mod window_size;

use std::path::PathBuf;

/// Environment variable that overrides the per-user base directory ff-rdp
/// resolves its Firefox profile root under
/// ([`profile_dir::secure_profile_root`], since iter-188 Theme B).
///
/// Useful for test isolation, and on Windows where `dirs::home_dir()` uses
/// the Windows API and ignores `HOME`/`USERPROFILE` overrides.
pub(crate) const HOME_OVERRIDE_ENV: &str = "FF_RDP_HOME";

/// Read [`HOME_OVERRIDE_ENV`], the single implementation every resolver that
/// honours it shares.
///
/// An **empty** value is treated the same as unset, so `FF_RDP_HOME=""`
/// never resolves to a CWD-relative directory.
pub(crate) fn home_override() -> Option<PathBuf> {
    std::env::var_os(HOME_OVERRIDE_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}
