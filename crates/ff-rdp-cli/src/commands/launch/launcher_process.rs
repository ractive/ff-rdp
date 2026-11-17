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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::Duration;

    use super::{Handover, ProcessView, browser_behind, browser_behind_retrying};

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
