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
//! `None` there without querying anything.

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
/// running, and the listener is a Firefox executable. `None` otherwise — the
/// caller then keeps `spawned` as the owner, which is what it was before.
///
/// The liveness check matters because Windows never re-parents an orphan: a
/// recorded parent PID outlives its process. A live `spawned` (whose PID
/// cannot be reused while `launch` holds its `Child` handle) is the launcher
/// still waiting on its browser, as `-wait-for-browser` promises.
#[cfg(any(windows, test))]
fn browser_behind(spawned: u32, port: u16, view: &impl ProcessView) -> Option<u32> {
    let listener = view.listener_pid(port)?;
    if listener == spawned || view.parent_pid(listener) != Some(spawned) || !view.is_alive(spawned)
    {
        return None;
    }
    view.is_firefox(listener).then_some(listener)
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

/// `LaunchHooks::real`'s hand-over probe: the browser PID behind the launcher
/// `spawned` on `port` (Windows), or `None` (everywhere else).
pub(super) fn find_browser(spawned: u32, port: u16) -> Option<u32> {
    #[cfg(windows)]
    {
        browser_behind(spawned, port, &OsProcessView)
    }
    #[cfg(not(windows))]
    {
        let _ = (spawned, port);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{ProcessView, browser_behind};

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

    #[test]
    fn unit_299_launcher_child_on_the_port_is_the_browser() {
        assert_eq!(browser_behind(LAUNCHER, PORT, &HANDED_OVER), Some(BROWSER));
    }

    /// Linux/macOS shape: the spawned process listens itself.
    #[test]
    fn unit_299_spawned_process_listening_itself_is_no_handover() {
        let view = View {
            listener: Some(LAUNCHER),
            parent: Some(1),
            ..HANDED_OVER
        };
        assert_eq!(browser_behind(LAUNCHER, PORT, &view), None);
    }

    #[test]
    fn unit_299_no_listener_is_no_handover() {
        let view = View {
            listener: None,
            ..HANDED_OVER
        };
        assert_eq!(browser_behind(LAUNCHER, PORT, &view), None);
    }

    /// A listener ff-rdp's launcher did not start — someone else's Firefox
    /// that took the port — is never adopted.
    #[test]
    fn unit_299_listener_with_another_parent_is_not_adopted() {
        for parent in [Some(4242), None] {
            let view = View {
                parent,
                ..HANDED_OVER
            };
            assert_eq!(browser_behind(LAUNCHER, PORT, &view), None, "{parent:?}");
        }
    }

    /// A launcher that already exited (the headed case without
    /// `-wait-for-browser`) leaves a stale parent PID behind; not adopted.
    #[test]
    fn unit_299_dead_launcher_is_no_handover() {
        let view = View {
            launcher_alive: false,
            ..HANDED_OVER
        };
        assert_eq!(browser_behind(LAUNCHER, PORT, &view), None);
    }

    #[test]
    fn unit_299_non_firefox_listener_is_not_adopted() {
        let view = View {
            firefox: false,
            ..HANDED_OVER
        };
        assert_eq!(browser_behind(LAUNCHER, PORT, &view), None);
    }

    /// Off Windows the real probe never names a browser: `launch` keeps the
    /// spawned PID, exactly as before iter-299.
    #[cfg(not(windows))]
    #[test]
    fn unit_299_find_browser_is_inert_off_windows() {
        assert_eq!(super::find_browser(std::process::id(), PORT), None);
    }
}
