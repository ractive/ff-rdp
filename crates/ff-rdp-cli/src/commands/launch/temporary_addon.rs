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

use super::startup_url;
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
        warning = open_url(transport, port, url, Instant::now() + timeout)
            .err()
            .map(|error| {
                format!(
                    "--temporary-addon: the add-ons are installed, but the startup URL {url} \
                     did not load: {error}"
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
/// WindowGlobalTarget `navigateTo` only starts the load. Either way
/// [`startup_url::wait_for_document`] then checks the document through the
/// route the next command takes, as the plain `launch <URL>` does: the reply
/// to `navigateTo` says a load of the URL stopped, not that it finished — an
/// aborted load replies the same (no status check in Firefox's listener) —
/// nor that a fresh `getTarget` already hands out the new document. When the
/// tab stays on its old page, the wait opens the URL again.
fn open_url(
    transport: &mut ff_rdp_core::RdpTransport,
    port: u16,
    url: &str,
    deadline: Instant,
) -> Result<(), AppError> {
    let tabs = RootActor::list_tabs(transport)?;
    let tab = crate::tab_target::resolve_tab_with_context(&tabs, None, None, "127.0.0.1", port)?;
    let (tab_actor, before) = (tab.actor.clone(), tab.url.clone());
    navigate(transport, &tab_actor, url, deadline)?;
    let mut renavigate = || navigate(transport, &tab_actor, url, deadline);
    startup_url::wait_for_document(port, url, &before, deadline, Some(&mut renavigate))
}

/// One `navigateTo` of `tab` to `url`: the descriptor's, waiting for the load
/// (Firefox 140+), else the WindowGlobalTarget's, which only starts it.
fn navigate(
    transport: &mut ff_rdp_core::RdpTransport,
    tab: &ff_rdp_core::ActorId,
    url: &str,
    deadline: Instant,
) -> Result<(), AppError> {
    let started = Instant::now();
    let navigated =
        transport.with_read_deadline(deadline, |t| TabActor::navigate_to(t, tab, url, true));
    tracing::debug!(
        elapsed_ms = started.elapsed().as_millis(),
        ok = navigated.is_ok(),
        "launch: --temporary-addon navigateTo replied"
    );
    match navigated {
        Err(ProtocolError::ActorError {
            kind: ff_rdp_core::ActorErrorKind::UnrecognizedPacketType,
            ..
        }) => {
            let target = TabActor::get_target(transport, tab)?;
            ff_rdp_core::WindowGlobalTarget::navigate_to(transport, &target.actor, url)?;
            Ok(())
        }
        other => Ok(other?),
    }
}

/// What [`install_and_navigate`] reports back to `launch`.
#[derive(Debug, Default)]
pub(crate) struct Installed {
    /// One `{path, id, actor}` object per installed add-on.
    pub(crate) addons: Vec<Value>,
    /// Set when the startup URL had not loaded within the wait.
    pub(crate) warning: Option<String>,
}
