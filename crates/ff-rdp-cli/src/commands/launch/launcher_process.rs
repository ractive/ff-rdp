//! iter-299: Firefox's Windows *launcher process*.
//!
//! On Windows the `firefox.exe` ff-rdp spawns is a launcher that starts the
//! real browser as its child; the browser holds the debug port. `launch`
//! passes `-wait-for-browser` there so the launcher stays alive as the
//! browser's parent for the browser's whole lifetime (headless implies it; a
//! headed launcher otherwise exits 0 at once and orphans the browser). Once
//! the browser lists a tab, [`find_browser`] identifies it so `launch` can
//! record *its* PID as the owner — `stop` and `launch --replace` look the
//! port's listener up and refuse any PID no owner marker names.
//!
//! Every other platform runs Firefox directly, and [`find_browser`] answers
//! [`Handover::NotNeeded`] there without querying anything.

/// What [`find_browser`] concluded about the spawned PID.
///
/// Off Windows only `NotNeeded` is ever constructed outside tests.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Handover {
    /// The spawned PID is the browser: it listens itself (Linux, macOS, or a
    /// Windows install whose launcher process is disabled).
    NotNeeded,
    /// The spawned launcher handed the port to this browser PID.
    Browser(u32),
    /// The port's listener could not be tied to the spawned launcher; the
    /// reason, for a launch warning. The owner marker keeps the spawned PID.
    Unidentified(String),
}

/// What the hand-over decision needs to know about processes, so the decision
/// itself is unit-tested on every OS over plain PIDs.
#[cfg(any(windows, test))]
trait ProcessView {
    /// The PID listening on `port`, if the OS names one.
    fn listener_pid(&self, port: u16) -> Option<u32>;
    /// The parent PID the OS records for `pid`.
    fn parent_pid(&self, pid: u32) -> Option<u32>;
    /// Whether `pid` is still running.
    fn is_alive(&self, pid: u32) -> bool;
    /// Whether `pid` runs a Firefox executable (name from the OS).
    fn is_firefox(&self, pid: u32) -> bool;
}

/// The browser `spawned` handed the debug port to: the port's listener, when
/// it is not `spawned` itself, `spawned` is its parent, `spawned` is still
/// running, and the listener is a Firefox executable.
///
/// The liveness check matters because Windows never re-parents an orphan: a
/// recorded parent PID outlives its process. A live `spawned` (whose PID
/// cannot be reused while `launch` holds its `Child` handle) is the launcher
/// still waiting on its browser, as `-wait-for-browser` promises.
#[cfg(any(windows, test))]
fn browser_behind(spawned: u32, port: u16, view: &impl ProcessView) -> Handover {
    let Some(listener) = view.listener_pid(port) else {
        return Handover::Unidentified(format!("no listener found on port {port}"));
    };
    if listener == spawned {
        return Handover::NotNeeded;
    }
    let parent = view.parent_pid(listener);
    if parent != Some(spawned) {
        let parent = parent.map_or_else(|| "unknown".to_owned(), |p| p.to_string());
        return Handover::Unidentified(format!(
            "port {port} is held by PID {listener}, whose parent ({parent}) is not the \
             spawned PID {spawned}"
        ));
    }
    if !view.is_alive(spawned) {
        return Handover::Unidentified(format!(
            "the spawned launcher PID {spawned} has already exited"
        ));
    }
    if !view.is_firefox(listener) {
        return Handover::Unidentified(format!(
            "PID {listener} on port {port} is not a Firefox executable"
        ));
    }
    Handover::Browser(listener)
}

/// [`browser_behind`], asked again while it answers `Unidentified` and
/// `attempts` remain: the port-owner lookup shells out (`netstat`) and can
/// miss a socket table still settling right after the first tab appeared.
#[cfg(any(windows, test))]
fn browser_behind_retrying(
    spawned: u32,
    port: u16,
    view: &impl ProcessView,
    attempts: u32,
    pause: std::time::Duration,
) -> Handover {
    let mut outcome = browser_behind(spawned, port, view);
    for _ in 1..attempts {
        if !matches!(outcome, Handover::Unidentified(_)) {
            break;
        }
        std::thread::sleep(pause);
        outcome = browser_behind(spawned, port, view);
    }
    outcome
}

#[cfg(windows)]
struct OsProcessView;

#[cfg(windows)]
impl ProcessView for OsProcessView {
    fn listener_pid(&self, port: u16) -> Option<u32> {
        crate::port_owner::find_listener(port)
            .ok()
            .flatten()
            .map(|owner| owner.pid)
    }

    fn parent_pid(&self, pid: u32) -> Option<u32> {
        crate::util::process::parent_pid(pid)
    }

    fn is_alive(&self, pid: u32) -> bool {
        crate::util::process::is_process_alive(pid)
    }

    fn is_firefox(&self, pid: u32) -> bool {
        crate::util::process::process_executable_name(pid)
            .is_some_and(|name| crate::util::process::is_firefox_executable_name(&name))
    }
}

/// `LaunchHooks::real`'s hand-over probe for the launcher `spawned` on `port`:
/// up to ~2 s of attempts on Windows, [`Handover::NotNeeded`] everywhere else.
pub(super) fn find_browser(spawned: u32, port: u16) -> Handover {
    #[cfg(windows)]
    {
        browser_behind_retrying(
            spawned,
            port,
            &OsProcessView,
            10,
            std::time::Duration::from_millis(200),
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (spawned, port);
        Handover::NotNeeded
    }
}

/// iter-304: the browser `launch` adopted from the launcher ([`Handover::Browser`]),
/// with its start token read at adoption, so a failed launch can stop it
/// itself and never signals a recycled PID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AdoptedBrowser {
    pub(super) pid: u32,
    pub(super) start_token: Option<String>,
}

impl AdoptedBrowser {
    pub(super) fn new(pid: u32) -> Self {
        Self {
            pid,
            start_token: crate::util::process::process_start_token(pid),
        }
    }
}

/// How long a failed launch waits for the processes its launcher started to
/// exit before it gives up on removing the profile.
const CHILD_EXIT_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// The process operations [`settle_with`] needs, so its decisions are
/// unit-tested over plain PIDs.
trait ProcessControl {
    fn is_alive(&self, pid: u32) -> bool;
    fn start_token(&self, pid: u32) -> Option<String>;
    /// The PIDs whose recorded parent is `pid`.
    fn children_of(&self, pid: u32) -> Vec<u32>;
    /// Terminate `pid` and the process tree it roots.
    fn terminate_tree(&self, pid: u32);
    /// Whether `pid` exited within `timeout`.
    fn wait_for_exit(&self, pid: u32, timeout: std::time::Duration) -> bool;
}

struct OsProcessControl;

impl ProcessControl for OsProcessControl {
    fn is_alive(&self, pid: u32) -> bool {
        crate::util::process::is_process_alive(pid)
    }
    fn start_token(&self, pid: u32) -> Option<String> {
        crate::util::process::process_start_token(pid)
    }
    fn children_of(&self, pid: u32) -> Vec<u32> {
        #[cfg(windows)]
        {
            crate::util::process::child_pids(pid)
        }
        #[cfg(not(windows))]
        {
            // Off Windows the spawned PID is the browser itself and leads its
            // own process group, which the failed-launch path already killed.
            let _ = pid;
            Vec::new()
        }
    }
    fn terminate_tree(&self, pid: u32) {
        crate::util::process::kill_process_tree(pid, None);
        crate::util::process::kill_process_group_force(pid);
    }
    fn wait_for_exit(&self, pid: u32, timeout: std::time::Duration) -> bool {
        super::replace::wait_for_exit(crate::util::process::is_process_alive, pid, timeout)
    }
}

/// Terminate the adopted browser — only while its start token still matches
/// the one read at adoption. `Ok` when it is gone, was replaced by another
/// process (a reused PID: the browser is gone) or was terminated.
fn terminate_adopted(
    browser: &AdoptedBrowser,
    control: &impl ProcessControl,
) -> Result<bool, String> {
    let pid = browser.pid;
    if !control.is_alive(pid) {
        return Ok(false);
    }
    match (browser.start_token.as_deref(), control.start_token(pid)) {
        (Some(adopted), Some(now)) if now != adopted => Ok(false),
        (Some(_), Some(_)) => {
            control.terminate_tree(pid);
            Ok(true)
        }
        (_, None) if !control.is_alive(pid) => Ok(false),
        _ => Err(format!(
            "the browser behind the launcher (pid {pid}) is still running and its identity \
             could not be confirmed, so ff-rdp did not stop it"
        )),
    }
}

/// After a failed launch reaped the `launcher` it spawned (still holding its
/// handle, so the PID cannot have been reused): stop the browser adopted from
/// it, if any, then wait until every process the launcher started has exited,
/// so the profile they hold can be removed.
///
/// iter-304: on GitHub's Windows runner a failed launch removed the profile
/// while the browser behind the launcher was still exiting — the removal
/// failed (`remove-failed`) and the debug port was briefly still open
/// (`live_297_bad_manifest_fails_launch_and_stops_firefox`, runs 44577054335
/// and 44588362892; `live_175_failed_launch_leaves_no_profile_dir`, run
/// 44570808653). Children other than the adopted browser are only waited
/// for, never signalled: the tree kill already reached them, and a listed
/// child might be an orphan of an earlier holder of the PID number.
///
/// `Err` carries the reason for a launch warning; the profile must then stay.
fn settle_with(
    launcher: u32,
    adopted: Option<&AdoptedBrowser>,
    control: &impl ProcessControl,
) -> Result<(), String> {
    let terminated = match adopted {
        Some(browser) => terminate_adopted(browser, control)?.then_some(browser.pid),
        None => None,
    };
    let mut waiting = control.children_of(launcher);
    // A child that started before the launcher is an orphan of an earlier
    // holder of the launcher's PID number, not ours. Start tokens are
    // comparable creation times on Windows; an unreadable one is kept (waited
    // for), the conservative direction.
    let born = |pid: u32| control.start_token(pid)?.parse::<u64>().ok();
    if let Some(launcher_born) = born(launcher) {
        waiting.retain(|&pid| born(pid).is_none_or(|child| child >= launcher_born));
    }
    if let Some(browser) = adopted {
        // The adopted browser is waited for only when it was terminated here:
        // a PID that now runs another process is not ours.
        waiting.retain(|&pid| pid != browser.pid);
    }
    waiting.extend(terminated);
    let deadline = std::time::Instant::now() + CHILD_EXIT_WAIT;
    let survivors: Vec<u32> = waiting
        .into_iter()
        .filter(|&pid| {
            !control.wait_for_exit(
                pid,
                deadline.saturating_duration_since(std::time::Instant::now()),
            )
        })
        .collect();
    if survivors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Firefox process(es) {survivors:?} started by the launcher (pid {launcher}) were still \
             running {} s after the launch failed",
            CHILD_EXIT_WAIT.as_secs()
        ))
    }
}

/// `LaunchHooks::real`'s [`settle_with`] over the OS.
pub(super) fn settle_after_failed_launch(
    launcher: u32,
    adopted: Option<&AdoptedBrowser>,
) -> Result<(), String> {
    settle_with(launcher, adopted, &OsProcessControl)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::Duration;

    use super::{
        AdoptedBrowser, Handover, ProcessControl, ProcessView, browser_behind,
        browser_behind_retrying, settle_with,
    };

    /// The launcher's children for [`settle_with`]: the browser (alive until
    /// terminated, unless `ignores_terminate`, with a fixed start token) and
    /// `others`, each alive until waited for unless listed in `stuck`.
    struct Tree {
        browser_alive: Cell<bool>,
        token: Option<&'static str>,
        ignores_terminate: bool,
        terminated: Cell<u32>,
        others: Vec<u32>,
        stuck: Vec<u32>,
        /// Per-PID start tokens that override the browser's.
        tokens: Vec<(u32, &'static str)>,
        waited: std::cell::RefCell<Vec<u32>>,
    }

    impl Tree {
        fn running(token: Option<&'static str>) -> Self {
            Self {
                browser_alive: Cell::new(true),
                token,
                ignores_terminate: false,
                terminated: Cell::new(0),
                others: Vec::new(),
                stuck: Vec::new(),
                tokens: Vec::new(),
                waited: std::cell::RefCell::new(Vec::new()),
            }
        }
    }

    impl ProcessControl for Tree {
        fn is_alive(&self, pid: u32) -> bool {
            assert_eq!(pid, BROWSER, "only the browser's liveness is asked");
            self.browser_alive.get()
        }
        fn start_token(&self, pid: u32) -> Option<String> {
            if let Some((_, token)) = self.tokens.iter().find(|(p, _)| *p == pid) {
                return Some((*token).to_owned());
            }
            self.browser_alive.get().then_some(self.token?.to_owned())
        }
        fn children_of(&self, pid: u32) -> Vec<u32> {
            assert_eq!(pid, LAUNCHER);
            let mut children = self.others.clone();
            if self.browser_alive.get() {
                children.push(BROWSER);
            }
            children
        }
        fn terminate_tree(&self, pid: u32) {
            assert_eq!(pid, BROWSER, "only the adopted browser is signalled");
            self.terminated.set(self.terminated.get() + 1);
            if !self.ignores_terminate {
                self.browser_alive.set(false);
            }
        }
        fn wait_for_exit(&self, pid: u32, _timeout: Duration) -> bool {
            self.waited.borrow_mut().push(pid);
            if pid == BROWSER {
                !self.browser_alive.get()
            } else {
                !self.stuck.contains(&pid)
            }
        }
    }

    fn adopted(token: Option<&str>) -> AdoptedBrowser {
        AdoptedBrowser {
            pid: BROWSER,
            start_token: token.map(str::to_owned),
        }
    }

    /// iter-304: the adopted browser is terminated and waited for, and so is
    /// every other process the launcher started.
    #[test]
    fn unit_304_failed_launch_stops_the_adopted_browser() {
        let tree = Tree {
            others: vec![4001],
            ..Tree::running(Some("t1"))
        };
        assert_eq!(
            settle_with(LAUNCHER, Some(&adopted(Some("t1"))), &tree),
            Ok(())
        );
        assert_eq!(tree.terminated.get(), 1);
        assert_eq!(*tree.waited.borrow(), [4001, BROWSER]);
    }

    /// Before the hand-over there is no adopted browser: nothing is
    /// signalled, the launcher's children are only waited for
    /// (`live_175_failed_launch_leaves_no_profile_dir`).
    #[test]
    fn unit_304_unadopted_children_are_waited_for_not_signalled() {
        let tree = Tree {
            browser_alive: Cell::new(false),
            others: vec![4001],
            ..Tree::running(Some("t1"))
        };
        assert_eq!(settle_with(LAUNCHER, None, &tree), Ok(()));
        assert_eq!(tree.terminated.get(), 0);
        assert!(tree.waited.borrow().contains(&4001), "{:?}", tree.waited);

        let stuck = Tree {
            browser_alive: Cell::new(false),
            others: vec![4001],
            stuck: vec![4001],
            ..Tree::running(Some("t1"))
        };
        let outcome = settle_with(LAUNCHER, None, &stuck);
        assert!(
            outcome
                .as_ref()
                .is_err_and(|reason| reason.contains("[4001]")
                    && reason.contains(&format!("pid {LAUNCHER}"))),
            "{outcome:?}"
        );
    }

    /// A browser that already exited needs nothing.
    #[test]
    fn unit_304_exited_browser_is_not_signalled() {
        let tree = Tree::running(Some("t1"));
        tree.browser_alive.set(false);
        assert_eq!(
            settle_with(LAUNCHER, Some(&adopted(Some("t1"))), &tree),
            Ok(())
        );
        assert_eq!(tree.terminated.get(), 0);
    }

    /// A PID now running another process (different start token) is never
    /// signalled: the adopted browser is gone.
    #[test]
    fn unit_304_reused_pid_is_not_signalled() {
        let tree = Tree::running(Some("someone-else"));
        assert_eq!(
            settle_with(LAUNCHER, Some(&adopted(Some("t1"))), &tree),
            Ok(()),
            "a reused PID is neither signalled nor waited for"
        );
        assert_eq!(tree.terminated.get(), 0);
        assert!(tree.waited.borrow().is_empty(), "{:?}", tree.waited);
    }

    /// A process whose recorded parent is the launcher's PID number but which
    /// started before the launcher is an orphan of an earlier holder of that
    /// number: not waited for.
    #[test]
    fn unit_304_orphan_older_than_the_launcher_is_not_waited_for() {
        let tree = Tree {
            browser_alive: Cell::new(false),
            others: vec![4001, 4002],
            stuck: vec![4001],
            tokens: vec![(LAUNCHER, "200"), (4001, "100"), (4002, "300")],
            ..Tree::running(Some("t1"))
        };
        assert_eq!(settle_with(LAUNCHER, None, &tree), Ok(()));
        assert_eq!(*tree.waited.borrow(), [4002]);
    }

    /// No identity proof, no signal: the launch reports it and keeps the
    /// profile.
    #[test]
    fn unit_304_unconfirmed_identity_is_not_signalled() {
        for (adopted_token, current) in [(None, Some("t1")), (Some("t1"), None)] {
            let tree = Tree::running(current);
            let outcome = settle_with(LAUNCHER, Some(&adopted(adopted_token)), &tree);
            assert!(
                outcome
                    .as_ref()
                    .is_err_and(|reason| reason.contains("could not be confirmed")),
                "{outcome:?}"
            );
            assert_eq!(tree.terminated.get(), 0);
        }
    }

    /// A browser that survives termination is reported, naming its PID.
    #[test]
    fn unit_304_browser_surviving_termination_is_reported() {
        let tree = Tree {
            ignores_terminate: true,
            ..Tree::running(Some("t1"))
        };
        let outcome = settle_with(LAUNCHER, Some(&adopted(Some("t1"))), &tree);
        assert!(
            outcome
                .as_ref()
                .is_err_and(|reason| reason.contains(&format!("[{BROWSER}]"))
                    && reason.contains("still running")),
            "{outcome:?}"
        );
    }

    const PORT: u16 = 6123;
    const LAUNCHER: u32 = 7628;
    const BROWSER: u32 = 2812;

    /// The process table of the diagnosed case: launcher 7628 started browser
    /// 2812, which holds port 6123 (run 44154936841).
    #[derive(Clone, Copy)]
    struct View {
        listener: Option<u32>,
        parent: Option<u32>,
        launcher_alive: bool,
        firefox: bool,
    }

    const HANDED_OVER: View = View {
        listener: Some(BROWSER),
        parent: Some(LAUNCHER),
        launcher_alive: true,
        firefox: true,
    };

    impl ProcessView for View {
        fn listener_pid(&self, port: u16) -> Option<u32> {
            assert_eq!(port, PORT);
            self.listener
        }
        fn parent_pid(&self, pid: u32) -> Option<u32> {
            assert_eq!(
                Some(pid),
                self.listener,
                "only the listener's parent is asked"
            );
            self.parent
        }
        fn is_alive(&self, pid: u32) -> bool {
            assert_eq!(pid, LAUNCHER, "only the spawned PID's liveness is asked");
            self.launcher_alive
        }
        fn is_firefox(&self, pid: u32) -> bool {
            assert_eq!(Some(pid), self.listener);
            self.firefox
        }
    }

    fn unidentified(outcome: &Handover, needle: &str) -> bool {
        matches!(outcome, Handover::Unidentified(reason) if reason.contains(needle))
    }

    #[test]
    fn unit_299_launcher_child_on_the_port_is_the_browser() {
        assert_eq!(
            browser_behind(LAUNCHER, PORT, &HANDED_OVER),
            Handover::Browser(BROWSER)
        );
    }

    /// Linux/macOS shape (or a disabled launcher): the spawned process listens.
    #[test]
    fn unit_299_spawned_process_listening_itself_needs_no_handover() {
        let view = View {
            listener: Some(LAUNCHER),
            parent: Some(1),
            ..HANDED_OVER
        };
        assert_eq!(browser_behind(LAUNCHER, PORT, &view), Handover::NotNeeded);
    }

    #[test]
    fn unit_299_no_listener_is_unidentified() {
        let view = View {
            listener: None,
            ..HANDED_OVER
        };
        let outcome = browser_behind(LAUNCHER, PORT, &view);
        assert!(unidentified(&outcome, "no listener"), "{outcome:?}");
    }

    /// A listener ff-rdp's launcher did not start — someone else's Firefox
    /// that took the port — is never adopted.
    #[test]
    fn unit_299_listener_with_another_parent_is_not_adopted() {
        for (parent, shown) in [(Some(4242), "(4242)"), (None, "(unknown)")] {
            let view = View {
                parent,
                ..HANDED_OVER
            };
            let outcome = browser_behind(LAUNCHER, PORT, &view);
            assert!(unidentified(&outcome, shown), "{outcome:?}");
        }
    }

    /// A launcher that already exited (the headed case without
    /// `-wait-for-browser`) leaves a stale parent PID behind; not adopted.
    #[test]
    fn unit_299_dead_launcher_is_not_adopted() {
        let view = View {
            launcher_alive: false,
            ..HANDED_OVER
        };
        let outcome = browser_behind(LAUNCHER, PORT, &view);
        assert!(unidentified(&outcome, "already exited"), "{outcome:?}");
    }

    #[test]
    fn unit_299_non_firefox_listener_is_not_adopted() {
        let view = View {
            firefox: false,
            ..HANDED_OVER
        };
        let outcome = browser_behind(LAUNCHER, PORT, &view);
        assert!(unidentified(&outcome, "not a Firefox"), "{outcome:?}");
    }

    /// A port-owner lookup that misses at first is asked again; a definite
    /// answer stops the retries.
    #[test]
    fn unit_299_unidentified_is_retried_until_the_listener_shows() {
        struct Settling {
            calls: Cell<u32>,
        }
        impl ProcessView for Settling {
            fn listener_pid(&self, _port: u16) -> Option<u32> {
                self.calls.set(self.calls.get() + 1);
                (self.calls.get() >= 3).then_some(BROWSER)
            }
            fn parent_pid(&self, _pid: u32) -> Option<u32> {
                Some(LAUNCHER)
            }
            fn is_alive(&self, _pid: u32) -> bool {
                true
            }
            fn is_firefox(&self, _pid: u32) -> bool {
                true
            }
        }
        let view = Settling {
            calls: Cell::new(0),
        };
        let outcome = browser_behind_retrying(LAUNCHER, PORT, &view, 10, Duration::ZERO);
        assert_eq!(outcome, Handover::Browser(BROWSER));
        assert_eq!(view.calls.get(), 3, "no attempt after a definite answer");

        let never = View {
            listener: None,
            ..HANDED_OVER
        };
        let outcome = browser_behind_retrying(LAUNCHER, PORT, &never, 3, Duration::ZERO);
        assert!(unidentified(&outcome, "no listener"), "{outcome:?}");
    }

    /// Off Windows the real probe never names a browser: `launch` keeps the
    /// spawned PID, exactly as before iter-299.
    #[cfg(not(windows))]
    #[test]
    fn unit_299_find_browser_is_inert_off_windows() {
        assert_eq!(
            super::find_browser(std::process::id(), PORT),
            Handover::NotNeeded
        );
    }
}
