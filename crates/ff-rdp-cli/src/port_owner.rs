//! Identify the process listening on a local TCP port.
//!
//! Used by `launch` (to detect port collisions before spawning Firefox) and
//! by `doctor` (to report the connected Firefox PID). Implementation shells
//! out to platform-native commands (`lsof` on Unix, `netstat`/`tasklist` on
//! Windows) — no extra crate dependencies are required.

use std::time::Duration;

/// A TCP port's listener, as observed at a single point in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortOwner {
    pub(crate) pid: u32,
    pub(crate) process_name: String,
    /// Uptime of the listener process, when known. `None` when the OS query
    /// did not return start-time data.
    pub(crate) uptime_s: Option<u64>,
}

/// Look up the process listening on `port` on the local machine.
///
/// Returns `Ok(Some(_))` when a listener is found, `Ok(None)` when nothing is
/// listening, or `Err(_)` when the OS query was inconclusive (e.g. the helper
/// command is missing). Callers that just need a soft probe should treat
/// errors as "unknown".
pub fn find_listener(port: u16) -> Result<Option<PortOwner>, String> {
    #[cfg(target_os = "windows")]
    {
        find_listener_windows(port)
    }
    #[cfg(not(target_os = "windows"))]
    {
        find_listener_unix(port)
    }
}

/// Quick check: does *anything* accept TCP connections on `port` right now?
///
/// Cheaper than `find_listener` because it skips the OS lookup. Use this for a
/// fast pre-spawn probe, then call `find_listener` to identify the owner if
/// the port is occupied.
pub fn is_port_in_use(port: u16) -> bool {
    let addr = format!("127.0.0.1:{port}");
    std::net::TcpStream::connect_timeout(
        &match addr.parse() {
            Ok(a) => a,
            Err(_) => return false,
        },
        Duration::from_millis(200),
    )
    .is_ok()
}

#[cfg(not(target_os = "windows"))]
fn find_listener_unix(port: u16) -> Result<Option<PortOwner>, String> {
    let output = std::process::Command::new("lsof")
        .arg("-nP")
        .arg(format!("-iTCP:{port}"))
        .arg("-sTCP:LISTEN")
        .arg("-Fpcn")
        .output()
        .map_err(|e| format!("running lsof: {e}"))?;

    if !output.status.success() {
        // `lsof` exits 1 when no rows match — that means no listener.
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_lsof_output(&stdout, port))
}

#[cfg(not(target_os = "windows"))]
fn parse_lsof_output(output: &str, _port: u16) -> Option<PortOwner> {
    // -Fpcn emits one field per line, each prefixed by a single character:
    //   p<pid>
    //   c<command>
    //   n<name>
    // Multiple records may be emitted; the first listener wins.
    let mut pid: Option<u32> = None;
    let mut name = String::new();
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix('p') {
            if pid.is_some() {
                break;
            }
            pid = rest.trim().parse().ok();
        } else if let Some(rest) = line.strip_prefix('c')
            && pid.is_some()
            && name.is_empty()
        {
            rest.trim().clone_into(&mut name);
        }
    }
    let pid = pid?;
    let uptime_s = process_uptime_s(pid);
    Some(PortOwner {
        pid,
        process_name: name,
        uptime_s,
    })
}

#[cfg(not(target_os = "windows"))]
fn process_uptime_s(pid: u32) -> Option<u64> {
    // `ps -o etimes=` prints elapsed seconds since the process started,
    // padded with leading whitespace. Available on macOS and Linux.
    let output = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "etimes="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.trim().parse().ok()
}

#[cfg(target_os = "windows")]
fn find_listener_windows(port: u16) -> Result<Option<PortOwner>, String> {
    // `netstat -ano` prints IPv4 and IPv6 TCP rows (and UDP rows, skipped):
    //   TCP    127.0.0.1:6000   0.0.0.0:0   LISTENING   1234
    // The state column is localized and the text is in the OEM code page, so
    // the parser never reads it — see `parse_netstat_pid`.
    let output = std::process::Command::new("netstat")
        .arg("-ano")
        .output()
        .map_err(|e| format!("running netstat: {e}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(pid) = parse_netstat_pid(&stdout, port) else {
        return Ok(None);
    };

    // `tasklist /FI "PID eq <pid>" /NH /FO CSV` prints the process name in
    // CSV. Filter keywords are not localized; its messages are.
    let process_name = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| parse_tasklist_name(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default();

    Ok(Some(PortOwner {
        pid,
        process_name,
        uptime_s: None,
    }))
}

/// PID of the TCP listener on local `port` in `netstat -ano` output.
///
/// A row is a listener when its foreign address has port 0 (`0.0.0.0:0`,
/// `[::]:0`): connected, `TIME_WAIT` and `SYN_SENT` rows always name a real
/// remote port. The state column is never read — Windows localizes it
/// (`LISTENING`, German `ABHÖREN`, garbled to `ABH�REN` by the OEM code
/// page) — and the PID is the last column, so a state that is several words
/// in some language still parses.
#[cfg(any(target_os = "windows", test))]
fn parse_netstat_pid(output: &str, port: u16) -> Option<u32> {
    output.lines().find_map(|line| {
        let cols: Vec<&str> = line.split_whitespace().collect();
        let [proto, local, foreign, _state @ .., pid] = cols.as_slice() else {
            return None;
        };
        let is_listener = cols.len() >= 5
            && proto.eq_ignore_ascii_case("TCP")
            && netstat_addr_port(local) == Some(port)
            && netstat_addr_port(foreign) == Some(0);
        if is_listener { pid.parse().ok() } else { None }
    })
}

/// Port of a netstat address column: `127.0.0.1:6000`, `[::1]:6000`.
#[cfg(any(target_os = "windows", test))]
fn netstat_addr_port(addr: &str) -> Option<u16> {
    addr.rsplit_once(':')?.1.parse().ok()
}

/// Image name from `tasklist /NH /FO CSV` output (`"firefox.exe","4242",…`).
///
/// When the process is gone tasklist prints a localized message instead
/// (`INFO: No tasks…`, German `INFORMATION: Es werden keine Aufgaben…`), so
/// only a quoted CSV row counts — no message prefix is matched.
#[cfg(any(target_os = "windows", test))]
fn parse_tasklist_name(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let first = line.trim_start().split(',').next()?.trim();
        let name = first.strip_prefix('"')?.strip_suffix('"')?;
        (!name.is_empty()).then(|| name.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_port_in_use_returns_true_for_active_listener() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(is_port_in_use(port));
    }

    #[test]
    fn is_port_in_use_returns_false_for_closed_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        // The port is now free; nothing should accept.
        assert!(!is_port_in_use(port));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn parse_lsof_output_extracts_pid_and_command() {
        let raw = "p4242\ncfirefox\nnTCP *:6000 (LISTEN)\n";
        let owner = parse_lsof_output(raw, 6000).unwrap();
        assert_eq!(owner.pid, 4242);
        assert_eq!(owner.process_name, "firefox");
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn parse_lsof_output_returns_none_when_no_pid() {
        let raw = "";
        let owner = parse_lsof_output(raw, 6000);
        assert!(owner.is_none());
    }

    #[test]
    fn parse_netstat_pid_extracts_pid_for_english_listening_row() {
        let raw = "\r\nActive Connections\r\n\
                   \r\n\
                   \x20 Proto  Local Address          Foreign Address        State           PID\r\n\
                   \x20 TCP    127.0.0.1:6000         0.0.0.0:0              LISTENING       4242\r\n";
        assert_eq!(parse_netstat_pid(raw, 6000), Some(4242));
    }

    /// Rows captured on German Windows 11 (Firefox 160 on 6399) as raw OEM
    /// bytes: `Ö` is 0x99 in code page 850, which `from_utf8_lossy` turns
    /// into U+FFFD — exactly what `find_listener_windows` parses.
    #[test]
    fn parse_netstat_pid_handles_german_oem_rows() {
        let mut raw = b"\r\nAktive Verbindungen\r\n\r\n  Proto  Lokale Adresse         Remoteadresse          Status           PID\r\n".to_vec();
        raw.extend_from_slice(
            b"  TCP    127.0.0.1:8706         127.0.0.1:6399         WARTEND         0\r\n",
        );
        raw.extend_from_slice(
            b"  TCP    127.0.0.1:6399         0.0.0.0:0              ABH\x99REN         30944\r\n",
        );
        let text = String::from_utf8_lossy(&raw);
        assert!(text.contains("ABH\u{FFFD}REN"), "lossy decode: {text}");
        assert_eq!(parse_netstat_pid(&text, 6399), Some(30944));
        // The WARTEND (TIME_WAIT) row's local port is not a listener.
        assert_eq!(parse_netstat_pid(&text, 8706), None);
    }

    #[test]
    fn parse_netstat_pid_ignores_time_wait_row_whose_remote_is_the_port() {
        let raw = "  TCP    127.0.0.1:8706  127.0.0.1:6399  TIME_WAIT  0\n";
        assert_eq!(parse_netstat_pid(raw, 6399), None);
        let raw = "  TCP    127.0.0.1:6399  127.0.0.1:8706  ESTABLISHED  77\n";
        assert_eq!(parse_netstat_pid(raw, 6399), None);
    }

    #[test]
    fn parse_netstat_pid_extracts_ipv6_listener() {
        let raw = "  TCP    [::]:6399              [::]:0                 LISTENING       5150\n\
                   \x20 TCP    [::1]:7000             [::]:0                 ABH\u{FFFD}REN         5151\n";
        assert_eq!(parse_netstat_pid(raw, 6399), Some(5150));
        assert_eq!(parse_netstat_pid(raw, 7000), Some(5151));
    }

    #[test]
    fn parse_netstat_pid_does_not_match_a_port_sharing_a_prefix() {
        let raw = "  TCP    127.0.0.1:63990  0.0.0.0:0  LISTENING  1111\n\
                   \x20 TCP    127.0.0.1:16399  0.0.0.0:0  LISTENING  2222\n";
        assert_eq!(parse_netstat_pid(raw, 6399), None);
        let raw = format!("{raw}  TCP    127.0.0.1:6399  0.0.0.0:0  LISTENING  3333\n");
        assert_eq!(parse_netstat_pid(&raw, 6399), Some(3333));
    }

    #[test]
    fn parse_netstat_pid_skips_udp_and_header_rows() {
        let raw = "  Proto  Local Address  Foreign Address  State  PID\n\
                   \x20 UDP    0.0.0.0:6399   *:*   999\n\
                   \x20 UDP    [::]:6399      *:*   998\n";
        assert_eq!(parse_netstat_pid(raw, 6399), None);
    }

    #[test]
    fn parse_tasklist_name_reads_csv_row() {
        let raw = "\r\n\"firefox.exe\",\"30944\",\"Console\",\"1\",\"412.120 K\"\r\n";
        assert_eq!(parse_tasklist_name(raw).as_deref(), Some("firefox.exe"));
    }

    #[test]
    fn parse_tasklist_name_ignores_localized_no_task_messages() {
        let english = "INFO: No tasks are running which match the specified criteria.\r\n";
        let german = "INFORMATION: Es werden keine Aufgaben mit den angegebenen Kriterien ausgef\u{FFFD}hrt.\r\n";
        assert_eq!(parse_tasklist_name(english), None);
        assert_eq!(parse_tasklist_name(german), None);
        assert_eq!(parse_tasklist_name(""), None);
    }
}
