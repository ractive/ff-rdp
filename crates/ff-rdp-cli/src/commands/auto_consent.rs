use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

use ff_rdp_core::types::Grip;
use ff_rdp_core::{RdpConnection, RootActor, TabActor, WebConsoleActor};
use serde_json::Value;

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
#[cfg(test)]
const XPI_SHA256_HEX: &str = "a2119abc329638d6e7af1ab4e5548a348465e02eec11de08dee0af84919923dc";

/// Has [`install`] already placed the extension into `profile_dir`? When it
/// has not, the coming launch is the extension's first run there, and it will
/// open its options tab.
pub(crate) fn is_installed(profile_dir: &Path) -> bool {
    profile_dir
        .join("extensions")
        .join(format!("{EXTENSION_ID}.xpi"))
        .is_file()
}

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

/// Parent-process script that closes the options tab Consent-O-Matic opens
/// the first time it runs in a profile.
///
/// Why it opens: the vendored 1.1.5 `service.js` ends with
/// `getDebugValues().then(t => { t.autoOpenOptionsTab && (t.autoOpenOptionsTab = !1,
/// setDebugValues(t), chrome.tabs.create({url: getURL("options.html")})) })`,
/// and `autoOpenOptionsTab` defaults to `true` until it is stored in
/// `storage.sync`. Every fresh profile therefore gets the tab, about 1–2 s
/// after `launch` returns — which is why a `tabs` run straight after `launch`
/// sometimes did not see it yet. The flag lives in `storage-sync-v2.sqlite`,
/// so it cannot be pre-seeded from `user.js`, and the signed XPI cannot be
/// edited.
///
/// The script runs in the parent process: it closes the tab if it is already
/// open, otherwise polls every 50 ms for up to 60 s and closes it when it
/// appears, then stops — the timer outlives the RDP connection. Its value is a
/// Promise that resolves to `"closed"` as soon as it closed the tab, or to
/// `"watching"` once `__WAIT_MS__` passed without one (immediately for 0). It
/// matches the extension's exact `options.html` URL, never the last tab of a
/// window, and closes at most one tab.
const CLOSE_OPTIONS_TAB_JS: &str = r#"(() => {
  const ID = "__EXTENSION_ID__";
  const WAIT_MS = __WAIT_MS__;
  const { setInterval, clearInterval } = ChromeUtils.importESModule("resource://gre/modules/Timer.sys.mjs");
  const closeOptionsTab = () => {
    const policy = WebExtensionPolicy.getByID(ID);
    if (!policy) return false;
    const target = policy.getURL("options.html");
    for (const win of Services.wm.getEnumerator("navigator:browser")) {
      const tabs = win.gBrowser?.tabs ?? [];
      if (tabs.length < 2) continue;
      for (const tab of tabs) {
        if (tab.linkedBrowser?.currentURI?.spec === target) {
          win.gBrowser.removeTab(tab);
          return true;
        }
      }
    }
    return false;
  };
  if (closeOptionsTab()) return "closed";
  return new Promise(resolve => {
    const started = Date.now();
    let settled = false;
    const settle = verdict => {
      if (!settled) {
        settled = true;
        resolve(verdict);
      }
    };
    if (WAIT_MS === 0) settle("watching");
    const timer = setInterval(() => {
      if (closeOptionsTab()) {
        clearInterval(timer);
        settle("closed");
        return;
      }
      const elapsed = Date.now() - started;
      if (elapsed >= WAIT_MS) settle("watching");
      if (elapsed >= 60000) clearInterval(timer);
    }, 50);
  });
})()"#;

/// [`CLOSE_OPTIONS_TAB_JS`] for this extension, resolving after at most `wait`.
fn close_options_tab_js(wait: Duration) -> String {
    CLOSE_OPTIONS_TAB_JS
        .replace("__EXTENSION_ID__", EXTENSION_ID)
        .replace("__WAIT_MS__", &wait.as_millis().to_string())
}

/// Arm the options-tab closer in the Firefox listening on `127.0.0.1:port`
/// and wait up to `wait` for it to close the tab.
///
/// Returns the verdict: `"closed"`, or `"watching"` when `wait` passed first
/// (the closer keeps running inside Firefox). Needs `devtools.chrome.enabled`,
/// which `launch` writes into every profile.
pub(crate) fn suppress_options_tab(port: u16, wait: Duration) -> Result<String, String> {
    // One round trip that may legitimately take `wait`; the margin covers the
    // connect and the two actor lookups.
    let timeout = wait + Duration::from_secs(5);
    let mut connection =
        RdpConnection::connect("127.0.0.1", port, timeout).map_err(|e| format!("connect: {e}"))?;
    let transport = connection.transport_mut();
    let script = close_options_tab_js(wait);
    let evaluated = RootActor::get_process(transport, 0)
        .and_then(|descriptor| TabActor::get_process_target(transport, &descriptor))
        .and_then(|target| {
            WebConsoleActor::evaluate_js_async(transport, &target.console_actor, &script)
        })
        .map_err(|e| format!("parent-process evaluation: {e}"))?;
    if let Some(exception) = evaluated.exception {
        return Err(format!("parent-process script threw: {exception:?}"));
    }
    match evaluated.result {
        Grip::Value(Value::String(verdict)) => Ok(verdict),
        other => Err(format!("unexpected parent-process result: {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::sha256_hex;

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
        assert!(!is_installed(tmp.path()));
        install(tmp.path()).expect("install should succeed");
        assert!(is_installed(tmp.path()));
        let installed = tmp
            .path()
            .join("extensions")
            .join(format!("{EXTENSION_ID}.xpi"));
        assert!(installed.is_file(), "{} missing", installed.display());
        let bytes = std::fs::read(&installed).expect("read installed xpi");
        assert_eq!(bytes.len(), XPI_BYTES.len());
        assert!(bytes.starts_with(b"PK\x03\x04"));
    }

    /// The closer must target this extension's options page and carry the
    /// requested wait — a stale placeholder would be a JS syntax error that
    /// only a live Firefox would report.
    #[test]
    fn close_options_tab_js_fills_every_placeholder() {
        let js = close_options_tab_js(Duration::from_millis(10_000));
        assert!(!js.contains("__"), "unfilled placeholder in {js}");
        assert!(js.contains(&format!("const ID = \"{EXTENSION_ID}\";")));
        assert!(js.contains("const WAIT_MS = 10000;"));
        assert!(js.contains("policy.getURL(\"options.html\")"));
        assert!(close_options_tab_js(Duration::ZERO).contains("const WAIT_MS = 0;"));
    }

    /// Reinstalling over an existing file must succeed (atomic rename).
    #[test]
    fn install_is_idempotent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        install(tmp.path()).expect("first install");
        install(tmp.path()).expect("second install");
    }
}
