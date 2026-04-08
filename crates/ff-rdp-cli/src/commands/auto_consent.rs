use std::path::Path;

use crate::error::AppError;

/// Extension ID used by Consent-O-Matic on AMO.
const EXTENSION_ID: &str = "gdpr@cavi.au.dk";

/// AMO download URL for Consent-O-Matic v1.1.5.
const XPI_URL: &str =
    "https://addons.mozilla.org/firefox/downloads/file/4515369/consent_o_matic-1.1.5.xpi";

/// Return the platform cache directory for the XPI file.
/// Uses `dirs::cache_dir()` / "ff-rdp" / "extensions" / "{id}.xpi".
fn cached_xpi_path() -> Result<std::path::PathBuf, AppError> {
    let base = dirs::cache_dir().ok_or_else(|| {
        AppError::User("cannot determine cache directory for extension download".to_owned())
    })?;
    Ok(base
        .join("ff-rdp")
        .join("extensions")
        .join(format!("{EXTENSION_ID}.xpi")))
}

/// Download the XPI from AMO if not already cached. Returns the path to the
/// cached file.
fn ensure_cached() -> Result<std::path::PathBuf, AppError> {
    let path = cached_xpi_path()?;
    if path.is_file() {
        return Ok(path);
    }
    // Create parent directories
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            AppError::User(format!(
                "failed to create cache directory {}: {e}",
                parent.display()
            ))
        })?;
    }

    // Download the XPI
    let response = ureq::get(XPI_URL).call().map_err(|e| {
        AppError::User(format!("failed to download Consent-O-Matic extension: {e}"))
    })?;

    // Read body into Vec<u8>
    let body = response
        .into_body()
        .read_to_vec()
        .map_err(|e| AppError::User(format!("failed to read extension download: {e}")))?;

    // Write atomically: write to temp file then rename
    let tmp_path = path.with_extension("xpi.tmp");
    std::fs::write(&tmp_path, &body)
        .map_err(|e| AppError::User(format!("failed to write cached extension: {e}")))?;
    std::fs::rename(&tmp_path, &path)
        .map_err(|e| AppError::User(format!("failed to finalize cached extension: {e}")))?;

    Ok(path)
}

/// Install the Consent-O-Matic extension into the given Firefox profile.
///
/// Downloads the XPI from AMO (or uses the cached copy) and copies it into
/// `<profile>/extensions/{extension-id}.xpi`. Firefox picks it up on next
/// startup.
pub(crate) fn install(profile_dir: &Path) -> Result<(), AppError> {
    let cached = ensure_cached()?;
    let ext_dir = profile_dir.join("extensions");
    std::fs::create_dir_all(&ext_dir).map_err(|e| {
        AppError::User(format!(
            "failed to create extensions directory {}: {e}",
            ext_dir.display()
        ))
    })?;
    let dest = ext_dir.join(format!("{EXTENSION_ID}.xpi"));
    std::fs::copy(&cached, &dest).map_err(|e| {
        AppError::User(format!(
            "failed to install Consent-O-Matic to {}: {e}",
            dest.display()
        ))
    })?;
    Ok(())
}
