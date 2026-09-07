use std::sync::{Arc, Mutex};
use std::time::Duration;

use ff_rdp_core::{
    ActorId, DeviceActor, FrontKind, ProtocolError, RdpConnection, RdpTransport, Registry,
    ResourceCommand, RootActor, Session, TabActor, TabInfo, TargetInfo,
};
use serde_json::json;

use crate::cli::args::Cli;
use crate::daemon::client::{ConnectionTarget, resolve_connection_target};
use crate::error::AppError;

/// Shared state after connecting to Firefox and resolving a tab target.
///
/// Owns a [`Session`] (transport + actor registry) and the resolved tab
/// metadata.  The registry is pre-populated with the target and console fronts
/// discovered during the `getTarget` handshake.
pub struct ConnectedTab {
    session: Session,
    /// Firefox major version, if detectable from the greeting or device actor.
    ///
    /// Currently the version is also stored in the process-global
    /// `connection_meta::remembered_version()`.  This field is kept for
    /// potential future use (e.g. exposing it via `ConnectedTab::firefox_version()`
    /// without a global read).
    #[allow(dead_code)]
    pub(crate) firefox_version: Option<u32>,
    pub(crate) target: TargetInfo,
    tab_actor: ActorId,
    /// Whether this connection goes through the daemon proxy.
    pub(crate) via_daemon: bool,
}

/// Connect to Firefox (directly or via daemon), resolve the target tab, and
/// call `getTarget` on it.
///
/// When a daemon is available, the CLI connects to the daemon's proxy port on
/// localhost.  The daemon transparently forwards RDP frames, so the rest of
/// the protocol handshake is identical.
pub fn connect_and_get_target(cli: &Cli) -> Result<ConnectedTab, AppError> {
    let target = resolve_connection_target(&cli.host, cli.port, cli.daemon_timeout, cli.no_daemon);

    let (connect_host, connect_port, via_daemon, auth_token, deferred_warning) = match target {
        ConnectionTarget::Daemon { port, auth_token } => {
            ("127.0.0.1".to_owned(), port, true, Some(auth_token), None)
        }
        ConnectionTarget::Direct { deferred_warning } => {
            (cli.host.clone(), cli.port, false, None, deferred_warning)
        }
    };

    // iter-164 (defect 2): the caller asked for daemon mode, autostart did not
    // deliver one, and we are about to run over a direct connection instead.
    // Historically `deferred_warning` was printed only if the *direct* fallback
    // also failed, so on the (common, under-load) success path the fact that
    // daemon mode silently degraded was dropped on the floor. Remember it so
    // `meta` can report it under `--verbose`.
    if let Some(w) = &deferred_warning {
        crate::connection_meta::remember_daemon_fallback(w.clone());
    }

    let connection = connect_to_firefox(
        &connect_host,
        connect_port,
        cli,
        via_daemon,
        auth_token.as_deref(),
    )
    .inspect_err(|_| {
        // The direct fallback failed too — surface the original
        // daemon-side warning alongside the connection error so the user
        // sees the full picture.
        if let Some(w) = &deferred_warning {
            // stderr-ok: (b) debug/diagnostic — the real connection error
            // still propagates through the envelope via `?` below.
            eprintln!("{w}");
        }
    })
    .map_err(ConnectFailure::into_app_error)?;

    handshake_and_resolve_tab(connection, cli, via_daemon)
}

/// Like [`connect_and_get_target`] but always bypasses the daemon and
/// connects directly to Firefox.  Use this for commands (e.g. screenshot)
/// whose protocol interactions are incompatible with the daemon proxy.
pub fn connect_direct(cli: &Cli) -> Result<ConnectedTab, AppError> {
    let connection = connect_to_firefox(&cli.host, cli.port, cli, false, None)
        .map_err(ConnectFailure::into_app_error)?;

    handshake_and_resolve_tab(connection, cli, false)
}

/// Establish a TCP connection to Firefox (or daemon proxy) and produce
/// user-friendly errors on failure.
///
/// When `auth_token` is `Some`, the token is sent as the very first frame
/// (`{"auth": "<token>"}`) to authenticate with the daemon before the normal
/// RDP handshake begins.
fn connect_to_firefox(
    host: &str,
    port: u16,
    cli: &Cli,
    via_daemon: bool,
    auth_token: Option<&str>,
) -> Result<RdpConnection, ConnectFailure> {
    let timeout = Duration::from_millis(cli.timeout);

    if let Some(token) = auth_token {
        // Daemon auth path:
        // 1. Open raw TCP (no greeting read yet).
        // 2. Send auth frame.
        // 3. Then proceed with the normal connect (reads greeting).
        let mut transport =
            RdpTransport::connect_raw(host, port, timeout).map_err(|e| {
                let detail = e.to_string();
                let app = match e {
                    ProtocolError::ConnectionFailed(_) | ProtocolError::Timeout => {
                        AppError::Connection(format!(
                            "could not connect to daemon on port {port} — try --no-daemon to connect directly to Firefox.\n\
                             hint: run `ff-rdp doctor` to inspect daemon health."
                        ))
                    }
                    other => AppError::from(other),
                };
                ConnectFailure::new(app, detail)
            })?;

        // Send the auth frame before any other request.
        transport.send(&json!({"auth": token})).map_err(|e| {
            ConnectFailure::same(AppError::Internal(anyhow::anyhow!(
                "sending daemon auth frame: {e}"
            )))
        })?;

        // Read the greeting (daemon sends it after successful auth).
        // If the daemon closes the connection here it rejected our auth token.
        // If it times out, the daemon is overloaded or the socket is stale.
        let greeting = transport.recv().map_err(|e| {
            let detail = e.to_string();
            // Distinguish: a read timeout (or transient I/O error) means the
            // daemon isn't responding, not that the token was wrong
            // (E1 — honest error messages).
            let app = if e.is_transient() {
                AppError::Timeout(
                    "daemon did not respond within the timeout after auth — \
                     the daemon may be overloaded or the connection is stale.\n\
                     hint: run `ff-rdp daemon stop` then retry, or use --no-daemon."
                        .to_string(),
                )
            } else {
                AppError::User(format!(
                    "daemon auth rejected (wrong token): {e}\n\
                     hint: stop the running daemon (`ff-rdp daemon stop`) or use --no-daemon."
                ))
            };
            ConnectFailure::new(app, detail)
        })?;

        // Verify protocol version — a mismatch means the running daemon is a
        // different build than this CLI binary.  A missing or non-numeric
        // `protocol_version` field is also treated as a mismatch (reported as
        // daemon=0) — a pre-versioning daemon predates this CLI and must be
        // restarted to pick up the new handshake.
        let daemon_version = greeting
            .get("protocol_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0);
        let expected = crate::daemon::server::DAEMON_PROTOCOL_VERSION;
        if daemon_version != expected {
            return Err(ConnectFailure::same(AppError::DaemonVersionMismatch {
                daemon: daemon_version,
                cli: expected,
            }));
        }

        // Now wrap in RdpConnection. We already consumed the greeting; pass it
        // through so the Firefox version stays available for connection_meta.
        return Ok(RdpConnection::from_authenticated_transport(
            transport, &greeting,
        ));
    }

    RdpConnection::connect(host, port, timeout).map_err(|e| {
        let detail = e.to_string();
        let app = match e {
            ProtocolError::ConnectionFailed(_) | ProtocolError::Timeout if !via_daemon => {
                AppError::Connection(format!(
                    "could not connect to Firefox at {}:{} — is Firefox running with --start-debugger-server {}?\n\
                     hint: run `ff-rdp doctor` for a full diagnostic, or `ff-rdp launch` to start Firefox with debugging enabled.",
                    cli.host, cli.port, cli.port
                ))
            }
            ProtocolError::ConnectionFailed(_) | ProtocolError::Timeout if via_daemon => {
                AppError::Connection(format!(
                    "could not connect to daemon on port {port} — try --no-daemon to connect directly to Firefox.\n\
                     hint: run `ff-rdp doctor` to inspect daemon health."
                ))
            }
            other => AppError::from(other),
        };
        ConnectFailure::new(app, detail)
    })
}

/// A [`connect_to_firefox`] failure, keeping the raw transport error next to
/// the user-facing message built from it.
///
/// The two are not interchangeable. [`AppError::Connection`]'s text is written
/// for a terminal — multi-line, with a `hint:` line naming `ff-rdp doctor` —
/// which is right for a command that is about to abort, and wrong for the home
/// view, whose `browser.detail` JSON field wants the one-line transport reason
/// and emits its own hints separately. So the constructor keeps both and each
/// caller takes the half it needs.
struct ConnectFailure {
    /// Boxed because `AppError` is 136 bytes and this rides in the `Err` of a
    /// `Result` whose `Ok` is smaller (`clippy::result_large_err`).
    app: Box<AppError>,
    /// The underlying transport error, undressed.
    detail: String,
}

impl ConnectFailure {
    fn new(app: AppError, detail: String) -> Self {
        Self {
            app: Box::new(app),
            detail,
        }
    }

    /// A failure whose raw detail is simply the user-facing message — used for
    /// the daemon-handshake errors that have no distinct transport-level text.
    fn same(app: AppError) -> Self {
        let detail = app.to_string();
        Self::new(app, detail)
    }

    fn into_app_error(self) -> AppError {
        *self.app
    }
}

/// How [`connect_and_list_tabs`] should reach Firefox.
///
/// This is an explicit parameter rather than a lookup inside the primitive
/// because [`resolve_connection_target`] *starts* a daemon when it does not
/// find one, and the home view must never do that ([[decision-log]] DEC-050:
/// "the home view starts nothing"). Making the caller hand over the registry
/// entry it already read turns that rule into something the type system keeps,
/// and saves the redundant second registry read the old `page_block` paid for.
#[derive(Clone, Copy)]
pub enum TabListRouting<'a> {
    /// Straight to Firefox's debugger port. Touches no daemon, starts nothing.
    Direct,
    /// Through the proxy of a daemon that is **already running** — so the refs
    /// handed out by a later [`TabListing::attach`] are live handles in that
    /// daemon's ref store.
    RunningDaemon {
        proxy_port: u16,
        auth_token: &'a str,
    },
}

/// Why [`connect_and_list_tabs`] gave up.
///
/// The two variants are the two states the home view renders differently: a
/// browser that never answered is `reachable: false` and sends the agent to
/// `launch`, while a browser that greeted us and then failed `listTabs` is
/// `reachable: true` and sends it to `doctor`.
pub enum TabListError {
    /// The TCP connect or the RDP greeting never landed.
    Connect {
        /// Boxed for the same reason as [`ConnectFailure::app`].
        error: Box<AppError>,
        /// The raw transport reason, without the multi-line hint text — see
        /// [`ConnectFailure`].
        detail: String,
    },
    /// The greeting landed but `listTabs` did not: the browser *is* reachable.
    ListTabs {
        /// The version from the greeting, which is already known at this point.
        firefox_version: Option<u32>,
        error: Box<AppError>,
        /// The raw actor/transport reason.
        detail: String,
    },
}

impl TabListError {
    /// Collapse to the error a normal command would have reported.
    pub fn into_app_error(self) -> AppError {
        match self {
            Self::Connect { error, .. } | Self::ListTabs { error, .. } => *error,
        }
    }

    /// The one-line reason, for callers that report the failure as state
    /// rather than raising it.
    pub fn detail(&self) -> &str {
        match self {
            Self::Connect { detail, .. } | Self::ListTabs { detail, .. } => detail,
        }
    }
}

/// A live connection that has greeted Firefox and listed its tabs, but has not
/// yet attached to one of them.
///
/// This is the split that lets the home view (iter-239) get both the full tab
/// list *and* an attached target from a single connect: it reads
/// [`tabs`](Self::tabs) for its `tabs` block, then calls
/// [`attach`](Self::attach) on the same connection for the focused tab's
/// accessibility view. Before, those were two independent `connect` calls —
/// two TCP round trips and two RDP handshakes for one invocation of the
/// command the `SessionStart` hook runs on every agent session.
pub struct TabListing {
    connection: RdpConnection,
    /// The version the RDP greeting carried, before any device-actor fallback.
    greeting_version: Option<u32>,
    tabs: Vec<TabInfo>,
    via_daemon: bool,
}

impl TabListing {
    /// The full `listTabs` payload — every tab, not just the resolved one.
    pub fn tabs(&self) -> &[TabInfo] {
        &self.tabs
    }

    /// The Firefox version as the greeting reported it, or `None` when the
    /// greeting omitted `ua`.
    ///
    /// Deliberately *not* the device-actor fallback [`attach`](Self::attach)
    /// resolves: this is what the connection announced about itself, and the
    /// home view reports it verbatim rather than a value synthesised by a
    /// round trip it may never make.
    pub fn greeting_version(&self) -> Option<u32> {
        self.greeting_version
    }

    /// Resolve the target tab on **this** connection and call `getTarget` on
    /// it, yielding the same [`ConnectedTab`] a plain
    /// [`connect_and_get_target`] would have produced.
    pub fn attach(self, cli: &Cli) -> Result<ConnectedTab, AppError> {
        let Self {
            mut connection,
            greeting_version,
            tabs,
            via_daemon,
        } = self;

        // When the RDP greeting omits the `ua` field (some Firefox builds strip
        // it), try the device actor's `getDescription` as a version fallback.
        // This ensures `remembered_version()` is populated for all downstream
        // callers (e.g. `version_mismatch_message()` in the screenshot path) and
        // that the compatibility warning is emitted based on the resolved
        // version, not the (absent) greeting one.
        let effective_version = if greeting_version.is_none() {
            DeviceActor::query_version(connection.transport_mut()).unwrap_or(None)
        } else {
            greeting_version
        };
        if effective_version != greeting_version {
            connection.set_firefox_version(effective_version);
        }
        crate::connection_meta::remember_version(effective_version);

        let tab = crate::tab_target::resolve_tab_with_context(
            &tabs,
            cli.tab.as_deref(),
            cli.tab_id.as_deref(),
            &cli.host,
            cli.port,
        )?;
        let tab_actor = tab.actor.clone();

        let target_info =
            TabActor::get_target(connection.transport_mut(), &tab_actor).map_err(AppError::from)?;

        // Consume the RdpConnection and build a Session so all subsequent
        // actor interactions use the registry for front resolution.
        let firefox_version = connection.firefox_version();
        let transport = connection.into_transport();
        let session = Session::new(transport);

        // Register the target front (WindowGlobalTarget) and its console front.
        // These two are always present after getTarget.
        register_target_fronts(session.registry(), &target_info);

        Ok(ConnectedTab {
            session,
            firefox_version,
            target: target_info,
            tab_actor,
            via_daemon,
        })
    }
}

/// Connect once and list every tab, leaving the connection open so the caller
/// can [`attach`](TabListing::attach) to one of them without reconnecting.
///
/// Unlike [`connect_and_get_target`], the route is the caller's decision (see
/// [`TabListRouting`]) and no daemon is ever started. Unlike both existing
/// entry points, the error distinguishes "never reached the browser" from
/// "reached it, and `listTabs` failed" — the split the home view renders as
/// `reachable: false` vs `reachable: true` with a `detail`.
pub fn connect_and_list_tabs(
    cli: &Cli,
    routing: TabListRouting<'_>,
) -> Result<TabListing, TabListError> {
    let (host, port, via_daemon, auth_token) = match routing {
        TabListRouting::Direct => (cli.host.as_str(), cli.port, false, None),
        TabListRouting::RunningDaemon {
            proxy_port,
            auth_token,
        } => ("127.0.0.1", proxy_port, true, Some(auth_token)),
    };

    let connection = connect_to_firefox(host, port, cli, via_daemon, auth_token)
        .map_err(|ConnectFailure { app, detail }| TabListError::Connect { error: app, detail })?;

    handshake_and_list_tabs(connection, via_daemon)
}

/// Greet, remember the version, and run `listTabs`.
fn handshake_and_list_tabs(
    mut connection: RdpConnection,
    via_daemon: bool,
) -> Result<TabListing, TabListError> {
    let greeting_version = connection.firefox_version();
    // Remember the greeting version *before* `listTabs`, so a `listTabs`
    // failure still leaves downstream error paths able to name the version.
    // `attach` overwrites it with the device-actor fallback when there is one;
    // `remember_version` ignores a `None`, so the ordering is safe either way.
    crate::connection_meta::remember_version(greeting_version);

    let tabs = match RootActor::list_tabs(connection.transport_mut()) {
        Ok(tabs) => tabs,
        Err(e) => {
            let detail = e.to_string();
            return Err(TabListError::ListTabs {
                firefox_version: greeting_version,
                error: Box::new(AppError::from(e)),
                detail,
            });
        }
    };

    Ok(TabListing {
        connection,
        greeting_version,
        tabs,
        via_daemon,
    })
}

/// Run the RDP handshake: list tabs, resolve the target tab, call `getTarget`,
/// and register the discovered actor fronts in the session registry.
fn handshake_and_resolve_tab(
    connection: RdpConnection,
    cli: &Cli,
    via_daemon: bool,
) -> Result<ConnectedTab, AppError> {
    handshake_and_list_tabs(connection, via_daemon)
        .map_err(TabListError::into_app_error)?
        .attach(cli)
}

/// Register target and console fronts in the registry after `getTarget`.
///
/// Called once per `handshake_and_resolve_tab` and again after each
/// `refresh_target` to keep the registry in sync with Firefox's actor state.
pub(crate) fn register_target_fronts(registry: &Arc<Registry>, target: &TargetInfo) {
    let target_id = target.actor.clone();
    let console_id = target.console_actor.clone();

    registry.register(target_id.clone(), FrontKind::Target, None);
    registry.register(console_id, FrontKind::Console, Some(target_id));
}

impl ConnectedTab {
    /// Borrow the underlying RDP transport.
    pub fn transport_mut(&mut self) -> &mut RdpTransport {
        self.session.transport_mut()
    }

    /// Borrow the session (transport + registry).
    ///
    /// Used by theme C/D agents (iter-61t) to access both transport and
    /// registry together without separate borrows.
    #[allow(dead_code)]
    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    /// Return a reference to the actor registry for this session.
    ///
    /// Callers use this to look up pre-registered fronts or to register
    /// additional fronts (e.g. after acquiring a watcher actor).
    pub fn registry(&self) -> &Arc<Registry> {
        self.session.registry()
    }

    pub fn target_tab_actor(&self) -> &ActorId {
        &self.tab_actor
    }

    /// Return the attached [`ResourceCommand`] bus, or create-and-attach one
    /// if none exists yet.
    ///
    /// This helper centralises the "lazily construct the bus once and reuse"
    /// pattern used by navigate and other commands.  The created bus is stored
    /// on the session so subsequent calls return the same instance.
    ///
    /// # Watcher-actor stability invariant ("first watcher_actor wins")
    ///
    /// Once a `ResourceCommand` bus is created, the `watcher_actor` passed in
    /// subsequent calls is **silently ignored** — the bus always uses the actor
    /// that was bound at creation time.  This is intentional: Firefox assigns a
    /// stable watcher actor per tab for the lifetime of the connection, so all
    /// calls within a single session refer to the same actor.  If Firefox ever
    /// changes this (per-navigation watcher rotation), callers would need to
    /// detect the change and call `session.clear_resource_command()` before
    /// calling this helper again.
    pub fn get_or_init_resource_command(
        &mut self,
        watcher_actor: ActorId,
    ) -> Arc<Mutex<ResourceCommand>> {
        if let Some(existing) = self.session.resource_command() {
            return Arc::clone(existing);
        }
        let rc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));
        self.session.set_resource_command(Arc::clone(&rc));
        rc
    }

    /// Re-resolve the target actors (consoleActor, etc.) from Firefox.
    ///
    /// After a navigation has **committed**, the old docshell is gone and its
    /// `consoleActor` ID is stale; calling this refreshes `self.target` so the
    /// next `eval` uses the actor bound to the new docshell.
    ///
    /// **It does not escape a navigation that is still in flight** (iter-220).
    /// Between the click and the commit, `getTarget` re-forwards the *outgoing*
    /// docshell under a fresh `childN/` prefix: a new-looking target actor with
    /// the same `innerWindowId` and the same `url`. Evaluating against it either
    /// describes the page you are leaving or hangs, because Firefox drops the
    /// request without answering when the docshell goes. A caller that acts and
    /// then reads must wait for the navigation first — see
    /// `page_view::collect_settled`.
    ///
    /// When a refresh succeeds the registry is also updated: the old console
    /// front is invalidated (via `invalidate_target` on the old target ID) and
    /// the new target + console fronts are registered.
    ///
    /// Errors are intentionally swallowed: a failed refresh is non-fatal
    /// since the caller will get a `noSuchActor` error on the next eval
    /// (same failure mode as before) and the retry with a fresh target will
    /// succeed.
    pub fn refresh_target(&mut self) {
        let tab_actor = self.tab_actor.clone();
        match TabActor::get_target(self.session.transport_mut(), &tab_actor) {
            Ok(fresh) => {
                // Invalidate the stale target front and all its owned actors.
                let old_target_id = self.target.actor.clone();
                self.session.registry().invalidate_target(&old_target_id);
                // Register the fresh fronts.
                register_target_fronts(self.session.registry(), &fresh);
                self.target = fresh;
            }
            Err(e) => {
                // stderr-ok: (b) warn-and-continue — non-fatal per the doc
                // comment above; retried with a fresh target on next eval.
                eprintln!("warning: navigate: could not refresh target actors: {e:#}");
            }
        }
    }

    /// Guard the wait loops against the current target being torn down by a
    /// navigation (iter-220).
    ///
    /// Thin forwarder to
    /// [`RdpTransport::set_target_guard`](ff_rdp_core::RdpTransport::set_target_guard);
    /// `Some(inner_window_id)` arms it, `None` disarms it.  Callers arm it only
    /// around a section they are prepared to retry — see
    /// [`crate::commands::page_view::attach`].
    pub(crate) fn set_target_guard(&mut self, inner_window_id: Option<u64>) {
        self.session
            .transport_mut()
            .set_target_guard(inner_window_id);
    }

    /// Take (and clear) the destination URL of the most recent top-level
    /// navigation Firefox announced on this connection (iter-220).
    ///
    /// Thin forwarder to
    /// [`RdpTransport::take_navigation_started`](ff_rdp_core::RdpTransport::take_navigation_started).
    pub(crate) fn take_navigation_started(&mut self) -> Option<String> {
        self.session.transport_mut().take_navigation_started()
    }

    /// Arm [`set_target_guard`](Self::set_target_guard) for the returned
    /// scope's lifetime and disarm it automatically when the scope drops
    /// (iter-220 review finding).
    ///
    /// `set_target_guard`/`take_navigation_started`-style manual arm/clear
    /// pairs rely on every call site remembering the second half; a `?`
    /// early-return added between them later — or a panic unwinding out of
    /// the guarded section — would leave the guard armed for a later,
    /// unrelated wait on the same connection. This makes clearing it
    /// structural instead of a doc-comment promise: the scope
    /// `Deref`/`DerefMut`s to `ConnectedTab`, so a caller passes `&mut scope`
    /// anywhere a `&mut ConnectedTab` is expected, same as `ctx` itself.
    pub(crate) fn arm_target_guard(
        &mut self,
        inner_window_id: Option<u64>,
    ) -> TargetGuardScope<'_> {
        self.set_target_guard(inner_window_id);
        TargetGuardScope { ctx: self }
    }

    /// Build a `ConnectedTab` directly from a transport and console actor ID,
    /// bypassing the connect + `getTarget` handshake.
    ///
    /// Test-only: lets unit tests elsewhere in the crate (e.g.
    /// `js_helpers::poll_js_condition` tests) exercise `&mut ConnectedTab`
    /// APIs against a mock transport without spinning up a full RDP session.
    #[cfg(test)]
    pub(crate) fn for_test(transport: RdpTransport, console_actor: ActorId) -> Self {
        let target = TargetInfo {
            actor: ActorId::from("conn0/target1"),
            console_actor,
            thread_actor: None,
            inspector_actor: None,
            screenshot_content_actor: None,
            accessibility_actor: None,
            responsive_actor: None,
            manifest_actor: None,
            browsing_context_id: None,
            inner_window_id: None,
            url: None,
        };
        Self {
            session: Session::new(transport),
            firefox_version: None,
            target,
            tab_actor: ActorId::from("conn0/tab1"),
            via_daemon: false,
        }
    }
}

/// RAII scope returned by [`ConnectedTab::arm_target_guard`] (iter-220).
///
/// Disarms the target guard on drop, however the scope ends — a normal fall
/// through, an early `return`/`?`, or a panic unwinding out of it — so a
/// guarded section cannot leave the guard armed for whatever the connection
/// waits on next. See [`ConnectedTab::arm_target_guard`]'s doc comment.
pub(crate) struct TargetGuardScope<'a> {
    ctx: &'a mut ConnectedTab,
}

impl std::ops::Deref for TargetGuardScope<'_> {
    type Target = ConnectedTab;

    fn deref(&self) -> &ConnectedTab {
        self.ctx
    }
}

impl std::ops::DerefMut for TargetGuardScope<'_> {
    fn deref_mut(&mut self) -> &mut ConnectedTab {
        self.ctx
    }
}

impl Drop for TargetGuardScope<'_> {
    fn drop(&mut self) {
        self.ctx.set_target_guard(None);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use clap::Parser as _;
    use ff_rdp_core::transport::encode_frame;
    use serde_json::Value;

    use super::*;

    /// Replies recorded from a real Firefox (see `.claude/CLAUDE.md` — fixtures
    /// are never hand-crafted), replayed by the mock below.
    const LIST_TABS_REPLY: &str = include_str!("../../tests/fixtures/list_tabs_response.json");
    const GET_TARGET_REPLY: &str = include_str!("../../tests/fixtures/get_target_response.json");

    /// A listener that speaks just enough RDP for the connect + `listTabs` +
    /// `getTarget` handshake, and — the point of the whole exercise — counts
    /// how many times it was connected to.
    ///
    /// iter-239 is a round-trip change, so a test that only checks the parsed
    /// result would pass just as happily against the two-connection code it
    /// replaced. The connection counter is the assertion that actually pins it.
    struct MockFirefox {
        port: u16,
        connections: Arc<AtomicUsize>,
        shutdown: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl MockFirefox {
        /// `list_tabs_ok = false` makes the server answer `listTabs` with an
        /// actor error — the "greeting landed, listTabs did not" branch.
        fn start(list_tabs_ok: bool) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
            let port = listener.local_addr().expect("addr").port();
            listener.set_nonblocking(true).expect("nonblocking");

            let connections = Arc::new(AtomicUsize::new(0));
            let shutdown = Arc::new(AtomicBool::new(false));
            let counter = Arc::clone(&connections);
            let stop = Arc::clone(&shutdown);

            let thread = std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            counter.fetch_add(1, Ordering::SeqCst);
                            stream.set_nonblocking(false).expect("blocking stream");
                            serve(stream, list_tabs_ok);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            });

            Self {
                port,
                connections,
                shutdown,
                thread: Some(thread),
            }
        }

        fn connections(&self) -> usize {
            self.connections.load(Ordering::SeqCst)
        }

        fn cli(&self, extra: &[&str]) -> Cli {
            let port = self.port.to_string();
            let mut argv = vec![
                "ff-rdp",
                "--host",
                "127.0.0.1",
                "--port",
                &port,
                "--no-daemon",
                "--timeout",
                "5000",
            ];
            argv.extend_from_slice(extra);
            // The home view is what iter-239 is about, and `home` is also the
            // subcommand clap substitutes for a bare `ff-rdp` (see `main`), so
            // it is both the realistic and the parseable choice here.
            argv.push("home");
            Cli::try_parse_from(argv).expect("mock cli args parse")
        }
    }

    impl Drop for MockFirefox {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::SeqCst);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    /// Read one `<len>:<json>` frame, or `None` at EOF.
    fn read_frame(stream: &mut TcpStream) -> Option<Value> {
        let mut len_digits = String::new();
        loop {
            let mut byte = [0u8; 1];
            if stream.read_exact(&mut byte).is_err() {
                return None;
            }
            if byte[0] == b':' {
                break;
            }
            len_digits.push(char::from(byte[0]));
        }
        let len: usize = len_digits.parse().ok()?;
        let mut body = vec![0u8; len];
        stream.read_exact(&mut body).ok()?;
        serde_json::from_slice(&body).ok()
    }

    fn send(stream: &mut TcpStream, value: &Value) {
        let frame = encode_frame(&value.to_string());
        let _ = stream.write_all(frame.as_bytes());
    }

    fn serve(mut stream: TcpStream, list_tabs_ok: bool) {
        send(
            &mut stream,
            &json!({
                "from": "root",
                "applicationType": "browser",
                "traits": {},
                "ua": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:143.0) Gecko/20100101 Firefox/143.0",
            }),
        );

        while let Some(request) = read_frame(&mut stream) {
            let to = request
                .get("to")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let kind = request
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();

            let reply = match kind {
                "listTabs" if list_tabs_ok => {
                    serde_json::from_str(LIST_TABS_REPLY).expect("recorded listTabs")
                }
                "getTarget" => {
                    let mut reply: Value =
                        serde_json::from_str(GET_TARGET_REPLY).expect("recorded getTarget");
                    reply["from"] = json!(to);
                    reply
                }
                // Everything else (including `listTabs` in the failure mode)
                // answers with an actor error rather than closing, so nothing
                // downstream mistakes the mock for a lost connection and
                // reconnects.
                _ => json!({
                    "from": to,
                    "error": "unknownError",
                    "message": "mock firefox does not implement this",
                }),
            };
            send(&mut stream, &reply);
        }
    }

    /// Task A: the primitive lists every tab *and* attaches to one of them
    /// over a single connection — the two round trips iter-239 merged.
    #[test]
    fn unit_239_lists_every_tab_and_attaches_on_one_connection() {
        let mock = MockFirefox::start(true);
        let cli = mock.cli(&[]);

        let listing = connect_and_list_tabs(&cli, TabListRouting::Direct).unwrap_or_else(|e| {
            panic!("connect_and_list_tabs: {}", e.into_app_error());
        });

        assert_eq!(listing.greeting_version(), Some(143));
        let urls: Vec<&str> = listing.tabs().iter().map(|t| t.url.as_str()).collect();
        assert_eq!(
            urls,
            ["https://example.com/", "https://www.rust-lang.org/"],
            "the listing must carry every tab, not just the resolved one"
        );

        let ctx = listing.attach(&cli).expect("attach on the same connection");
        assert_eq!(
            ctx.target_tab_actor().as_ref(),
            "server1.conn0.tabDescriptor1",
            "no --tab flag resolves to the selected tab"
        );
        assert_eq!(
            ctx.target.console_actor.as_ref(),
            "server1.conn0.child2/consoleActor3"
        );

        drop(ctx);
        assert_eq!(
            mock.connections(),
            1,
            "the tab list and the attached target must come from one connect"
        );
    }

    /// `--tab N` is honoured by `attach`, and still costs no extra connection:
    /// the selector is applied to the list the same connection already
    /// returned.
    #[test]
    fn unit_239_attach_honours_the_tab_selector() {
        let mock = MockFirefox::start(true);
        let cli = mock.cli(&["--tab", "2"]);

        let listing = connect_and_list_tabs(&cli, TabListRouting::Direct)
            .unwrap_or_else(|e| panic!("connect_and_list_tabs: {}", e.into_app_error()));
        let ctx = listing.attach(&cli).expect("attach");

        assert_eq!(
            ctx.target_tab_actor().as_ref(),
            "server1.conn0.tabDescriptor2"
        );
        drop(ctx);
        assert_eq!(mock.connections(), 1);
    }

    /// The two failure branches the home view renders differently: a greeting
    /// that never landed is `reachable: false`, a `listTabs` that failed after
    /// one is `reachable: true` with the version already known.
    #[test]
    fn unit_239_list_tabs_failure_is_not_a_connect_failure() {
        let mock = MockFirefox::start(false);
        let cli = mock.cli(&[]);

        match connect_and_list_tabs(&cli, TabListRouting::Direct) {
            Err(TabListError::ListTabs {
                firefox_version,
                detail,
                ..
            }) => {
                assert_eq!(
                    firefox_version,
                    Some(143),
                    "the greeting landed, so its version is known"
                );
                assert!(!detail.is_empty(), "the reason must be reportable");
            }
            Err(TabListError::Connect { detail, .. }) => {
                panic!("a reachable browser must not report a connect failure: {detail}")
            }
            Ok(_) => panic!("the mock refused listTabs"),
        }
    }

    /// A dark port is a connect failure, and its `detail` is the raw transport
    /// reason — not the multi-line `AppError::Connection` text, whose `hint:`
    /// line the home view emits itself.
    #[test]
    fn unit_239_connect_failure_detail_is_the_undressed_reason() {
        let dark = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            let port = listener.local_addr().expect("addr").port();
            drop(listener);
            port
        };
        let port = dark.to_string();
        let cli = Cli::try_parse_from([
            "ff-rdp",
            "--host",
            "127.0.0.1",
            "--port",
            &port,
            "--no-daemon",
            "--timeout",
            "1500",
            "home",
        ])
        .expect("cli args parse");

        match connect_and_list_tabs(&cli, TabListRouting::Direct) {
            Err(e @ TabListError::Connect { .. }) => {
                let detail = e.detail().to_owned();
                assert!(
                    !detail.contains("hint:"),
                    "the JSON detail must stay a one-line reason: {detail}"
                );
                assert!(
                    e.into_app_error().to_string().contains("hint:"),
                    "…while the raised error keeps the terminal-facing hint"
                );
            }
            Err(other) => panic!(
                "a dark port must be a connect failure, got: {}",
                other.into_app_error()
            ),
            Ok(_) => panic!("nothing is listening on the dark port"),
        }
    }
}
