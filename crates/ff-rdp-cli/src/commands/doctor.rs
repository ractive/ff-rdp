//! `ff-rdp doctor` — top-to-bottom connection diagnostic.
//!
//! Probes each layer of the stack and reports the first one that fails so the
//! user (or AI agent) sees a single concrete next step. Exits 0 when every
//! probe passes, 1 when any probe fails — CI friendly.

use std::path::Path;
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
    /// The probe did not apply in this context and was intentionally not run
    /// (e.g. binary_staleness outside an ff-rdp checkout). Never counts as a
    /// failure — exit code is unaffected.
    Skipped,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Warn => "warn",
            Self::Fail => "fail",
            Self::Skipped => "skipped",
        }
    }
    fn glyph(self) -> &'static str {
        match self {
            Self::Pass => "✓",
            Self::Warn => "⚠",
            Self::Fail => "✗",
            Self::Skipped => "–",
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

    // 6. Binary staleness — compare embedded build SHA to git HEAD in CWD, but
    // only when the CWD is actually an ff-rdp checkout. Comparing against a
    // foreign repo's HEAD (observed firing against the user's `neon` repo)
    // produces a confidently-wrong staleness warning, so guard it (iter-98
    // Theme C).
    let embedded_sha = env!("FF_RDP_BUILD_VERSION_SHA");
    let in_ff_rdp_checkout = is_ff_rdp_checkout();
    let head_sha_result = git_head_sha();
    probes.push(probe_binary_staleness(
        embedded_sha,
        head_sha_result,
        in_ff_rdp_checkout,
    ));

    // 7. Profile disk usage — warn when managed profile dirs have piled up
    // faster than `launch`'s bounded auto-prune (iter-96 Theme B) keeps up.
    probes.push(probe_profile_disk_usage());

    let any_failed = probes.iter().any(|p| p.status == Status::Fail);

    let results = build_results_json(&probes);
    let version_long = crate::cli::args::build_version_string();
    let mut meta = json!({
        "host": host,
        "port": port,
        "version_long": version_long,
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
            match registry::read_registry(port) {
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
        Some(v) if v < COMPATIBLE_FIREFOX_MIN => Probe {
            name: "firefox_version",
            status: Status::Pass,
            detail: format!(
                "Firefox {v} (older than tested range {COMPATIBLE_FIREFOX_MIN}–{COMPATIBLE_FIREFOX_MAX}, may lack newer features but should still work)"
            ),
            hint: None,
        },
        Some(v) => Probe {
            name: "firefox_version",
            status: Status::Pass,
            detail: format!(
                "Firefox {v} (newer than tested range {COMPATIBLE_FIREFOX_MIN}–{COMPATIBLE_FIREFOX_MAX}, but supported)"
            ),
            hint: None,
        },
    }
}

/// Run `git rev-parse HEAD` in the CWD and return the trimmed stdout on success,
/// or `Err(())` if git is unavailable, exits non-zero, or CWD is not a repo.
fn git_head_sha() -> Result<String, ()> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|_| ())?;
    if !output.status.success() {
        return Err(());
    }
    let sha = String::from_utf8(output.stdout).map_err(|_| ())?;
    Ok(sha.trim().to_owned())
}

/// True when the CWD sits inside the ff-rdp workspace checkout.
///
/// The binary_staleness probe compares the running binary's embedded build SHA
/// against the CWD repo's git HEAD. That comparison is only meaningful inside
/// the ff-rdp checkout the binary was built from — run from a *foreign* repo it
/// produces a confidently-wrong warning (observed firing against the user's
/// `neon` repo). We resolve the git repo root via `git rev-parse --show-toplevel`
/// and treat the checkout as ff-rdp iff the workspace membership marker
/// `crates/ff-rdp-core/Cargo.toml` exists at that root.
///
/// Returns `false` when git is unavailable, the CWD is not a repo, or the
/// marker is absent — in every "not clearly ff-rdp" case the caller reports the
/// staleness check as `skipped` rather than comparing against a foreign HEAD.
fn is_ff_rdp_checkout() -> bool {
    let output = match std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };
    let Ok(root) = String::from_utf8(output.stdout) else {
        return false;
    };
    let root = root.trim();
    if root.is_empty() {
        return false;
    }
    // Workspace membership marker — the core crate is unique to this workspace.
    Path::new(root)
        .join("crates")
        .join("ff-rdp-core")
        .join("Cargo.toml")
        .is_file()
}

/// Pure core logic — takes the embedded SHA, the git HEAD result, and whether
/// the CWD is an ff-rdp checkout, so that unit tests don't need a git binary.
///
/// When `in_ff_rdp_checkout` is `false` the probe reports `skipped` with the
/// reason "not in an ff-rdp checkout" instead of comparing the binary against a
/// foreign repo's HEAD (iter-98 Theme C). The empty-SHA (tarball/hermetic)
/// short-circuit still runs first because that verdict holds regardless of CWD.
fn probe_binary_staleness(
    embedded_sha: &str,
    head_sha_result: Result<String, ()>,
    in_ff_rdp_checkout: bool,
) -> Probe {
    // Empty embedded SHA means hermetic/tarball build — no provenance to check.
    if embedded_sha.is_empty() {
        return Probe {
            name: "binary_staleness",
            status: Status::Pass,
            detail: "binary was built without git provenance (tarball or hermetic build)"
                .to_owned(),
            hint: None,
        };
    }

    // Repo-identity guard: outside an ff-rdp checkout the CWD repo's HEAD is
    // unrelated to the binary's build SHA, so a comparison would be
    // confidently wrong. Report the probe as `skipped` instead (iter-98 Theme C).
    if !in_ff_rdp_checkout {
        return Probe {
            name: "binary_staleness",
            status: Status::Skipped,
            detail: "not in an ff-rdp checkout".to_owned(),
            hint: None,
        };
    }

    // Strip any `+dirty` suffix before comparing.
    let clean_sha = embedded_sha
        .split_once('+')
        .map_or(embedded_sha, |(base, _)| base);

    let Ok(head_sha) = head_sha_result else {
        return Probe {
            name: "binary_staleness",
            status: Status::Pass,
            detail:
                "current directory is not a git repo (or git not available); staleness check skipped"
                    .to_owned(),
            hint: None,
        };
    };

    // The embedded SHA is short (12 chars); HEAD is full 40. Compare as a prefix.
    if head_sha.starts_with(clean_sha) {
        let display = &head_sha[..head_sha.len().min(12)];
        return Probe {
            name: "binary_staleness",
            status: Status::Pass,
            detail: format!("installed binary matches HEAD ({display})"),
            hint: None,
        };
    }

    let head_short = &head_sha[..head_sha.len().min(12)];
    Probe {
        name: "binary_staleness",
        status: Status::Warn,
        detail: format!(
            "installed binary SHA {clean_sha} differs from HEAD {head_short} — your local checkout is ahead of the installed binary"
        ),
        hint: Some(
            "run `cargo install --path crates/ff-rdp-cli --force` to rebuild and reinstall"
                .to_owned(),
        ),
    }
}

/// Warning thresholds for `profile_disk_usage` (iter-96 Theme C): beyond
/// either of these, the check goes from `pass` to `warn`. Chosen generously
/// — normal usage self-prunes via Theme B (`launch`'s bounded auto-prune); a
/// warn here means Theme B isn't keeping up, e.g. because Firefox is never
/// started via `ff-rdp launch` on this machine, or `FF_RDP_PROFILE_PRUNE_MAX`
/// was lowered.
const PROFILE_DISK_USAGE_WARN_COUNT: usize = 100;
const PROFILE_DISK_USAGE_WARN_BYTES: u64 = 1024 * 1024 * 1024; // 1 GiB

/// Resolve the real profile root and delegate to [`probe_profile_disk_usage_at`].
///
/// A missing/unreadable profile root (no per-user state/data directory
/// available, or `ff-rdp launch` has simply never run here) is reported as
/// `pass`, not `fail` — this check exists to catch *accumulation*, not to
/// gate on the root's mere existence.
fn probe_profile_disk_usage() -> Probe {
    match crate::util::profile_dir::secure_profile_root() {
        Ok(root) => probe_profile_disk_usage_at(&root),
        Err(_) => Probe {
            name: "profile_disk_usage",
            status: Status::Pass,
            detail: "profile root unavailable — nothing to check".to_owned(),
            hint: None,
        },
    }
}

/// Pure core logic — takes `&Path` so unit tests can point it at a temp dir
/// instead of the real `secure_profile_root()`.
fn probe_profile_disk_usage_at(root: &Path) -> Probe {
    // Capped walk: the size scan stops once it crosses the warn threshold,
    // so `doctor` stays fast even on a multi-GiB backlog. Past the cap the
    // reported figure is a lower bound, hence the "at least" wording below.
    let summary =
        crate::commands::profiles::aggregate_profiles_capped(root, PROFILE_DISK_USAGE_WARN_BYTES);
    let size_qualifier = if summary.total_size_bytes > PROFILE_DISK_USAGE_WARN_BYTES {
        "at least "
    } else {
        ""
    };
    let detail = format!(
        "{} managed profile dir(s), {}{} bytes under {}",
        summary.count,
        size_qualifier,
        summary.total_size_bytes,
        root.display()
    );

    if summary.count > PROFILE_DISK_USAGE_WARN_COUNT
        || summary.total_size_bytes > PROFILE_DISK_USAGE_WARN_BYTES
    {
        Probe {
            name: "profile_disk_usage",
            status: Status::Warn,
            detail,
            hint: Some(
                "run `ff-rdp profiles prune` to remove stale profile directories".to_owned(),
            ),
        }
    } else {
        Probe {
            name: "profile_disk_usage",
            status: Status::Pass,
            detail,
            hint: None,
        }
    }
}

/// Build the JSON `results` array for a set of probes.
///
/// Key insertion order matters: with `serde_json`'s `preserve_order` feature
/// enabled, `Value::Object` preserves insertion order, and the text-table
/// renderer (`render_table` in `output_pipeline.rs`) derives column order
/// from the first row's key order. We insert narrow, "at a glance" columns
/// first (glyph, name, status) and the wide free-text columns (hint, detail)
/// last, so the table stays readable instead of being pushed off-screen by
/// `detail`. This does not change the JSON shape — `hint` is still omitted
/// entirely when `None`.
fn build_results_json(probes: &[Probe]) -> Value {
    let arr: Vec<Value> = probes
        .iter()
        .map(|p| {
            let mut obj = serde_json::Map::new();
            obj.insert(
                "glyph".to_string(),
                Value::String(p.status.glyph().to_string()),
            );
            obj.insert("name".to_string(), Value::String(p.name.to_string()));
            obj.insert(
                "status".to_string(),
                Value::String(p.status.as_str().to_string()),
            );
            if let Some(hint) = &p.hint {
                obj.insert("hint".to_string(), Value::String(hint.clone()));
            }
            obj.insert("detail".to_string(), Value::String(p.detail.clone()));
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
    fn probe_version_out_of_range_is_pass() {
        // Versions newer or older than the tested range are reported as Pass
        // — the RDP surface rarely breaks across Firefox releases and shouting
        // on every run is more noise than signal.  The detail string distinguishes
        // older vs newer so users can read it correctly.
        let older = probe_version(Some(99));
        assert_eq!(older.status, Status::Pass);
        assert!(
            older.detail.contains("older than"),
            "older-version detail should say 'older than': {}",
            older.detail
        );
        let newer = probe_version(Some(999));
        assert_eq!(newer.status, Status::Pass);
        assert!(
            newer.detail.contains("newer than"),
            "newer-version detail should say 'newer than': {}",
            newer.detail
        );
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

    // --- binary_staleness tests ---

    #[test]
    fn unit_doctor_binary_staleness_check_short_circuits_outside_repo() {
        let p = probe_binary_staleness("abc123def456", Err(()), true);
        assert_eq!(p.status, Status::Pass);
    }

    #[test]
    fn unit_doctor_binary_staleness_check_short_circuits_without_git() {
        // No-git and outside-repo are the same code path; assert the detail
        // string explicitly so a regression that removes the early-return is caught.
        let p = probe_binary_staleness("abc123def456", Err(()), true);
        assert_eq!(p.status, Status::Pass);
        let detail_lower = p.detail.to_lowercase();
        assert!(
            detail_lower.contains("skipped") || detail_lower.contains("git"),
            "detail should mention 'skipped' or 'git': {}",
            p.detail
        );
    }

    #[test]
    fn unit_doctor_binary_staleness_check_empty_embedded_sha() {
        let p = probe_binary_staleness(
            "",
            Ok("abc123def4567890abcdef0123456789012345678".into()),
            true,
        );
        assert_eq!(p.status, Status::Pass);
        let detail_lower = p.detail.to_lowercase();
        assert!(
            detail_lower.contains("tarball") || detail_lower.contains("without git"),
            "detail should mention tarball or hermetic: {}",
            p.detail
        );
    }

    #[test]
    fn unit_doctor_binary_staleness_check_matching_sha_is_pass() {
        let p = probe_binary_staleness(
            "abc123def456",
            Ok("abc123def4567890abcdef0123456789012345678".into()),
            true,
        );
        assert_eq!(p.status, Status::Pass);
    }

    #[test]
    fn unit_doctor_binary_staleness_check_strips_dirty_suffix() {
        let p = probe_binary_staleness(
            "abc123def456+dirty",
            Ok("abc123def4567890abcdef0123456789012345678".into()),
            true,
        );
        assert_eq!(
            p.status,
            Status::Pass,
            "+dirty suffix must not break prefix match: {}",
            p.detail
        );
    }

    /// iter-98 Theme C: outside an ff-rdp checkout the probe reports `skipped`
    /// with the reason "not in an ff-rdp checkout" — never comparing the binary
    /// against a foreign repo's HEAD, even when that HEAD differs (which pre-fix
    /// would have produced a spurious `warn`).
    #[test]
    fn unit_doctor_binary_staleness_skipped_outside_ff_rdp_checkout() {
        let p = probe_binary_staleness(
            "abc123def456",
            // A foreign HEAD that differs from the embedded SHA — pre-fix this
            // would have warned; post-fix it must be skipped.
            Ok("999999999999000000000000000000000000aaaa".into()),
            false,
        );
        assert_eq!(
            p.status,
            Status::Skipped,
            "outside an ff-rdp checkout the probe must be skipped, not compared: {}",
            p.detail
        );
        assert!(
            p.detail.contains("not in an ff-rdp checkout"),
            "detail must carry the skip reason: {}",
            p.detail
        );
        assert!(
            p.hint.is_none(),
            "a skipped probe carries no remediation hint: {p:?}"
        );
    }

    #[test]
    fn pre_fix_repro_doctor_warns_when_installed_sha_differs_from_head() {
        let p = probe_binary_staleness(
            "abc123def456",
            Ok("999999999999000000000000000000000000aaaa".into()),
            true,
        );
        assert_eq!(p.status, Status::Warn);
        assert!(
            p.detail.contains("abc123def456"),
            "detail should contain embedded SHA: {}",
            p.detail
        );
        assert!(
            p.detail.contains("999999999999"),
            "detail should contain head short SHA: {}",
            p.detail
        );
        let hint = p.hint.expect("Warn probe must have a hint");
        assert!(
            hint.contains("cargo install"),
            "hint should mention cargo install: {hint}"
        );
    }

    // --- profile_disk_usage tests (iter-96 Theme C) ---

    /// AC: `unit_doctor_profile_disk_usage_warns_above_threshold` — 101
    /// managed profile dirs (empty is fine — this exercises the count
    /// threshold, not the size threshold) trips the check to `warn`.
    #[test]
    fn unit_doctor_profile_disk_usage_warns_above_threshold() {
        let root = tempfile::tempdir().expect("tempdir");
        for i in 0..101 {
            std::fs::create_dir_all(root.path().join(format!("ff-rdp-profile-{i:016}")))
                .expect("create fake profile dir");
        }

        let p = probe_profile_disk_usage_at(root.path());

        assert_eq!(p.status, Status::Warn, "detail: {}", p.detail);
        let hint = p.hint.expect("Warn probe must have a hint");
        assert!(
            hint.contains("profiles prune"),
            "hint should point at `ff-rdp profiles prune`: {hint}"
        );
    }

    #[test]
    fn unit_doctor_profile_disk_usage_pass_below_threshold() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(
            root.path()
                .join(format!("ff-rdp-profile-{}", "a".repeat(16))),
        )
        .expect("create fake profile dir");

        let p = probe_profile_disk_usage_at(root.path());

        assert_eq!(p.status, Status::Pass, "detail: {}", p.detail);
        assert!(p.hint.is_none());
    }

    #[test]
    fn unit_doctor_profile_disk_usage_pass_when_root_missing() {
        let root = tempfile::tempdir().expect("tempdir");
        let missing = root.path().join("does-not-exist");

        let p = probe_profile_disk_usage_at(&missing);

        assert_eq!(
            p.status,
            Status::Pass,
            "a missing profile root must never be reported as a failure: {}",
            p.detail
        );
    }
}
