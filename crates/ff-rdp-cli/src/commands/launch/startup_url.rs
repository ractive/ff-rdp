//! `launch <URL>`: wait until the tab the next command will use shows the
//! startup URL's document, fully loaded (iter-300).
//!
//! `listTabs` is no proof: it reports the startup URL as soon as Firefox
//! starts loading it, while the tab's document is still the initial
//! `about:blank` (`readyState` `uninitialized`). An `eval` run right after a
//! plain `launch --headless <URL>` read that document in 22 of 30 runs
//! (Firefox 160, local http page). So each poll does what the next command
//! does — a fresh `listTabs`, the selected tab, `getTarget`, one evaluation in
//! its console — and the wait ends once that document is the URL's (or a
//! redirect's) and `readyState` is `complete`.
//!
//! Shared by the plain path (Firefox got the URL on its command line) and the
//! `--temporary-addon` path (after its `navigateTo`); both turn a failed wait
//! into a `results.warnings` entry, never a failed launch. The add-on path
//! also opens the URL again when Firefox dropped its navigation (see
//! [`wait_for_document`]).

use std::time::{Duration, Instant};

use ff_rdp_core::{Grip, RdpConnection, RootActor, WebConsoleActor};

use crate::error::AppError;

/// One evaluation: the document's URL, its `readyState`, and its
/// `documentURI` (an error page keeps `location.href` at the requested URL
/// and shows itself only there).
const PROBE: &str = "JSON.stringify([location.href, document.readyState, document.documentURI])";

/// How long one poll may take; a hung reply is dropped with its connection.
const ATTEMPT: Duration = Duration::from_secs(2);

/// Between polls.
const POLL: Duration = Duration::from_millis(100);

/// How many polls in a row must find the tab back on its old document, fully
/// loaded, before a dropped navigation is opened again.
const SETTLED_POLLS: u32 = 3;

/// How often a dropped navigation is opened again.
const RENAVIGATIONS: u32 = 2;

/// Opens the URL once more after Firefox dropped the navigation.
pub(crate) type Renavigate<'a> = &'a mut dyn FnMut() -> Result<(), AppError>;

/// Wait until the selected tab of the Firefox on `127.0.0.1:port` shows `url`
/// loaded, or `deadline` passes.
///
/// `before` is the URL the tab showed before this navigation (`about:blank`
/// for a fresh start); see [`committed`] for how redirects count. Firefox's
/// error page for the URL ends the wait early with an error naming it.
///
/// `renavigate` is for a navigation ff-rdp started itself (`navigateTo`):
/// when the tab stays on its old, fully loaded document, Firefox dropped the
/// load, and `renavigate` opens the URL again (at most [`RENAVIGATIONS`]
/// times). The tab descriptor's `navigateTo(waitForLoad)` replies on the
/// `STATE_STOP` of a load of the URL without checking its status, so an
/// aborted load replies like a finished one.
pub(crate) fn wait_for_document(
    port: u16,
    url: &str,
    before: &str,
    deadline: Instant,
    renavigate: Option<Renavigate<'_>>,
) -> Result<(), AppError> {
    let mut connection: Option<RdpConnection> = None;
    wait_with(
        |attempt_deadline| {
            let state = sample(&mut connection, port, attempt_deadline);
            if state.is_err() {
                // A timed-out or failed request can leave a late reply on the
                // socket; the next poll starts on a fresh connection.
                connection = None;
            }
            state
        },
        url,
        before,
        deadline,
        renavigate,
    )
}

/// [`wait_for_document`] over an injected `sample`, so the polling rules are
/// unit-testable.
fn wait_with(
    mut sample: impl FnMut(Instant) -> Result<DocumentState, AppError>,
    url: &str,
    before: &str,
    deadline: Instant,
    mut renavigate: Option<Renavigate<'_>>,
) -> Result<(), AppError> {
    let started = Instant::now();
    // Distinct consecutive observations, for the timeout message.
    let mut seen: Vec<String> = Vec::new();
    let (mut settled, mut renavigated) = (0, 0);
    while Instant::now() < deadline {
        let observed = match sample(deadline.min(Instant::now() + ATTEMPT)) {
            Ok(state) => match state.verdict(url, before) {
                Verdict::Loaded => {
                    tracing::debug!(
                        elapsed_ms = started.elapsed().as_millis(),
                        href = %state.href,
                        renavigated,
                        "launch: startup URL loaded"
                    );
                    return Ok(());
                }
                Verdict::ErrorPage(page) => {
                    return Err(AppError::User(format!(
                        "Firefox shows an error page ({page})"
                    )));
                }
                Verdict::Pending => {
                    settled = if state.href == before && state.ready_state == "complete" {
                        settled + 1
                    } else {
                        0
                    };
                    format!("{} {}", state.href, state.ready_state)
                }
            },
            Err(error) => error.to_string(),
        };
        if seen.last() != Some(&observed) {
            tracing::debug!(elapsed_ms = started.elapsed().as_millis(), %observed, "launch: startup URL wait");
            seen.push(observed);
        }
        if settled >= SETTLED_POLLS
            && renavigated < RENAVIGATIONS
            && let Some(renavigate) = renavigate.as_mut()
        {
            renavigated += 1;
            settled = 0;
            tracing::debug!(
                renavigated,
                "launch: the navigation was dropped; opening the URL again"
            );
            seen.push(format!("navigation dropped, opened again ({renavigated})"));
            if let Err(error) = renavigate() {
                seen.push(error.to_string());
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        std::thread::sleep(POLL.min(remaining));
    }
    let recent = seen.len().saturating_sub(4);
    Err(AppError::User(format!(
        "not loaded after {} ms (selected tab: {}); the next command may race the navigation",
        started.elapsed().as_millis(),
        if seen.is_empty() {
            "no answer".to_owned()
        } else {
            seen[recent..].join(" → ")
        }
    )))
}

/// One poll, the way the next command reaches the page.
fn sample(
    connection: &mut Option<RdpConnection>,
    port: u16,
    deadline: Instant,
) -> Result<DocumentState, AppError> {
    let budget = deadline.saturating_duration_since(Instant::now());
    if budget.is_zero() {
        return Err(AppError::Timeout("startup URL wait".into()));
    }
    let connection = match connection {
        Some(open) => open,
        None => connection.insert(RdpConnection::connect("127.0.0.1", port, budget)?),
    };
    let transport = connection.transport_mut();
    let tabs = transport.with_read_deadline(deadline, RootActor::list_tabs)?;
    let tab = crate::tab_target::resolve_tab_with_context(&tabs, None, None, "127.0.0.1", port)?;
    let target = crate::commands::connect_tab::resolve_target(transport, &tab.actor, deadline)?;
    let result = transport.with_read_deadline(deadline, |t| {
        WebConsoleActor::evaluate_js_async(t, &target.console_actor, PROBE)
    })?;
    match (result.exception, result.result) {
        (None, Grip::Value(serde_json::Value::String(json))) => DocumentState::parse(&json),
        (exception, result) => Err(AppError::User(format!(
            "unexpected probe result {result:?} (exception {exception:?})"
        ))),
    }
}

/// What the probe read from the tab's document.
#[derive(Debug)]
struct DocumentState {
    href: String,
    ready_state: String,
    document_uri: String,
}

#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Loaded,
    Pending,
    /// Firefox's error page for the URL, as `about:neterror?e=…`.
    ErrorPage(String),
}

impl DocumentState {
    fn parse(json: &str) -> Result<Self, AppError> {
        let [href, ready_state, document_uri]: [String; 3] = serde_json::from_str(json)
            .map_err(|e| AppError::User(format!("unreadable probe result {json}: {e}")))?;
        Ok(Self {
            href,
            ready_state,
            document_uri,
        })
    }

    fn verdict(&self, url: &str, before: &str) -> Verdict {
        if !committed(&self.href, url, before) {
            return Verdict::Pending;
        }
        if let Some(page) = error_page(&self.document_uri) {
            return Verdict::ErrorPage(page.to_owned());
        }
        if self.ready_state == "complete" {
            Verdict::Loaded
        } else {
            Verdict::Pending
        }
    }
}

/// `about:neterror?e=dnsNotFound` from an error page's `documentURI`, without
/// the failed URL that follows.
fn error_page(document_uri: &str) -> Option<&str> {
    [
        "about:neterror",
        "about:certerror",
        "about:blocked",
        "about:httpsonlyerror",
    ]
    .iter()
    .any(|page| document_uri.starts_with(page))
    .then(|| document_uri.split('&').next().unwrap_or(document_uri))
}

/// Has the tab committed the requested navigation? It has when it shows the
/// requested URL, or — allowing for redirects — any URL other than the one it
/// showed before the navigation and the initial `about:blank`.
fn committed(current: &str, requested: &str, before: &str) -> bool {
    current == requested || (current != before && current != "about:blank")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(href: &str, ready_state: &str, document_uri: &str) -> DocumentState {
        DocumentState {
            href: href.to_owned(),
            ready_state: ready_state.to_owned(),
            document_uri: document_uri.to_owned(),
        }
    }

    #[test]
    fn commit_means_requested_url_or_a_redirect_away_from_the_start() {
        let blank = "about:blank";
        assert!(committed("https://a/", "https://a/", blank));
        assert!(committed("about:blank", "about:blank", "about:home"));
        assert!(committed("https://www.a/", "https://a/", blank));
        assert!(!committed(blank, "https://a/", blank));
        assert!(!committed("about:home", "https://a/", "about:home"));
    }

    #[test]
    fn the_initial_blank_document_is_pending_even_while_listtabs_shows_the_url() {
        // What an immediate `eval` read after a plain `launch <URL>`.
        let initial = state("about:blank", "uninitialized", "about:blank");
        assert_eq!(
            initial.verdict("http://127.0.0.1:1/", "about:blank"),
            Verdict::Pending
        );
        let loading = state("http://127.0.0.1:1/", "interactive", "http://127.0.0.1:1/");
        assert_eq!(
            loading.verdict("http://127.0.0.1:1/", "about:blank"),
            Verdict::Pending
        );
        let loaded = state("http://127.0.0.1:1/", "complete", "http://127.0.0.1:1/");
        assert_eq!(
            loaded.verdict("http://127.0.0.1:1/", "about:blank"),
            Verdict::Loaded
        );
    }

    #[test]
    fn an_error_page_ends_the_wait_with_its_kind() {
        let failed = state(
            "http://127.0.0.1:1/",
            "complete",
            "about:neterror?e=connectionFailure&u=http%3A//127.0.0.1%3A1/&c=UTF-8",
        );
        assert_eq!(
            failed.verdict("http://127.0.0.1:1/", "about:blank"),
            Verdict::ErrorPage("about:neterror?e=connectionFailure".to_owned())
        );
        assert_eq!(
            error_page("about:httpsonlyerror?e=httpsOnlyErrorPage&u=http%3A//a/"),
            Some("about:httpsonlyerror?e=httpsOnlyErrorPage")
        );
        assert_eq!(error_page("http://127.0.0.1:1/"), None);
    }

    /// Feed `wait_with` a fixed sequence of observations (the last repeats).
    fn run(
        states: &[(&str, &str)],
        renavigate: Option<Renavigate<'_>>,
        budget: Duration,
    ) -> (Result<(), AppError>, usize) {
        let mut polls = 0;
        let result = wait_with(
            |_| {
                let (href, ready) = states[polls.min(states.len() - 1)];
                polls += 1;
                Ok(state(href, ready, href))
            },
            "http://a/",
            "about:blank",
            Instant::now() + budget,
            renavigate,
        );
        (result, polls)
    }

    #[test]
    fn a_dropped_navigation_is_opened_again_once_the_old_page_settles() {
        let mut calls = 0;
        let mut renavigate = || {
            calls += 1;
            Ok(())
        };
        let blank = ("about:blank", "complete");
        let (result, polls) = run(
            &[
                blank,
                blank,
                blank,
                ("http://a/", "loading"),
                ("http://a/", "complete"),
            ],
            Some(&mut renavigate),
            Duration::from_secs(5),
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(polls, 5);
        assert_eq!(calls, 1);
    }

    #[test]
    fn renavigation_is_bounded_and_the_timeout_names_what_the_tab_showed() {
        let mut calls = 0;
        let mut renavigate = || {
            calls += 1;
            Ok(())
        };
        let (result, _) = run(
            &[("about:blank", "complete")],
            Some(&mut renavigate),
            Duration::from_millis(1500),
        );
        assert_eq!(calls, RENAVIGATIONS);
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains("about:blank complete") && message.contains("opened again"),
            "{message}"
        );
    }

    #[test]
    fn without_renavigate_the_initial_document_is_just_waited_out() {
        // The plain path: Firefox loads the URL itself; never navigate.
        let (result, polls) = run(
            &[
                ("about:blank", "uninitialized"),
                ("about:blank", "uninitialized"),
                ("http://a/", "complete"),
            ],
            None,
            Duration::from_secs(5),
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(polls, 3);
    }

    #[test]
    fn probe_result_parses_and_rejects_other_shapes() {
        let parsed = DocumentState::parse(r#"["https://a/","complete","https://a/"]"#)
            .expect("three strings");
        assert_eq!(parsed.href, "https://a/");
        assert!(DocumentState::parse(r#"["https://a/","complete"]"#).is_err());
        assert!(DocumentState::parse("null").is_err());
    }
}
