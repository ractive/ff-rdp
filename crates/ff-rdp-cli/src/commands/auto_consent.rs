use std::path::Path;

use crate::error::AppError;

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
/// pinned version, replace both files and update `XPI_VERSION` below.
const XPI_BYTES: &[u8] = include_bytes!("../../assets/extensions/consent-o-matic-1.1.5.xpi");

#[cfg_attr(not(test), allow(dead_code))]
const XPI_VERSION: &str = "1.1.5";

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
    let tmp_path = dest.with_extension(format!("xpi.tmp.{}", std::process::id()));
    std::fs::write(&tmp_path, XPI_BYTES).map_err(|e| {
        AppError::User(format!(
            "failed to write Consent-O-Matic to {}: {e}",
            tmp_path.display()
        ))
    })?;
    std::fs::rename(&tmp_path, &dest).map_err(|e| {
        AppError::User(format!(
            "failed to install Consent-O-Matic to {}: {e}",
            dest.display()
        ))
    })?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unreadable_literal, clippy::many_single_char_names)]
mod tests {
    use super::*;

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
            "vendored Consent-O-Matic {XPI_VERSION} sha256 drifted: got {got}"
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

    /// Minimal pure-Rust SHA-256 used only by tests, so the production
    /// crate does not gain a runtime hash-crate dependency just for this
    /// assertion.
    fn sha256_hex(data: &[u8]) -> String {
        let digest = sha256(data);
        let mut s = String::with_capacity(64);
        for b in digest {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
        }
        s
    }

    fn sha256(data: &[u8]) -> [u8; 32] {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let mut h: [u32; 8] = [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ];

        let bit_len: u64 = (data.len() as u64) * 8;
        let mut msg = Vec::with_capacity(data.len() + 9 + 63);
        msg.extend_from_slice(data);
        msg.push(0x80);
        while msg.len() % 64 != 56 {
            msg.push(0);
        }
        msg.extend_from_slice(&bit_len.to_be_bytes());

        for chunk in msg.chunks_exact(64) {
            let mut w = [0u32; 64];
            for (i, word) in chunk.chunks_exact(4).enumerate() {
                w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
            }
            for i in 16..64 {
                let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
                let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
                w[i] = w[i - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[i - 7])
                    .wrapping_add(s1);
            }

            let mut a = h[0];
            let mut b = h[1];
            let mut c = h[2];
            let mut d = h[3];
            let mut e = h[4];
            let mut f = h[5];
            let mut g = h[6];
            let mut hh = h[7];

            for i in 0..64 {
                let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
                let ch = (e & f) ^ ((!e) & g);
                let t1 = hh
                    .wrapping_add(s1)
                    .wrapping_add(ch)
                    .wrapping_add(K[i])
                    .wrapping_add(w[i]);
                let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
                let maj = (a & b) ^ (a & c) ^ (b & c);
                let t2 = s0.wrapping_add(maj);
                hh = g;
                g = f;
                f = e;
                e = d.wrapping_add(t1);
                d = c;
                c = b;
                b = a;
                a = t1.wrapping_add(t2);
            }

            h[0] = h[0].wrapping_add(a);
            h[1] = h[1].wrapping_add(b);
            h[2] = h[2].wrapping_add(c);
            h[3] = h[3].wrapping_add(d);
            h[4] = h[4].wrapping_add(e);
            h[5] = h[5].wrapping_add(f);
            h[6] = h[6].wrapping_add(g);
            h[7] = h[7].wrapping_add(hh);
        }

        let mut out = [0u8; 32];
        for (i, word) in h.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    /// Sanity-check the test-only SHA-256 implementation against the
    /// canonical empty-string digest.
    #[test]
    fn sha256_empty_string_matches_nist_vector() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
