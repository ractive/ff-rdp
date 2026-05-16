//! `ff-rdp doctor` — top-to-bottom connection diagnostic.
//!
//! Probes each layer of the stack and reports the first one that fails so the
//! user (or AI agent) sees a single concrete next step. Exits 0 when every
//! probe passes, 1 when any probe fails — CI friendly.

use std::time::Duration;

use ff_rdp_core::{
    COMPATIBLE_FIREFOX_MAX, COMPATIBLE_FIREFOX_MIN, DeviceActor, RdpConnection, RootActor, TabInfo,
};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::connection_meta::is_loopback;
use crate::daemon::{client::find_running_daemon, registry};
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;
use crate::port_owner::{self, PortOwner};
use crate::tab_target::format_uptime_short as format_uptime;

/// Probe outcome with a one-line, copy-pasteable hint.
#[derive(Debug, Clone)]
struct Probe {
    name: &'static str,
    status: Status,
    detail: String,
    hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Pass,
    Warn,
    Fail,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
    fn glyph(self) -> &'static str {
        match self {
            Self::Pass => "✓",
            Self::Warn => "⚠",
            Self::Fail => "✗",
        }
    }
}

pub fn run(cli: &Cli) -> Result<(), AppError> {
    let host = cli.host.as_str();
    let port = cli.port;
    let mut probes: Vec<Probe> = Vec::new();

    // 1. Daemon registry
    probes.push(probe_daemon(host, port));

    // 2. Port owner — local OS query, only meaningful for loopback hosts.
    //    Looked up before the handshake attempt so we can report PID/uptime
    //    even when the listener is not Firefox.
    let local = is_loopback(host);
    let owner = if local {
        port_owner::find_listener(port).ok().flatten()
    } else {
        None
    };
    let port_in_use = if local {
        owner.is_some() || port_owner::is_port_in_use(port)
    } else {
        // Non-loopback: skip the local probe and attempt the handshake
        // unconditionally — that's the authoritative reachability test.
        true
    };
    probes.push(probe_port_owner(port, port_in_use, owner.as_ref(), local));

    // Skip the remaining probes when the port is dark on loopback; otherwise
    // attempt the handshake — for remote hosts that's the only reachability
    // signal we have.
    let mut firefox_version: Option<u32> = None;
    if port_in_use {
        match RdpConnection::connect(host, port, Duration::from_millis(cli.timeout)) {
            Ok(mut conn) => {
                firefox_version = conn.firefox_version();
                probes.push(Probe {
                    name: "rdp_handshake",
                    status: Status::Pass,
                    detail: format!("greeting received from {host}:{port}"),
                    hint: None,
                });

                // When the RDP greeting omits the `ua` field (some Firefox
                // builds and CI configurations do this), try the device actor's
                // `getDescription` response as a fallback source for the version
                // number.  This is a best-effort probe — failure is suppressed
                // and treated as "version still unknown".
                if firefox_version.is_none() {
                    // Ignore any protocol error from the device actor probe —
                    // it is purely informational and must not block the rest
                    // of the doctor output.
                    if let Ok(v) = DeviceActor::query_version(conn.transport_mut())
                        && v.is_some()
                    {
                        firefox_version = v;
                    }
                }

                match RootActor::list_tabs(conn.transport_mut()) {
                    Ok(t) => {
                        probes.push(probe_tabs(&t));
                    }
                    Err(e) => probes.push(Probe {
                        name: "tabs",
                        status: Status::Fail,
                        detail: format!("listTabs failed: {e}"),
                        hint: Some(
                            "the tab list could not be retrieved; reload Firefox or relaunch with `ff-rdp launch --temp-profile`".to_owned(),
                        ),
                    }),
                }
            }
            Err(e) => probes.push(Probe {
                name: "rdp_handshake",
                status: Status::Fail,
                detail: format!("RDP handshake failed: {e}"),
                hint: Some(
                    "the listener accepted TCP but did not return a Firefox greeting — check that --start-debugger-server matches --port and that the listener is Firefox"
                        .to_owned(),
                ),
            }),
        }
    } else {
        probes.push(Probe {
            name: "rdp_handshake",
            status: Status::Fail,
            detail: "skipped — no listener on the port".to_owned(),
            hint: Some(format!(
                "run `ff-rdp launch` to start Firefox with debugging on port {port}"
            )),
        });
        probes.push(Probe {
            name: "tabs",
            status: Status::Fail,
            detail: "skipped — no listener on the port".to_owned(),
            hint: None,
        });
    }

    // 5. Firefox version compatibility
    probes.push(probe_version(firefox_version));

    let any_failed = probes.iter().any(|p| p.status == Status::Fail);

    let results = build_results_json(&probes);
    let mut meta = json!({
        "host": host,
        "port": port,
    });
    crate::connection_meta::merge_into(&mut meta, host, port, firefox_version);
    let envelope = output::envelope(&results, probes.len(), &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)?;

    if any_failed {
        Err(AppError::Exit(1))
    } else {
        Ok(())
    }
}

fn probe_daemon(host: &str, port: u16) -> Probe {
    match find_running_daemon(host, port) {
        Ok(Some(info)) => Probe {
            name: "daemon",
            status: Status::Pass,
            detail: format!(
                "daemon running (PID {}, proxy port {}, started {})",
                info.pid, info.proxy_port, info.started_at
            ),
            hint: None,
        },
        Ok(None) => {
            // Look for a stale registry to surface that explicitly.
            match registry::read_registry() {
                Ok(Some(_)) => Probe {
                    name: "daemon",
                    status: Status::Warn,
                    detail: "daemon registry exists but PID is dead — stale entry was cleaned up"
                        .to_owned(),
                    hint: Some(
                        "no daemon is running; commands will connect directly to Firefox"
                            .to_owned(),
                    ),
                },
                Ok(None) => Probe {
                    name: "daemon",
                    status: Status::Pass,
                    detail: "no daemon running (commands will connect directly)".to_owned(),
                    hint: None,
                },
                Err(e) => Probe {
                    name: "daemon",
                    status: Status::Warn,
                    detail: format!("could not read daemon registry: {e:#}"),
                    hint: None,
                },
            }
        }
        Err(e) => Probe {
            name: "daemon",
            status: Status::Warn,
            detail: format!("daemon registry read error: {e:#}"),
            hint: None,
        },
    }
}

fn probe_port_owner(port: u16, in_use: bool, owner: Option<&PortOwner>, local: bool) -> Probe {
    if !local {
        return Probe {
            name: "port_owner",
            status: Status::Pass,
            detail: format!("port {port} is on a non-loopback host; skipping local OS probe"),
            hint: None,
        };
    }
    if !in_use {
        return Probe {
            name: "port_owner",
            status: Status::Fail,
            detail: format!("nothing is listening on port {port}"),
            hint: Some(format!(
                "run `ff-rdp launch --port {port}` to start Firefox with debugging enabled"
            )),
        };
    }
    match owner {
        Some(o) => {
            let uptime = match o.uptime_s {
                Some(s) => format!(", uptime {}", format_uptime(s)),
                None => String::new(),
            };
            let process = if o.process_name.is_empty() {
                String::new()
            } else {
                format!(" ({})", o.process_name)
            };
            Probe {
                name: "port_owner",
                status: Status::Pass,
                detail: format!("PID {}{process} is listening on port {port}{uptime}", o.pid),
                hint: None,
            }
        }
        None => Probe {
            name: "port_owner",
            status: Status::Warn,
            detail: format!("port {port} is in use but the owner could not be identified"),
            hint: Some(
                "install `lsof` (Unix) so doctor can identify the listener process".to_owned(),
            ),
        },
    }
}

fn probe_tabs(tabs: &[TabInfo]) -> Probe {
    if tabs.is_empty() {
        return Probe {
            name: "tabs",
            status: Status::Fail,
            detail: "Firefox is connected but exposes 0 tabs".to_owned(),
            hint: Some(
                "open a tab in Firefox, or relaunch with `ff-rdp launch --temp-profile` for a clean session".to_owned(),
            ),
        };
    }
    let selected = tabs.iter().filter(|t| t.selected).count();
    Probe {
        name: "tabs",
        status: Status::Pass,
        detail: format!("{} tab(s) available, {selected} selected", tabs.len()),
        hint: None,
    }
}

fn probe_version(version: Option<u32>) -> Probe {
    match version {
        None => Probe {
            name: "firefox_version",
            status: Status::Warn,
            detail: "Firefox version not advertised in the RDP greeting".to_owned(),
            hint: None,
        },
        Some(v) if (COMPATIBLE_FIREFOX_MIN..=COMPATIBLE_FIREFOX_MAX).contains(&v) => Probe {
            name: "firefox_version",
            status: Status::Pass,
            detail: format!(
                "Firefox {v} (within tested range {COMPATIBLE_FIREFOX_MIN}–{COMPATIBLE_FIREFOX_MAX})"
            ),
            hint: None,
        },
        Some(v) => Probe {
            name: "firefox_version",
            status: Status::Warn,
            detail: format!(
                "Firefox {v} is outside the tested range {COMPATIBLE_FIREFOX_MIN}–{COMPATIBLE_FIREFOX_MAX}"
            ),
            hint: Some(
                "some commands may misbehave on this version; report regressions at https://github.com/ractive/ff-rdp/issues".to_owned(),
            ),
        },
    }
}

fn build_results_json(probes: &[Probe]) -> Value {
    let arr: Vec<Value> = probes
        .iter()
        .map(|p| {
            let mut obj = serde_json::Map::new();
            obj.insert("name".to_string(), Value::String(p.name.to_string()));
            obj.insert(
                "status".to_string(),
                Value::String(p.status.as_str().to_string()),
            );
            obj.insert("detail".to_string(), Value::String(p.detail.clone()));
            obj.insert(
                "glyph".to_string(),
                Value::String(p.status.glyph().to_string()),
            );
            if let Some(hint) = &p.hint {
                obj.insert("hint".to_string(), Value::String(hint.clone()));
            }
            Value::Object(obj)
        })
        .collect();
    Value::Array(arr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_version_in_range_is_pass() {
        let p = probe_version(Some(149));
        assert_eq!(p.status, Status::Pass);
    }

    #[test]
    fn probe_version_out_of_range_is_warn() {
        let p = probe_version(Some(99));
        assert_eq!(p.status, Status::Warn);
    }

    #[test]
    fn probe_version_unknown_is_warn() {
        let p = probe_version(None);
        assert_eq!(p.status, Status::Warn);
    }

    #[test]
    fn build_results_includes_hint_when_present() {
        let probes = vec![Probe {
            name: "x",
            status: Status::Fail,
            detail: "bad".into(),
            hint: Some("try y".into()),
        }];
        let json = build_results_json(&probes);
        assert_eq!(json[0]["hint"], "try y");
        assert_eq!(json[0]["status"], "fail");
    }

    #[test]
    fn probe_tabs_zero_is_fail() {
        let p = probe_tabs(&[]);
        assert_eq!(p.status, Status::Fail);
    }
}
