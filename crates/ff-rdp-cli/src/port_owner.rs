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
    pub pid: u32,
    pub process_name: String,
    /// Uptime of the listener process, when known. `None` when the OS query
    /// did not return start-time data.
    pub uptime_s: Option<u64>,
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
    // `netstat -ano -p tcp` prints rows like:
    //   TCP    127.0.0.1:6000   0.0.0.0:0   LISTENING   1234
    let output = std::process::Command::new("netstat")
        .args(["-ano", "-p", "tcp"])
        .output()
        .map_err(|e| format!("running netstat: {e}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let pid = match parse_netstat_pid(&stdout, port) {
        Some(p) => p,
        None => return Ok(None),
    };

    // `tasklist /FI "PID eq <pid>" /NH /FO CSV` prints the process name in
    // CSV, which we parse out lossily.
    let process_name = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .ok()
        .and_then(|o| {
            if !o.status.success() {
                return None;
            }
            let s = String::from_utf8_lossy(&o.stdout).into_owned();
            // tasklist emits "INFO: No tasks are running..." (and similar) when
            // the process exited between netstat and tasklist; ignore non-CSV
            // lines so process_name falls back to empty.
            let first = s
                .lines()
                .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with("INFO:"))?
                .split(',')
                .next()?
                .trim();
            if !first.starts_with('"') {
                return None;
            }
            let stripped = first.trim_matches('"');
            if stripped.is_empty() {
                None
            } else {
                Some(stripped.to_owned())
            }
        })
        .unwrap_or_default();

    Ok(Some(PortOwner {
        pid,
        process_name,
        uptime_s: None,
    }))
}

#[cfg(target_os = "windows")]
fn parse_netstat_pid(output: &str, port: u16) -> Option<u32> {
    let needle = format!(":{port}");
    for line in output.lines() {
        let line = line.trim();
        if !line.starts_with("TCP") {
            continue;
        }
        if !line.contains("LISTENING") {
            continue;
        }
        // Local address column contains :<port> and remote column does not
        // for a LISTENING row. Be conservative: the local address is the 2nd
        // whitespace-delimited token.
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 5 {
            continue;
        }
        if !cols[1].ends_with(&needle) {
            continue;
        }
        if let Ok(pid) = cols[4].parse::<u32>() {
            return Some(pid);
        }
    }
    None
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

    #[cfg(target_os = "windows")]
    #[test]
    fn parse_netstat_pid_extracts_pid_for_listening_row() {
        let raw = "Active Connections\n\
                   \n\
                     Proto  Local Address          Foreign Address        State           PID\n\
                     TCP    127.0.0.1:6000         0.0.0.0:0              LISTENING       4242\n";
        assert_eq!(parse_netstat_pid(raw, 6000), Some(4242));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn parse_netstat_pid_returns_none_for_non_listening() {
        let raw = "  TCP    127.0.0.1:6000  0.0.0.0:0  ESTABLISHED  1\n";
        assert_eq!(parse_netstat_pid(raw, 6000), None);
    }
}
