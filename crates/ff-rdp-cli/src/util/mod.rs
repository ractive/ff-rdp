pub mod profile_dir;
pub mod safe_io;
pub mod window_size;

use std::path::PathBuf;

/// Environment variable that overrides the per-user base directory ff-rdp
/// resolves *all* of its state under: the daemon registry
/// ([`crate::daemon::registry::registry_dir`]), launch records
/// ([`crate::daemon_record::record_base_dir`]), and the Firefox profile
/// root ([`profile_dir::secure_profile_root`], since iter-188 Theme B).
///
/// Useful for test isolation, and on Windows where `dirs::home_dir()` uses
/// the Windows API and ignores `HOME`/`USERPROFILE` overrides.
pub(crate) const HOME_OVERRIDE_ENV: &str = "FF_RDP_HOME";

/// Read [`HOME_OVERRIDE_ENV`], the single implementation every resolver that
/// honours it shares.
///
/// An **empty** value is treated the same as unset. Before this helper
/// existed, `profile_dir::resolve_profile_root` filtered out the empty
/// string but `registry_dir()` and `record_base_dir()` did not — so
/// `FF_RDP_HOME=""` sent the registry and launch records to a
/// CWD-relative `./.ff-rdp` while the profiles root fell through to the
/// real per-user path, reintroducing the exact split-state-directory bug
/// iter-188 Theme B exists to eliminate, just through a different input.
/// One shared reader makes "the same variable, meaning the same thing"
/// true structurally instead of by three independently-written `match`
/// arms that can drift.
pub(crate) fn home_override() -> Option<PathBuf> {
    std::env::var_os(HOME_OVERRIDE_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}
