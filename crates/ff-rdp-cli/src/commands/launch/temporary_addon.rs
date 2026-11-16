//! `launch --temporary-addon <path>` (iteration 297): load unsigned
//! WebExtensions into the Firefox `launch` started, over RDP — the
//! about:debugging / `web-ext run` path — so release Firefox runs them without
//! signing. Temporary installs vanish when Firefox exits.
//!
//! The order matters: [`resolve`] runs before anything else happens (a bad path
//! never starts a browser), Firefox starts without the startup URL, then
//! [`install_and_navigate`] installs every add-on and only afterwards opens the
//! URL, so a `document_start` content script already sees the first page.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ff_rdp_core::{AddonsActor, ProtocolError, RdpConnection, RootActor, TabActor};
use serde_json::{Value, json};

use crate::error::AppError;

/// A `--temporary-addon` argument checked to exist, as given and as sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TemporaryAddon {
    /// The path as the user typed it (reported back and named in errors).
    pub(crate) given: String,
    /// Absolute, canonical native path handed to `installTemporaryAddon`.
    pub(crate) absolute: String,
}

/// Validate every `--temporary-addon` path before Firefox starts: it must
/// exist, and a directory must hold a `manifest.json`. Anything else (a
/// malformed manifest, a file that is not an extension) is Firefox's call.
pub(crate) fn resolve(paths: &[PathBuf]) -> Result<Vec<TemporaryAddon>, AppError> {
    paths.iter().map(|path| resolve_one(path)).collect()
}

fn resolve_one(path: &Path) -> Result<TemporaryAddon, AppError> {
    let given = path.display().to_string();
    let canonical = path.canonicalize().map_err(|e| {
        AppError::User(format!(
            "--temporary-addon {given}: cannot read path ({e}); expected an extension \
             directory containing manifest.json or an .xpi file"
        ))
    })?;
    if canonical.is_dir() && !canonical.join("manifest.json").is_file() {
        return Err(AppError::User(format!(
            "--temporary-addon {given}: directory has no manifest.json; expected an \
             extension directory containing manifest.json or an .xpi file"
        )));
    }
    let absolute = strip_verbatim_prefix(&canonical.to_string_lossy());
    Ok(TemporaryAddon { given, absolute })
}

/// `canonicalize` on Windows returns `\\?\C:\…` / `\\?\UNC\server\share\…`
/// verbatim paths. Firefox's `nsIFile` expects an ordinary native path, so
/// turn them back into `C:\…` and `\\server\share\…`. A no-op elsewhere.
fn strip_verbatim_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        path.to_owned()
    }
}

/// Install `addons` in order into the Firefox listening on `127.0.0.1:port`,
/// then navigate its selected tab (or first tab) to `url`, if one was given.
///
/// Returns one `{path, id, actor}` object per add-on for `results.temporary_addons`.
/// The first rejected install stops the sequence and becomes the error, with
/// Firefox's own message.
///
/// After `navigateTo` it waits (up to `timeout`) for the tab to report a new
/// URL, i.e. for the navigation to commit. Returning earlier hands the caller a
/// tab mid target-switch: a follow-up `eval`/`dom` then hit `noSuchActor`,
/// `tabDestroyed` or a 10 s recv timeout in 5 of 5 runs (Firefox 160). A URL
/// that has not committed by then is a warning, not a failure — launch never
/// promised a loaded document.
pub(crate) fn install_and_navigate(
    port: u16,
    addons: &[TemporaryAddon],
    url: Option<&str>,
    timeout: Duration,
) -> Result<Installed, AppError> {
    let mut connection = RdpConnection::connect("127.0.0.1", port, timeout)?;
    let transport = connection.transport_mut();
    let addons_actor = AddonsActor::from_root(transport)?;
    let mut installed = Vec::with_capacity(addons.len());
    let mut warning = None;
    for addon in addons {
        let reply = AddonsActor::install_temporary_addon(transport, &addons_actor, &addon.absolute)
            .map_err(|error| install_error(&addon.given, error))?;
        installed.push(json!({
            "path": addon.absolute,
            "id": reply.id,
            // `false` on every Firefox to date (addons.js:75): the install
            // reply carries no actor. Passed through rather than invented.
            "actor": reply.actor,
        }));
    }
    if let Some(url) = url {
        let tabs = RootActor::list_tabs(transport)?;
        let tab = tabs
            .iter()
            .find(|tab| tab.selected)
            .or_else(|| tabs.first())
            .ok_or_else(|| {
                AppError::User(
                    "--temporary-addon: Firefox listed no tab to open the URL in".to_owned(),
                )
            })?;
        let (tab_actor, before) = (tab.actor.clone(), tab.url.clone());
        let target = TabActor::get_target(transport, &tab_actor)?;
        ff_rdp_core::WindowGlobalTarget::navigate_to(transport, &target.actor, url)?;
        if url != before {
            warning = wait_for_commit(transport, &tab_actor, &before, timeout);
        }
    }
    Ok(Installed {
        addons: installed,
        warning,
    })
}

/// What [`install_and_navigate`] reports back to `launch`.
#[derive(Debug, Default)]
pub(crate) struct Installed {
    /// One `{path, id, actor}` object per installed add-on.
    pub(crate) addons: Vec<Value>,
    /// Set when the startup URL had not committed within the wait.
    pub(crate) warning: Option<String>,
}

/// Poll `listTabs` until `tab` reports a URL other than `before` (and other
/// than the initial `about:blank`), or `timeout` passes.
fn wait_for_commit(
    transport: &mut ff_rdp_core::RdpTransport,
    tab: &ff_rdp_core::ActorId,
    before: &str,
    timeout: Duration,
) -> Option<String> {
    let deadline = Instant::now() + timeout;
    let mut last = before.to_owned();
    loop {
        if let Ok(tabs) = RootActor::list_tabs(transport)
            && let Some(current) = tabs.iter().find(|t| &t.actor == tab)
        {
            if current.url != before && current.url != "about:blank" {
                return None;
            }
            last.clone_from(&current.url);
        }
        if Instant::now() >= deadline {
            return Some(format!(
                "--temporary-addon: the startup URL had not committed after {}s (tab still at \
                 {last}); the next command may race the navigation",
                timeout.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A rejected install keeps Firefox's message verbatim (it names the problem,
/// e.g. `Reading manifest: Property "version" is required`); transport-level
/// failures keep their own typed error.
fn install_error(given: &str, error: ProtocolError) -> AppError {
    match error {
        ProtocolError::ActorError { message, .. } => AppError::User(format!(
            "--temporary-addon {given}: Firefox rejected the temporary install: {message}"
        )),
        other => AppError::from(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_prefixes_are_stripped() {
        assert_eq!(strip_verbatim_prefix(r"\\?\C:\ext"), r"C:\ext");
        assert_eq!(
            strip_verbatim_prefix(r"\\?\UNC\server\share\ext"),
            r"\\server\share\ext"
        );
        assert_eq!(strip_verbatim_prefix("/home/u/ext"), "/home/u/ext");
    }

    #[test]
    fn resolve_accepts_extension_dir_and_file_with_absolute_paths() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("ext");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), "{}").unwrap();
        let xpi = root.path().join("ext.xpi");
        std::fs::write(&xpi, b"PK").unwrap();
        let resolved = resolve(&[dir.clone(), xpi.clone()]).unwrap();
        assert_eq!(resolved[0].given, dir.display().to_string());
        assert_eq!(resolved[1].given, xpi.display().to_string());
        for addon in &resolved {
            assert!(Path::new(&addon.absolute).is_absolute(), "{addon:?}");
            assert!(!addon.absolute.starts_with(r"\\?\"), "{addon:?}");
        }
    }

    #[test]
    fn resolve_rejects_missing_path_and_dir_without_manifest() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("nope");
        let err = resolve(&[missing]).unwrap_err().to_string();
        assert!(
            err.contains("--temporary-addon") && err.contains("cannot read path"),
            "{err}"
        );
        let err = resolve(&[root.path().to_path_buf()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("no manifest.json"), "{err}");
    }

    #[test]
    fn rejected_install_keeps_firefox_message() {
        let err = install_error(
            "./bad",
            ProtocolError::ActorError {
                actor: "server1.conn0.addonsActor2".to_owned(),
                kind: ff_rdp_core::ActorErrorKind::Other("Error".to_owned()),
                error: "Error".to_owned(),
                message: "Could not install add-on at '/abs/bad': Error: Extension is invalid"
                    .to_owned(),
            },
        );
        let text = err.to_string();
        assert!(
            text.starts_with("--temporary-addon ./bad: Firefox rejected"),
            "{text}"
        );
        assert!(text.contains("Extension is invalid"), "{text}");
    }
}
