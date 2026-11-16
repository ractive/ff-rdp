//! `launch --temporary-addon <path>`: load unsigned
//! WebExtensions into the Firefox `launch` started, over RDP — the
//! about:debugging / `web-ext run` path — so release Firefox runs them without
//! signing. Temporary installs vanish when Firefox exits.
//!
//! The order matters: [`resolve`] runs before anything else happens (a bad path
//! never starts a browser), Firefox starts without the startup URL, then
//! [`install_and_navigate`] installs every add-on and only afterwards opens the
//! URL, so a `document_start` content script already sees the first page.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ff_rdp_core::{AddonsActor, ProtocolError, RdpConnection, RootActor, TabActor};
use serde_json::Value;

use crate::commands::addon;
pub(crate) use crate::commands::addon::AddonPath as TemporaryAddon;
use crate::error::AppError;

/// Validate every `--temporary-addon` path before Firefox starts (see
/// [`addon::resolve_path`]).
pub(crate) fn resolve(paths: &[PathBuf]) -> Result<Vec<TemporaryAddon>, AppError> {
    paths
        .iter()
        .map(|path| addon::resolve_path(path, "--temporary-addon"))
        .collect()
}

/// Install `addons` in order into the Firefox listening on `127.0.0.1:port`,
/// then navigate its selected tab (or first tab) to `url`, if one was given.
///
/// Returns one `{path, id, actor}` object per add-on for `results.temporary_addons`.
/// The first rejected install stops the sequence and becomes the error, with
/// Firefox's own message.
///
/// The URL is opened in the selected tab and launch waits for it to load (see
/// [`open_url`]). Returning while the navigation is in flight hands the caller
/// a tab mid target-switch: a follow-up `eval`/`dom` then hit `noSuchActor`,
/// `tabDestroyed` or a 10 s recv timeout in 5 of 5 runs (Firefox 160). Any
/// failure to open or wait for the URL is a warning, not a failed launch: the
/// add-ons are installed, and a plain `launch <URL>` never fails on its URL
/// either.
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
    for one in addons {
        installed.push(addon::install(
            transport,
            &addons_actor,
            one,
            "--temporary-addon",
        )?);
    }
    if let Some(url) = url {
        warning = open_url(transport, port, url, timeout).err().map(|error| {
            format!(
                "--temporary-addon: the add-ons are installed, but opening the startup \
                     URL {url} did not finish: {error}; the next command may race the navigation"
            )
        });
    }
    Ok(Installed {
        addons: installed,
        warning,
    })
}

/// Open `url` in the selected (or first) tab and wait for it.
///
/// Firefox 140+ has the tab descriptor's `navigateTo(url, waitForLoad: true)`,
/// which replies once the load of `url` stops — Firefox's own definition,
/// redirects included. Older Firefox answers `unrecognizedPacketType`; then the
/// WindowGlobalTarget `navigateTo` starts the load and [`wait_for_commit`]
/// polls `listTabs` for the URL change.
fn open_url(
    transport: &mut ff_rdp_core::RdpTransport,
    port: u16,
    url: &str,
    timeout: Duration,
) -> Result<(), AppError> {
    let tabs = RootActor::list_tabs(transport)?;
    let tab = crate::tab_target::resolve_tab_with_context(&tabs, None, None, "127.0.0.1", port)?;
    let (tab_actor, before) = (tab.actor.clone(), tab.url.clone());
    match TabActor::navigate_to(transport, &tab_actor, url, true) {
        Err(ProtocolError::ActorError {
            kind: ff_rdp_core::ActorErrorKind::UnrecognizedPacketType,
            ..
        }) => {}
        other => return other.map_err(AppError::from),
    }
    let target = TabActor::get_target(transport, &tab_actor)?;
    ff_rdp_core::WindowGlobalTarget::navigate_to(transport, &target.actor, url)?;
    wait_for_commit(transport, &tab_actor, url, &before, timeout)
}

/// What [`install_and_navigate`] reports back to `launch`.
#[derive(Debug, Default)]
pub(crate) struct Installed {
    /// One `{path, id, actor}` object per installed add-on.
    pub(crate) addons: Vec<Value>,
    /// Set when the startup URL had not committed within the wait.
    pub(crate) warning: Option<String>,
}

/// Pre-140 fallback: poll `listTabs` until `tab` has committed `url`, or
/// `timeout` passes. See [`committed`].
fn wait_for_commit(
    transport: &mut ff_rdp_core::RdpTransport,
    tab: &ff_rdp_core::ActorId,
    url: &str,
    before: &str,
    timeout: Duration,
) -> Result<(), AppError> {
    let deadline = Instant::now() + timeout;
    let mut last = before.to_owned();
    loop {
        if let Ok(tabs) = RootActor::list_tabs(transport)
            && let Some(current) = tabs.iter().find(|t| &t.actor == tab)
        {
            if committed(&current.url, url, before) {
                return Ok(());
            }
            last.clone_from(&current.url);
        }
        if Instant::now() >= deadline {
            return Err(AppError::User(format!(
                "not committed after {} ms (tab still at {last})",
                timeout.as_millis()
            )));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Has the tab committed the requested navigation? It has when it shows the
/// requested URL, or — allowing for redirects — any URL other than the one it
/// showed before `navigateTo` and the initial `about:blank`.
fn committed(current: &str, requested: &str, before: &str) -> bool {
    current == requested || (current != before && current != "about:blank")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_means_requested_url_or_a_redirect_away_from_the_start() {
        let blank = "about:blank";
        assert!(committed("https://a/", "https://a/", blank));
        assert!(committed("about:blank", "about:blank", "about:home"));
        assert!(committed("https://www.a/", "https://a/", blank));
        assert!(!committed(blank, "https://a/", blank));
        assert!(!committed("about:home", "https://a/", "about:home"));
    }
}
