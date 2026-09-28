//! Explicit, local English langpack provisioning for a fresh managed profile.
//!
//! Firefox SourceStamp 6f2c158dfc7e9693f880fad2510ceb51a158c069:
//! HTMLMediaElement.cpp:1715–1725/3204 uses DOM_PROPERTIES; nsContentUtils.cpp:
//! 5705–5730/5903–5917 selects chrome://global/locale/dom/dom.properties.
//! nsChromeRegistryChrome.cpp:351–374 can fall back to another registered locale.
//! An English application locale alone therefore does not supply missing legacy
//! English resources. Staging a pack is not evidence that Firefox accepted it.
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::error::AppError;

pub(super) const ID: &str = "langpack-en-US@firefox.mozilla.org";
const GLOBAL: &str = "chrome/en-US/locale/en-US/global/";
const DOM: &str = "chrome/en-US/locale/en-US/global/dom/dom.properties";
const MAX_XPI: u64 = 4 * 1024 * 1024;
const MAX_EXPANDED: u64 = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
/// Installed AddonManager.sys.mjs:4226–4232: ALL=15, PROFILE=1.
/// Exclude only PROFILE from auto-disabling; never disable signature checks.
pub(super) const PROFILE_ACTIVATION: &str = "// Allow only profile-scope sideload activation for the explicitly supplied English language pack.\nuser_pref(\"extensions.autoDisableScopes\", 14);\n";

pub(super) struct Pack {
    bytes: Vec<u8>,
    version: String,
    minimum: Vec<u32>,
    maximum_major: u32,
    sha256: String,
}

fn user_error(error: &anyhow::Error) -> AppError {
    AppError::User(format!("English language pack: {error:#}"))
}

/// An immutable bounded read, with Unix nonblocking/no-follow protection against
/// a leaf swapped to a FIFO or symlink. Never execute archive content.
pub(super) fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>> {
    ensure!(
        fs::symlink_metadata(path)?.file_type().is_file(),
        "not a regular file: {}",
        path.display()
    );
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() <= limit,
        "file exceeds bound or is not regular"
    );
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        u64::try_from(bytes.len())? <= limit,
        "file grew beyond bound"
    );
    Ok(bytes)
}

fn numeric_version(value: &str) -> Result<Vec<u32>> {
    let parts: Vec<_> = value.split('.').collect();
    ensure!(
        (2..=4).contains(&parts.len()),
        "unsupported Firefox release version {value}"
    );
    parts
        .into_iter()
        .map(|p| {
            ensure!(
                !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()),
                "unsupported Firefox release version {value}"
            );
            Ok(p.parse()?)
        })
        .collect()
}

fn safe_member(name: &str) -> bool {
    let path = name.strip_suffix('/').unwrap_or(name);
    !name.is_empty()
        && !name.starts_with('/')
        && !name.chars().any(char::is_control)
        && !name.contains(['\\', ':', '%', '?', '#'])
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

// zip0.6.6 read.rs:401–430 allocates its entry Vec/HashMap from footer
// counts before reading central headers. Bound that constructor's inputs first.
// This checks only the ordinary ZIP metadata envelope; the library still owns
// member decoding, CRC and decompression. ZIP64/multidisk layouts are unsupported.
fn zip_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let raw: [u8; 2] = bytes
        .get(offset..offset + 2)
        .context("truncated ZIP metadata")?
        .try_into()?;
    Ok(u16::from_le_bytes(raw))
}

fn zip_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let raw: [u8; 4] = bytes
        .get(offset..offset + 4)
        .context("truncated ZIP metadata")?
        .try_into()?;
    Ok(u32::from_le_bytes(raw))
}

fn ordinary_zip_entries(bytes: &[u8]) -> Result<usize> {
    ensure!(
        bytes.len() >= 22 && u64::try_from(bytes.len())? <= MAX_XPI,
        "ZIP input size out of bounds"
    );
    // Match spec.rs CentralDirectoryEnd::find_and_parse's FIRST backwards
    // signature, not a more attractive earlier footer. An apparent footer inside
    // a comment must never let the constructor select different unchecked counts.
    let lower = bytes.len().saturating_sub(22 + usize::from(u16::MAX));
    let footer = (lower..=bytes.len() - 22)
        .rev()
        .find(|&offset| bytes[offset..offset + 4] == *b"PK\x05\x06")
        .context("ordinary ZIP footer absent")?;
    let comment = usize::from(zip_u16(bytes, footer + 20)?);
    ensure!(
        footer + 22 + comment == bytes.len(),
        "ZIP footer/comment extent mismatch"
    );
    ensure!(
        footer < 20 || bytes[footer - 20..footer - 16] != *b"PK\x06\x07",
        "ZIP64 locator unsupported"
    );
    ensure!(
        zip_u16(bytes, footer + 4)? == 0 && zip_u16(bytes, footer + 6)? == 0,
        "multidisk ZIP unsupported"
    );
    let entries = usize::from(zip_u16(bytes, footer + 10)?);
    ensure!(
        entries > 0
            && entries <= MAX_ENTRIES
            && usize::from(zip_u16(bytes, footer + 8)?) == entries,
        "ZIP entry count out of bounds or inconsistent"
    );
    let size = zip_u32(bytes, footer + 12)?;
    let offset = zip_u32(bytes, footer + 16)?;
    ensure!(
        size != u32::MAX && offset != u32::MAX,
        "ZIP64 directory unsupported"
    );
    let size = usize::try_from(size)?;
    let mut cursor = usize::try_from(offset)?;
    ensure!(
        cursor.checked_add(size) == Some(footer),
        "ordinary ZIP central directory extent mismatch"
    );
    ensure!(
        size >= entries * 46,
        "ZIP central directory too small for declared entries"
    );
    for _ in 0..entries {
        ensure!(
            cursor.checked_add(46).is_some_and(|end| end <= footer)
                && bytes[cursor..cursor + 4] == *b"PK\x01\x02",
            "invalid central directory header"
        );
        ensure!(
            zip_u16(bytes, cursor + 6)? < 45
                && zip_u32(bytes, cursor + 20)? != u32::MAX
                && zip_u32(bytes, cursor + 24)? != u32::MAX
                && zip_u32(bytes, cursor + 42)? != u32::MAX,
            "ZIP64 entry unsupported"
        );
        ensure!(
            zip_u16(bytes, cursor + 34)? == 0,
            "multidisk entry unsupported"
        );
        let name_len = usize::from(zip_u16(bytes, cursor + 28)?);
        let extra_len = usize::from(zip_u16(bytes, cursor + 30)?);
        let comment_len = usize::from(zip_u16(bytes, cursor + 32)?);
        let extra_start = cursor + 46 + name_len;
        let extra_end = extra_start + extra_len;
        let end = extra_end + comment_len;
        ensure!(
            end <= footer,
            "central directory variable metadata exceeds its extent"
        );
        let mut extra = extra_start;
        while extra < extra_end {
            ensure!(extra + 4 <= extra_end, "truncated ZIP extra-field header");
            let kind = zip_u16(bytes, extra)?;
            let length = usize::from(zip_u16(bytes, extra + 2)?);
            ensure!(kind != 1, "ZIP64 extra field unsupported");
            extra += 4 + length;
            ensure!(extra <= extra_end, "ZIP extra field exceeds its extent");
        }
        cursor = end;
    }
    ensure!(cursor == footer, "unaccounted central directory metadata");
    Ok(entries)
}

/// The same entry point is used by production and the injected-constructor
/// control, so rejected metadata cannot reach library allocation in either path.
fn construct_bounded_zip<'a, T>(
    bytes: &'a [u8],
    construct: impl FnOnce(&'a [u8], usize) -> Result<T>,
) -> Result<T> {
    let entries = ordinary_zip_entries(bytes)?;
    construct(bytes, entries)
}

impl Pack {
    pub(super) fn read(path: &Path) -> Result<Self, AppError> {
        read_regular(path, MAX_XPI)
            .and_then(Self::parse)
            .map_err(|error| user_error(&error))
    }

    fn parse(bytes: Vec<u8>) -> Result<Self> {
        ensure!(
            u64::try_from(bytes.len())? <= MAX_XPI,
            "archive exceeds compressed bound"
        );
        let mut archive = construct_bounded_zip(&bytes, |input, entries| {
            let archive = zip::ZipArchive::new(Cursor::new(input))?;
            ensure!(
                archive.len() == entries,
                "ZIP constructor entry count differs from bounded metadata"
            );
            Ok(archive)
        })?;
        ensure!(
            !archive.is_empty() && archive.len() <= MAX_ENTRIES,
            "archive entry count out of bounds"
        );
        let mut names = BTreeSet::new();
        let mut declared = 0_u64;
        let mut actual = 0_u64;
        let mut manifest = None;
        let mut dom_present = false;
        for index in 0..archive.len() {
            // by_index rejects encrypted members without a password. Read every
            // member to EOF (including CRC), bounded independently of its sizes.
            let mut entry = archive.by_index(index)?;
            let name = std::str::from_utf8(entry.name_raw())?.to_owned();
            ensure!(
                safe_member(&name) && names.insert(name.trim_end_matches('/').to_owned()),
                "unsafe or duplicate ZIP member {name}"
            );
            let kind = entry.unix_mode().unwrap_or(0) & 0o170_000;
            ensure!(
                matches!(kind, 0 | 0o100_000 | 0o040_000),
                "nonregular ZIP member {name}"
            );
            declared = declared
                .checked_add(entry.size())
                .context("ZIP size overflow")?;
            ensure!(declared <= MAX_EXPANDED, "declared expansion exceeds bound");
            let member_limit = match name.as_str() {
                "manifest.json" => 64 * 1024,
                DOM => 1024 * 1024,
                _ => MAX_EXPANDED,
            };
            ensure!(entry.size() <= member_limit, "member exceeds bound: {name}");
            let mut content = Vec::new();
            let mut count = 0_u64;
            let mut buffer = [0; 8192];
            loop {
                let n = entry.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                count += u64::try_from(n)?;
                actual += u64::try_from(n)?;
                ensure!(
                    count <= member_limit && actual <= MAX_EXPANDED,
                    "actual expansion exceeds bound"
                );
                if name == "manifest.json" {
                    content.extend_from_slice(&buffer[..n]);
                }
            }
            ensure!(count == entry.size(), "member size mismatch");
            if name == "manifest.json" {
                manifest = Some(content);
            }
            if name == DOM {
                dom_present = count > 0;
            }
        }
        drop(archive);
        for container in [
            "META-INF/manifest.mf",
            "META-INF/mozilla.sf",
            "META-INF/mozilla.rsa",
        ] {
            ensure!(
                names.contains(container),
                "signature container absent: {container}"
            );
        }
        // Container presence is NOT cryptographic signature validation. Firefox
        // retains its signed language-pack policy and decides whether to activate.
        let manifest: Value = serde_json::from_slice(&manifest.context("manifest absent")?)?;
        ensure!(
            manifest["manifest_version"] == 2 && manifest["langpack_id"] == "en-US",
            "not an English language pack"
        );
        for forbidden in [
            "background",
            "content_scripts",
            "permissions",
            "host_permissions",
            "experiment_apis",
        ] {
            ensure!(
                manifest.get(forbidden).is_none(),
                "unexpected extension capability {forbidden}"
            );
        }
        let gecko = &manifest["browser_specific_settings"]["gecko"];
        ensure!(gecko["id"] == ID, "unexpected add-on ID");
        let languages = manifest["languages"]
            .as_object()
            .context("languages object absent")?;
        ensure!(
            languages.len() == 1 && languages.contains_key("en-US"),
            "locale must be exactly en-US"
        );
        ensure!(
            languages["en-US"]["chrome_resources"]["global"] == GLOBAL && dom_present,
            "English global DOM provider missing"
        );
        let resources = languages["en-US"]["chrome_resources"]
            .as_object()
            .context("chrome resources absent")?;
        let directory = |value: &Value| -> Result<()> {
            let path = value.as_str().context("resource directory is not text")?;
            ensure!(
                path.ends_with('/') && safe_member(path),
                "unsafe resource directory"
            );
            Ok(())
        };
        for (alias, value) in resources {
            ensure!(
                !alias.is_empty()
                    && alias
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
                "unsafe chrome resource alias"
            );
            if let Some(platforms) = value.as_object() {
                ensure!(!platforms.is_empty(), "empty platform resources");
                for (platform, path) in platforms {
                    ensure!(
                        ["macosx", "linux", "android", "win"].contains(&platform.as_str()),
                        "unsupported resource platform"
                    );
                    directory(path)?;
                }
            } else {
                directory(value)?;
            }
        }
        if let Some(sources) = manifest.get("sources") {
            for source in sources
                .as_object()
                .context("sources not an object")?
                .values()
            {
                directory(&source["base_path"])?;
            }
        }
        let minimum = numeric_version(
            gecko["strict_min_version"]
                .as_str()
                .context("minimum version absent")?,
        )?;
        let maximum = gecko["strict_max_version"]
            .as_str()
            .context("maximum version absent")?;
        let major = maximum
            .strip_suffix(".*")
            .context("maximum version must be a release-major wildcard")?;
        ensure!(
            !major.is_empty() && major.bytes().all(|b| b.is_ascii_digit()),
            "unsupported maximum version"
        );
        let maximum_major: u32 = major.parse()?;
        ensure!(
            minimum[0] == maximum_major,
            "language pack must target one Firefox release major"
        );
        let version = manifest["version"]
            .as_str()
            .context("pack version absent")?;
        ensure!(
            !version.is_empty()
                && version.len() <= 128
                && version.bytes().all(|b| b.is_ascii_digit() || b == b'.'),
            "unsupported pack version"
        );
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        Ok(Self {
            bytes,
            version: version.to_owned(),
            minimum,
            maximum_major,
            sha256,
        })
    }

    pub(super) fn check_firefox(&self, firefox: &Path) -> Result<(), AppError> {
        self.check_firefox_inner(firefox)
            .map_err(|error| user_error(&error))
    }

    fn check_firefox_inner(&self, firefox: &Path) -> Result<()> {
        let binary = fs::canonicalize(firefox)?;
        let parent = binary.parent().context("Firefox directory absent")?;
        let ini: PathBuf = if parent.file_name().is_some_and(|n| n == "MacOS") {
            parent
                .parent()
                .context("Firefox Contents absent")?
                .join("Resources/application.ini")
        } else {
            parent.join("application.ini")
        };
        let bytes = read_regular(&ini, 64 * 1024)?;
        let mut in_app = false;
        let mut versions = Vec::new();
        for line in std::str::from_utf8(&bytes)?.lines().map(str::trim) {
            if line.starts_with('[') {
                in_app = line == "[App]";
            }
            if in_app && let Some(value) = line.strip_prefix("Version=") {
                versions.push(value);
            }
        }
        ensure!(
            versions.len() == 1,
            "application.ini must have one App Version"
        );
        self.check_version(versions[0])
    }

    fn check_version(&self, version: &str) -> Result<()> {
        let mut actual = numeric_version(version)?;
        let mut minimum = self.minimum.clone();
        actual.resize(4, 0);
        minimum.resize(4, 0);
        ensure!(
            actual[0] == self.maximum_major && actual >= minimum,
            "pack does not support Firefox {version}"
        );
        Ok(())
    }

    /// Only called inside the fresh managed-profile guard, before any spawn.
    pub(super) fn stage(&self, profile: &Path, base_prefs: &str) -> Result<(), AppError> {
        self.stage_inner(profile, base_prefs)
            .map_err(|error| user_error(&error))
    }

    fn stage_inner(&self, profile: &Path, base_prefs: &str) -> Result<()> {
        ensure!(
            read_regular(&profile.join("user.js"), 64 * 1024)? == base_prefs.as_bytes(),
            "managed preferences differ before staging"
        );
        let directory = profile.join("extensions");
        // create_dir refuses an existing directory/symlink; this mode owns a
        // newly allocated profile, never a pre-existing user's extensions tree.
        fs::create_dir(&directory)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
        temporary.write_all(&self.bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist_noclobber(directory.join(format!("{ID}.xpi")))?;
        let mut prefs =
            super::launch::open_user_js_append(profile, "English language-pack activation")
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        prefs.write_all(PROFILE_ACTIVATION.as_bytes())?;
        let expected = format!("{base_prefs}{PROFILE_ACTIVATION}");
        ensure!(
            read_regular(&profile.join("user.js"), 64 * 1024)? == expected.as_bytes(),
            "managed mode preferences differ after staging"
        );
        Ok(())
    }

    pub(super) fn version(&self) -> &str {
        &self.version
    }

    pub(super) fn verify_staged(&self, profile: &Path, base_prefs: &str) -> Result<()> {
        ensure!(
            read_regular(&profile.join("user.js"), 64 * 1024)?
                == format!("{base_prefs}{PROFILE_ACTIVATION}").as_bytes(),
            "managed mode preferences changed during initialization"
        );
        ensure!(
            read_regular(
                &profile.join("extensions").join(format!("{ID}.xpi")),
                MAX_XPI
            )? == self.bytes,
            "staged language pack changed during initialization"
        );
        Ok(())
    }

    pub(super) fn staged_result(&self) -> Value {
        json!({"id":ID,"version":self.version,"sha256":self.sha256,"status":"staged"})
    }
}

#[cfg(test)]
pub(super) fn fixture(manifest: &Value, extras: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(manifest).unwrap()),
        (DOM, b"test-only provider".to_vec()),
        ("META-INF/manifest.mf", b"unsigned fixture".to_vec()),
        ("META-INF/mozilla.sf", b"unsigned fixture".to_vec()),
        ("META-INF/mozilla.rsa", b"unsigned fixture".to_vec()),
    ] {
        writer.start_file(name, options).unwrap();
        writer.write_all(&bytes).unwrap();
    }
    for (name, bytes) in extras {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[cfg(test)]
pub(super) fn fixture_manifest() -> Value {
    json!({"manifest_version":2,"langpack_id":"en-US","version":"156.0.20260921.121718",
        "browser_specific_settings":{"gecko":{"id":ID,"strict_min_version":"156.0","strict_max_version":"156.*"}},
        "languages":{"en-US":{"chrome_resources":{"global":GLOBAL}}}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_147_langpack_archive_identity_and_paths_fail_closed() {
        let valid = fixture_manifest();
        Pack::parse(fixture(&valid, &[])).unwrap();
        for pointer in [
            "/langpack_id",
            "/browser_specific_settings/gecko/id",
            "/languages/en-US/chrome_resources/global",
        ] {
            let mut invalid = valid.clone();
            *invalid.pointer_mut(pointer).unwrap() = json!("unrelated");
            assert!(Pack::parse(fixture(&invalid, &[])).is_err());
        }
        let mut invalid = valid.clone();
        invalid["languages"]["de"] = json!({});
        assert!(Pack::parse(fixture(&invalid, &[])).is_err());
        invalid = valid.clone();
        invalid["background"] = json!({"scripts":["code.js"]});
        assert!(Pack::parse(fixture(&invalid, &[])).is_err());
        for path in [
            "../escape",
            "/absolute",
            "a\\b",
            "C:escape",
            "manifest.json",
            "a//b",
            "a/./b",
        ] {
            assert!(
                Pack::parse(fixture(&valid, &[(path, b"content")])).is_err(),
                "accepted {path}"
            );
        }
        // Corrupt only metadata in a crate-produced ZIP to exercise encrypted
        // and Unix-symlink rejection without an extraction or native process.
        let bytes = fixture(&valid, &[]);
        let central = bytes.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
        let local = bytes.windows(4).position(|b| b == b"PK\x03\x04").unwrap();
        let mut encrypted = bytes.clone();
        encrypted[central + 8] |= 1;
        encrypted[local + 6] |= 1;
        assert!(Pack::parse(encrypted).is_err());
        let mut symlink = bytes;
        symlink[central + 5] = 3; // Unix creator
        symlink[central + 38..central + 42].copy_from_slice(&(0o120_777_u32 << 16).to_le_bytes());
        assert!(Pack::parse(symlink).is_err());
        let mut invalid = valid;
        invalid["languages"]["en-US"]["chrome_resources"]["browser"] = json!("../outside/");
        assert!(Pack::parse(fixture(&invalid, &[])).is_err());
        assert!(Pack::parse(b"not a ZIP".to_vec()).is_err());
    }

    #[test]
    fn unit_147_zip_metadata_rejected_before_constructor_allocation() {
        let valid = fixture(&fixture_manifest(), &[]);
        let footer = valid.len() - 22;
        let central = usize::try_from(zip_u32(&valid, footer + 16).unwrap()).unwrap();
        // The production boundary admits ordinary metadata, including its true
        // count, before the actual-XPI control exercises the real constructor.
        assert_eq!(
            construct_bounded_zip(&valid, |_, count| Ok(count)).unwrap(),
            5
        );
        let mut cases = Vec::new();
        let mut excessive = vec![0; 100_000];
        let mut excessive_footer = [0; 22];
        excessive_footer[..4].copy_from_slice(b"PK\x05\x06");
        excessive_footer[8..10].copy_from_slice(&8192_u16.to_le_bytes());
        excessive_footer[10..12].copy_from_slice(&8192_u16.to_le_bytes());
        excessive_footer[12..16].copy_from_slice(&100_000_u32.to_le_bytes());
        excessive.extend_from_slice(&excessive_footer);
        // 8192 <= EOCD offset: the audited dependency would reserve this count
        // before discovering the missing central headers. Our gate must not.
        cases.push(excessive);
        // A complete ZIP64 end record/locator overrides benign ordinary counts
        // in the audited dependency. Its larger count must never be consulted.
        let mut zip64 = vec![0; 100_000];
        zip64.extend_from_slice(b"PK\x06\x06");
        zip64.extend_from_slice(&44_u64.to_le_bytes());
        zip64.extend_from_slice(&45_u16.to_le_bytes());
        zip64.extend_from_slice(&45_u16.to_le_bytes());
        zip64.extend_from_slice(&[0; 8]); // both disk numbers
        for value in [8192_u64, 8192, 100_000, 0] {
            zip64.extend_from_slice(&value.to_le_bytes());
        }
        zip64.extend_from_slice(b"PK\x06\x07");
        zip64.extend_from_slice(&0_u32.to_le_bytes());
        zip64.extend_from_slice(&100_000_u64.to_le_bytes());
        zip64.extend_from_slice(&1_u32.to_le_bytes());
        let mut benign_footer = excessive_footer;
        benign_footer[8..10].copy_from_slice(&5_u16.to_le_bytes());
        benign_footer[10..12].copy_from_slice(&5_u16.to_le_bytes());
        zip64.extend_from_slice(&benign_footer);
        cases.push(zip64);
        let mut multidisk = valid.clone();
        multidisk[footer + 4..footer + 6].copy_from_slice(&1_u16.to_le_bytes());
        cases.push(multidisk);
        let mut member64 = valid.clone();
        member64[central + 24..central + 28].copy_from_slice(&u32::MAX.to_le_bytes());
        cases.push(member64);
        let mut extra64 = valid.clone();
        let name_len = usize::from(zip_u16(&valid, central + 28).unwrap());
        let extra_len = zip_u16(&valid, central + 30).unwrap();
        let extra_start = central + 46 + name_len;
        drop(extra64.splice(extra_start..extra_start, [1, 0, 0, 0]));
        extra64[central + 30..central + 32].copy_from_slice(&(extra_len + 4).to_le_bytes());
        let expanded_size = zip_u32(&valid, footer + 12).unwrap() + 4;
        extra64[footer + 4 + 12..footer + 4 + 16].copy_from_slice(&expanded_size.to_le_bytes());
        cases.push(extra64);
        let mut metadata = valid.clone();
        metadata[central + 28..central + 30].copy_from_slice(&u16::MAX.to_le_bytes());
        cases.push(metadata);
        let mut mismatched_count = valid.clone();
        mismatched_count[footer + 8..footer + 10].copy_from_slice(&1_u16.to_le_bytes());
        cases.push(mismatched_count);
        let mut hidden_footer = valid.clone();
        hidden_footer[footer + 20..footer + 22].copy_from_slice(&22_u16.to_le_bytes());
        hidden_footer.extend_from_slice(&excessive_footer);
        cases.push(hidden_footer); // constructor chooses the footer in the comment
        cases.push(valid[..valid.len() - 1].to_vec());
        for malformed in cases {
            let called = std::cell::Cell::new(false);
            let result = construct_bounded_zip(&malformed, |_, _| {
                called.set(true);
                Ok(())
            });
            assert!(result.is_err());
            assert!(
                !called.get(),
                "untrusted metadata reached constructor allocation"
            );
        }
    }

    /// Opt-in resource control: reads a local fixture only, never launches Firefox.
    /// Separate from live locale acceptance; absence fails explicit selection.
    #[test]
    #[ignore = "requires explicitly supplied official Firefox 156.0.1 en-US XPI fixture"]
    fn unit_147_official_pack_preflight() {
        let path = std::env::var_os("FF_RDP_ENGLISH_PACK_TEST_XPI")
            .expect("local official fixture required");
        let pack = Pack::read(Path::new(&path)).unwrap();
        assert_eq!(pack.bytes.len(), 689_787);
        assert_eq!(
            pack.sha256,
            "6a1f941b8748d312799d4c4fd432c963cac03a17573f8fd125686643ce5743a3"
        );
        assert_eq!(pack.version, "156.0.20260921.121718");
        pack.check_version("156.0.1").unwrap();
    }

    #[test]
    fn unit_147_langpack_compressed_and_expanded_bounds() {
        assert!(Pack::parse(vec![0; usize::try_from(MAX_XPI).unwrap() + 1]).is_err());
        let expansion = vec![b'a'; usize::try_from(MAX_EXPANDED).unwrap()];
        // Small deflated input with aggregate expansion over the cap.
        assert!(Pack::parse(fixture(&fixture_manifest(), &[("oversized", &expansion)])).is_err());
        let huge_manifest = json!({"padding":"x".repeat(65536)});
        assert!(Pack::parse(fixture(&huge_manifest, &[])).is_err());
    }

    #[test]
    fn unit_147_langpack_release_compatibility_is_fail_closed() {
        let pack = Pack::parse(fixture(&fixture_manifest(), &[])).unwrap();
        for version in ["156.0", "156.0.1", "156.10.2"] {
            pack.check_version(version).unwrap();
        }
        for version in ["155.9", "157.0", "156.0a1", "156.0esr", "156", "156..0"] {
            assert!(pack.check_version(version).is_err(), "accepted {version}");
        }
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("firefox");
        fs::write(&binary, "not executed").unwrap();
        assert!(pack.check_firefox(&binary).is_err());
        fs::write(
            root.path().join("application.ini"),
            "[App]\nVersion=156.0.1\n",
        )
        .unwrap();
        pack.check_firefox(&binary).unwrap();
        fs::write(
            root.path().join("application.ini"),
            "[App]\nVersion=156.0.1\nVersion=156.0\n",
        )
        .unwrap();
        assert!(pack.check_firefox(&binary).is_err());
    }

    #[test]
    fn unit_147_langpack_staging_is_atomic_exact_and_only_staged() {
        let base = super::super::launch::USER_JS;
        let pack = Pack::parse(fixture(&fixture_manifest(), &[])).unwrap();
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("user.js"), base).unwrap();
        pack.stage(root.path(), base).unwrap();
        assert_eq!(
            fs::read(root.path().join(format!("extensions/{ID}.xpi"))).unwrap(),
            pack.bytes
        );
        assert_eq!(
            fs::read_to_string(root.path().join("user.js")).unwrap(),
            format!("{base}{PROFILE_ACTIVATION}")
        );
        assert_eq!(
            fs::read_dir(root.path().join("extensions"))
                .unwrap()
                .count(),
            1
        );
        assert_eq!(
            pack.staged_result(),
            json!({"id":ID,"version":pack.version,"sha256":pack.sha256,"status":"staged"})
        );
        assert!(pack.stage(root.path(), base).is_err()); // no overwrite / duplicate pref
        assert_eq!(
            fs::read_to_string(root.path().join("user.js")).unwrap(),
            format!("{base}{PROFILE_ACTIVATION}")
        );
        // Exact reviewed mode bytes include all original prefs, not a substring.
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(format!("{base}{PROFILE_ACTIVATION}"))
            ),
            "8116871929126e8d976ebd4c81af990a72c201ceaa18cef7e61f39bebaf378ba"
        );
    }
}
