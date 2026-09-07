//! iter-242 Part C — census of the live suite's third-party page dependencies.
//!
//! Two full dual-gate sweeps on the iteration-175 branch produced 272 passed /
//! 1 failed each, with a *different* single failure and both green in
//! isolation. One of them, `live_61r_eval::live_eval_on_hn`, got `""` back for
//! `document.title` on news.ycombinator.com. Iteration 181's sweep then hit
//! the same shape on theguardian.com through `live_137_consent_accept_via_daemon`.
//! Two named instances make it a class, not a test.
//!
//! This scan is the class's inventory. It is Firefox-free and network-free: it
//! reads the live tree's source and asserts two things.
//!
//! 1. **Every third-party host is declared.** A new external dependency has to
//!    be added to [`DECLARED_HOSTS`] below, with a note saying what the suite
//!    asserts about it. Silent growth is how the class got to fourteen files
//!    without anyone having decided it should.
//! 2. **Every file that navigates to one gates on `FF_RDP_LIVE_NETWORK_TESTS`.**
//!    A third-party page reached from the plain `FF_RDP_LIVE_TESTS` tier would
//!    red the sweep on a machine with no network at all.
//!
//! What the audit found (2026-09-06), recorded here because the finding is the
//! deliverable and a comment is where a future reader will look:
//!
//! - Every actual navigation to a third-party host is already behind the
//!   network gate. The five files that mention `en.wikipedia.org` outside that
//!   gate mention it only in prose — benchmark provenance for local fixtures.
//! - Only **five** of the fourteen assert on a third-party page's *content*
//!   rather than merely needing a real page: `live_eval_on_hn` (exact
//!   `document.title`), `live_eval_works_on_real_mdn` (a substring, so it
//!   survives a redesign), `live_137_consent_accept_via_daemon` (a consent
//!   banner and live frame targets), `live_cascade_real_site_cli` (an `h1`
//!   exists and carries rules), and `live_130`'s comparis navigation (asserts
//!   the committed *URL*, which is the site's contract with its own address
//!   and does not move when the page does).
//! - `live_eval_on_hn` is the one this iteration fixed: it now waits for
//!   `document.readyState == "complete"` and names readiness-versus-site in the
//!   failure message (`common::await_document_ready`).
//! - `live_137_consent_accept_via_daemon`'s `live_target_count: 0` signature is
//!   **not** touched here. Iteration 251 owns that signature; re-diagnosing it
//!   from this side would produce two half-owners for one failure.

use std::path::{Path, PathBuf};

/// Hosts the live suite is allowed to depend on, each with what is asserted
/// about it. Adding a host here is a deliberate act: it accepts that a sweep
/// can now be reddened by someone else's outage.
const DECLARED_HOSTS: [(&str, &str); 7] = [
    (
        "news.ycombinator.com",
        "live_61r_eval: strict-CSP page for `eval`; asserts document.title exactly, \
         behind a readiness wait (iter-242)",
    ),
    (
        "developer.mozilla.org",
        "live_eval_csp / live_daemon_stop_mdn / live_94_polish_bundle: real CSP page; \
         asserts the title *contains* \"MDN\"",
    ),
    (
        "www.theguardian.com",
        "live_137 / live_129 / live_159 / live_128: consent banner and network-event \
         volume on a real page (the live_137 signature belongs to iteration 251)",
    ),
    (
        "en.wikipedia.org",
        "live_126 / live_128 / live_111 / live_159: a real page with enough subresources \
         to exercise the network tier; no content assertions",
    ),
    (
        "www.comparis.ch",
        "live_130: asserts the committed URL, not page content",
    ),
    (
        "tennis-sepp.ch",
        "live_cascade_real_site: asserts an h1 exists and carries cascade rules",
    ),
    (
        "www.bbc.com",
        "live_144: a real page for session-hygiene assertions; no content assertions",
    ),
];

/// Hosts that are not third-party dependencies at all: loopback, the local
/// fixture server, and the two deliberately unresolvable names used to drive
/// DNS-failure paths.
const NOT_THIRD_PARTY: [&str; 6] = [
    "127.0.0.1",
    "localhost",
    "example.com",
    "www.w3.org",
    "this-domain-totally-does-not-exist-61l-zzz.invalid",
    "this-domain-totally-does-not-exist-174-zzz.invalid",
];

fn live_test_files() -> Vec<PathBuf> {
    let live_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/live");
    let mut out = Vec::new();
    let mut stack = vec![live_root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Every `https://<host>` / `http://<host>` literal on `line`, as hosts.
///
/// `example.com` and the `.invalid` names come back too; they are filtered
/// against [`NOT_THIRD_PARTY`] by the caller rather than here, so a new
/// loopback-ish host is a deliberate addition to that list.
fn hosts_on(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (idx, _) in line.match_indices("://") {
        let rest = &line[idx + 3..];
        let host: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
            .collect();
        if !host.is_empty() {
            out.push(host);
        }
    }
    out
}

/// `true` if the line is inside a `//` or `//!` comment — prose provenance
/// (`"measured against en.wikipedia.org"`) is not a dependency.
fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// AC (iter-242 Part C): the suite's third-party dependencies are declared,
/// and every one of them is reached only behind `FF_RDP_LIVE_NETWORK_TESTS`.
#[test]
fn iter_242_third_party_hosts_are_declared_and_network_gated() {
    let declared: Vec<&str> = DECLARED_HOSTS.iter().map(|(h, _)| *h).collect();
    let mut undeclared: Vec<String> = Vec::new();
    let mut ungated: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    for path in live_test_files() {
        let file = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_owned();
        let src = std::fs::read_to_string(&path).expect("read live test source");
        let gated =
            src.contains("FF_RDP_LIVE_NETWORK_TESTS") || src.contains("live_network_tests_enabled");

        for (idx, line) in src.lines().enumerate() {
            if is_comment(line) {
                continue;
            }
            for host in hosts_on(line) {
                if NOT_THIRD_PARTY.contains(&host.as_str()) {
                    continue;
                }
                if !seen.contains(&host) {
                    seen.push(host.clone());
                }
                if !declared.contains(&host.as_str()) {
                    undeclared.push(format!("{file}:{}: {host}", idx + 1));
                } else if !gated {
                    ungated.push(format!("{file}:{}: {host}", idx + 1));
                }
            }
        }
    }

    assert!(
        undeclared.is_empty(),
        "undeclared third-party hosts — add each to DECLARED_HOSTS with what the suite \
         asserts about it, or use the local fixture server instead ({}):\n  {}",
        undeclared.len(),
        undeclared.join("\n  ")
    );
    assert!(
        ungated.is_empty(),
        "third-party navigations outside the FF_RDP_LIVE_NETWORK_TESTS gate ({}):\n  {}",
        ungated.len(),
        ungated.join("\n  ")
    );

    // A declaration nobody uses any more is stale documentation that reads as
    // a live dependency. Drop it rather than leaving the census overstating
    // what the suite reaches for.
    let stale: Vec<&str> = declared
        .iter()
        .copied()
        .filter(|h| !seen.iter().any(|s| s == h))
        .collect();
    assert!(
        stale.is_empty(),
        "DECLARED_HOSTS names hosts the live tree no longer navigates to: {stale:?}"
    );
}

/// The census is worthless if it stops finding the dependencies it is meant to
/// track — a moved tree or a broken host parser would leave it green and
/// blind.
#[test]
fn iter_242_third_party_census_is_not_vacuous() {
    let mut found = 0usize;
    for path in live_test_files() {
        let src = std::fs::read_to_string(&path).expect("read live test source");
        for line in src.lines().filter(|l| !is_comment(l)) {
            found += hosts_on(line)
                .into_iter()
                .filter(|h| !NOT_THIRD_PARTY.contains(&h.as_str()))
                .count();
        }
    }
    assert!(
        found >= 10,
        "expected at least 10 third-party URL literals across the live tree, found {found} — \
         the scan is looking at the wrong tree or the parser broke"
    );
}
