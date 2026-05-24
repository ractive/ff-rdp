use std::io::Write as _;
use std::path::Path;

use crate::error::AppError;
use crate::util::safe_io::safe_create;

/// Extension ID used by Consent-O-Matic on AMO.
const EXTENSION_ID: &str = "gdpr@cavi.au.dk";

/// Vendored Consent-O-Matic XPI (v1.1.5, MIT-licensed).
///
/// We embed the bytes at build time rather than downloading from AMO at
/// install time so a hostile network / compromised CA / AMO compromise
/// cannot substitute a malicious WebExtension that would then run inside
/// the launched Firefox with full WebExtension permissions. See
/// `kb/iterations/iteration-64-xpi-integrity.md` (finding F-3 of the
/// 2026-05-24 security review).
///
/// The companion `LICENSE-consent-o-matic.txt` documents the upstream
/// source URL and the SHA-256 of the bytes embedded here. To bump the
/// pinned version: replace the `.xpi` asset, regenerate the SHA-256 in
/// `LICENSE-consent-o-matic.txt`, and update both the `include_bytes!`
/// path below AND `XPI_SHA256_HEX` together. The
/// `vendored_xpi_provenance_file_is_in_sync` test guards against
/// forgetting to update the provenance text.
const XPI_BYTES: &[u8] = include_bytes!("../../assets/extensions/consent-o-matic-1.1.5.xpi");

/// SHA-256 of `XPI_BYTES`, captured at vendor time. Exposed so tests can
/// fail loudly if the on-disk asset is ever modified without also updating
/// the licence/provenance file.
#[cfg_attr(not(test), allow(dead_code))]
const XPI_SHA256_HEX: &str = "a2119abc329638d6e7af1ab4e5548a348465e02eec11de08dee0af84919923dc";

/// Install the Consent-O-Matic extension into the given Firefox profile.
///
/// Writes the vendored XPI bytes into
/// `<profile>/extensions/{extension-id}.xpi`. Firefox picks the extension
/// up on next startup.
pub(crate) fn install(profile_dir: &Path) -> Result<(), AppError> {
    let ext_dir = profile_dir.join("extensions");
    std::fs::create_dir_all(&ext_dir).map_err(|e| {
        AppError::User(format!(
            "failed to create extensions directory {}: {e}",
            ext_dir.display()
        ))
    })?;
    let dest = ext_dir.join(format!("{EXTENSION_ID}.xpi"));

    // Write atomically: write to a per-process temp file then rename, so
    // concurrent launches don't race on the destination path.
    // Use safe_create (O_EXCL) because the tmp path is freshly randomized;
    // if it already exists something unexpected happened and we should fail fast.
    let tmp_path = dest.with_extension(format!("xpi.tmp.{}", std::process::id()));
    let mut tmp_file = safe_create(&tmp_path).map_err(|e| {
        AppError::User(format!(
            "failed to create temp file for Consent-O-Matic at {}: {e}",
            tmp_path.display()
        ))
    })?;
    tmp_file.write_all(XPI_BYTES).map_err(|e| {
        AppError::User(format!(
            "failed to write Consent-O-Matic to {}: {e}",
            tmp_path.display()
        ))
    })?;
    // Close the file handle before renaming — Windows refuses to rename a file
    // that still has an open handle in the current process.
    drop(tmp_file);
    std::fs::rename(&tmp_path, &dest).map_err(|e| {
        AppError::User(format!(
            "failed to install Consent-O-Matic to {}: {e}",
            dest.display()
        ))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn sha256_hex(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let digest = hasher.finalize();
        let mut s = String::with_capacity(64);
        for b in digest {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
        }
        s
    }

    /// XPI files are ZIPs; the magic bytes are `PK\x03\x04`.
    #[test]
    fn vendored_xpi_is_a_zip() {
        assert!(
            XPI_BYTES.starts_with(b"PK\x03\x04"),
            "vendored XPI does not start with the ZIP magic bytes"
        );
    }

    /// The vendored bytes must match the SHA-256 captured in
    /// `LICENSE-consent-o-matic.txt`. If this fails, the binary asset has
    /// drifted from the documented provenance — re-vendor the upstream
    /// release and update both files together.
    #[test]
    fn vendored_xpi_matches_pinned_sha256() {
        let got = sha256_hex(XPI_BYTES);
        assert_eq!(
            got, XPI_SHA256_HEX,
            "vendored Consent-O-Matic sha256 drifted: got {got}"
        );
    }

    /// The provenance text must also pin the same SHA-256, so a future
    /// re-vendor cannot quietly bump `XPI_BYTES` + `XPI_SHA256_HEX`
    /// without also updating the licence/provenance file. Embeds the
    /// text via `include_str!` so it's checked at compile time.
    #[test]
    fn vendored_xpi_provenance_file_is_in_sync() {
        const LICENSE_TEXT: &str =
            include_str!("../../assets/extensions/LICENSE-consent-o-matic.txt");
        assert!(
            LICENSE_TEXT.contains(XPI_SHA256_HEX),
            "LICENSE-consent-o-matic.txt does not document the current XPI sha256 ({XPI_SHA256_HEX}) — re-vendor must update both files together"
        );
    }

    #[test]
    fn install_writes_xpi_into_profile_extensions_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        install(tmp.path()).expect("install should succeed");
        let installed = tmp
            .path()
            .join("extensions")
            .join(format!("{EXTENSION_ID}.xpi"));
        assert!(installed.is_file(), "{} missing", installed.display());
        let bytes = std::fs::read(&installed).expect("read installed xpi");
        assert_eq!(bytes.len(), XPI_BYTES.len());
        assert!(bytes.starts_with(b"PK\x03\x04"));
    }

    /// Reinstalling over an existing file must succeed (atomic rename).
    #[test]
    fn install_is_idempotent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        install(tmp.path()).expect("first install");
        install(tmp.path()).expect("second install");
    }
}
