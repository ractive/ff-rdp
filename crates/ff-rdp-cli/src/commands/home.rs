//! Bare `ff-rdp` — the content-first home view (iter-212 Theme A).
//!
//! Until this iteration, `ff-rdp` with no subcommand printed clap's usage dump
//! and exited 2. Every one of the 84 benchmark runs in
//! `kb/research/axi-benchmark-comparison.md` opened with a `--help` turn, and
//! `--help` cannot answer the only three questions an agent actually has at
//! that moment: *is there a browser, what is on it, and what do I type next?*
//!
//! This command answers exactly those, always exits 0 (a missing browser is
//! state, not an error), and is the payload of the `SessionStart` hook
//! [`crate::commands::install_hook`] writes.
//!
//! ## What it will not do
//!
//! It never starts anything. `launch` starts Firefox and `daemon start` starts
//! the daemon; the home view only reports. In particular it attaches to an
//! already-running daemon when there is one — so `page` carries `--ref`
//! handles — but never auto-starts one, because it runs on every agent session
//! start and a multi-second daemon spawn hidden behind a bare command would be
//! a nasty surprise.

use std::fmt::Write as _;
use std::time::Duration;

use ff_rdp_core::{RdpConnection, RootActor};
use serde_json::{Value, json};

use crate::cli::args::{Cli, HomeArgs};
use crate::daemon::client::find_running_daemon;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{connect_and_get_target, connect_direct};
use super::page_view::{self, CollectOptions, DEFAULT_INTERACTIVE_LIMIT};
use super::skill_doc::{DESCRIPTION, IDIOMS};

/// Interactive entries kept in the `page` block when the view runs as a
/// session hook.
///
/// The hook fires on every agent session, so its output is a standing tax on
/// the context window: headings plus the first fifteen controls is enough to
/// name the page and act on it, and an agent that needs the other 200 has
/// `a11y summary --all` one command away.
const HOOK_INTERACTIVE_LIMIT: usize = 15;

/// Hard cap on hint lines. Five is not a style preference: the hints are the
/// part an agent reads as instructions, and a list long enough to need
/// skimming is a list it will skim.
const MAX_HINTS: usize = 5;

// Text-renderer caps. The `page` block is already capped by
// `interactive_limit`, but headings and landmarks are not, so a link farm with
// 300 headings would blow the budget the AC `home_text_view_is_bounded` pins.
const TEXT_MAX_TABS: usize = 8;
const TEXT_MAX_LANDMARKS: usize = 6;
const TEXT_MAX_HEADINGS: usize = 10;
const TEXT_MAX_INTERACTIVE: usize = 25;

/// Did the caller actually type `--format`?
///
/// The home view's default is the text rendering, not the JSON envelope every
/// other command defaults to: bare `ff-rdp` exists to be *read*, by an agent
/// or a person, and a JSON dump is the thing they would then have to render
/// themselves. `--format json` (or any `--jq` filter) still gets the envelope.
///
/// `Cli::format` cannot answer this on its own — clap has already substituted
/// its `"json"` default by the time we see it — so this reads argv. Takes the
/// slice rather than calling `std::env::args()` so it is testable.
fn format_is_explicit(argv: &[String]) -> bool {
    let mut seen_separator = false;
    argv.iter().skip(1).any(|arg| {
        if arg == "--" {
            seen_separator = true;
        }
        !seen_separator && (arg == "--format" || arg.starts_with("--format="))
    })
}

/// URLs that mean "a browser is up but nothing is loaded". Offering
/// `click --ref` on one of these is offering nothing.
fn is_blank_url(url: &str) -> bool {
    matches!(
        url,
        "" | "about:blank" | "about:newtab" | "about:home" | "about:privatebrowsing"
    )
}

/// Replace a leading home directory with `~`, so the `bin:` line is short and
/// stays free of the operator's username.
fn collapse_home(path: &str) -> String {
    let Some(home) = dirs::home_dir() else {
        return path.to_owned();
    };
    let home = home.to_string_lossy();
    if home.is_empty() || !path.starts_with(home.as_ref()) {
        return path.to_owned();
    }
    let rest = &path[home.len()..];
    if rest.is_empty() {
        return "~".to_owned();
    }
    if rest.starts_with(['/', '\\']) {
        format!("~{rest}")
    } else {
        // `/home/jamesx` must not collapse against `/home/james`.
        path.to_owned()
    }
}

/// The state the hints are a function of, extracted from the assembled
/// `results` so [`hints_for`] can be unit-tested without a browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HintState<'a> {
    browser_reachable: bool,
    /// A tab exists whose URL is not `about:blank`-shaped.
    has_loaded_page: bool,
    /// The first `ref` in the page block, when refs were registered.
    first_ref: Option<&'a str>,
}

/// The `-> ff-rdp …` lines, chosen by state and capped at [`MAX_HINTS`].
///
/// The page-loaded set is generated from [`IDIOMS`] — the same table the
/// generated `SKILL.md` section teaches from — so a change to the idioms
/// reaches the session hook and the skill together.
///
/// Placeholders stay in `<>` (`<URL>`, `<text>`) so an agent can see at a
/// glance which part it must supply; `{ref}` is substituted with a ref the
/// page view actually minted, because `click --ref e3` is a command it can run
/// verbatim and `click --ref <ref>` is not. When no ref was minted (no daemon,
/// so no ref store) the `{ref}` idioms are replaced by the one command that
/// produces refs, rather than offered as handles that would not resolve.
fn hints_for(state: HintState<'_>) -> Vec<String> {
    let mut hints: Vec<String> = Vec::new();

    if !state.browser_reachable {
        hints.push(
            "ff-rdp launch --headless        # start Firefox with the debug port open".into(),
        );
        hints.push("ff-rdp launch --headless <URL>  # …and land on a page in one step".into());
        hints.push("ff-rdp doctor                   # if that fails: probe every layer".into());
        return hints;
    }

    if !state.has_loaded_page {
        hints.push("ff-rdp navigate <URL>           # blocks until the document commits".into());
        hints.push("ff-rdp tabs                     # list every open tab".into());
        return hints;
    }

    if let Some(r) = state.first_ref {
        for (_, _, command) in IDIOMS {
            hints.push(command.replace("{ref}", r));
        }
    } else {
        hints.push("ff-rdp a11y summary  # re-read the page and mint --ref handles".into());
        for (_, _, command) in IDIOMS {
            if !command.contains("{ref}") {
                hints.push((*command).to_owned());
            }
        }
    }
    hints.push("ff-rdp console --limit 20".into());
    hints.truncate(MAX_HINTS);
    hints
}

/// Probe the daemon registry. Never an error: "no daemon" is an answer.
fn daemon_block(host: &str, port: u16) -> Value {
    match find_running_daemon(host, port) {
        Ok(Some(info)) => json!({
            "running": true,
            "pid": info.pid,
            "proxy_port": info.proxy_port,
            "firefox_port": info.firefox_port,
        }),
        // A registry read that fails (permissions, corrupt JSON) is reported
        // as "no daemon" rather than propagated: the home view's contract is
        // that it always renders, and `doctor` is the command that explains
        // why a layer is unhappy.
        Ok(None) | Err(_) => json!({
            "running": false,
            "pid": Value::Null,
            "proxy_port": Value::Null,
            "firefox_port": port,
        }),
    }
}

/// Reachability + version + tab list, in one connection.
///
/// Returns `(browser_block, tabs)`. Failure to connect is the expected case
/// on a cold machine, so it produces `reachable: false` and an empty tab list
/// rather than an error.
fn browser_and_tabs(cli: &Cli) -> (Value, Vec<Value>) {
    let unreachable = |detail: Option<String>| {
        json!({
            "reachable": false,
            "firefox_version": Value::Null,
            "host": cli.host,
            "port": cli.port,
            "detail": detail,
        })
    };

    let mut connection =
        match RdpConnection::connect(&cli.host, cli.port, Duration::from_millis(cli.timeout)) {
            Ok(c) => c,
            Err(e) => return (unreachable(Some(e.to_string())), Vec::new()),
        };
    let firefox_version = connection.firefox_version();
    crate::connection_meta::remember_version(firefox_version);

    let tabs = match RootActor::list_tabs(connection.transport_mut()) {
        Ok(tabs) => serde_json::to_value(&tabs).unwrap_or(Value::Null),
        Err(e) => {
            // The greeting landed but `listTabs` did not: the browser *is*
            // reachable, and saying otherwise would send the agent to
            // `launch` when it should be looking at `doctor`.
            return (
                json!({
                    "reachable": true,
                    "firefox_version": firefox_version,
                    "host": cli.host,
                    "port": cli.port,
                    "detail": format!("listTabs failed: {e}"),
                }),
                Vec::new(),
            );
        }
    };

    let tabs = normalize_tabs(&tabs);
    (
        json!({
            "reachable": true,
            "firefox_version": firefox_version,
            "host": cli.host,
            "port": cli.port,
            "detail": Value::Null,
        }),
        tabs,
    )
}

/// Project the raw `listTabs` payload onto the four fields the home view
/// promises — `{index, title, url, selected}` — with a 1-based index matching
/// what `--tab N` accepts.
fn normalize_tabs(raw: &Value) -> Vec<Value> {
    let Some(items) = raw.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            json!({
                "index": i + 1,
                "title": tab.get("title").and_then(Value::as_str).unwrap_or_default(),
                "url": tab.get("url").and_then(Value::as_str).unwrap_or_default(),
                "selected": tab.get("selected").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect()
}

/// The tab the `page` block describes: the selected one, else the first.
fn focused_tab(tabs: &[Value]) -> Option<&Value> {
    tabs.iter()
        .find(|t| t.get("selected").and_then(Value::as_bool) == Some(true))
        .or_else(|| tabs.first())
}

/// Collect the accessibility view of the focused tab, or `None` when there is
/// nothing worth showing.
///
/// Routed through the daemon **only when one is already running**, so the refs
/// in the output are live handles without the home view ever spawning a
/// daemon. See the module docs for why that asymmetry is deliberate.
fn page_block(cli: &Cli, interactive_limit: usize, daemon_running: bool) -> Option<Value> {
    let mut ctx = if daemon_running && !cli.no_daemon {
        connect_and_get_target(cli).ok()?
    } else {
        connect_direct(cli).ok()?
    };
    let console_actor = ctx.target.console_actor.clone();
    let page = page_view::collect(
        &mut ctx,
        &console_actor,
        &CollectOptions {
            interactive_limit: Some(interactive_limit),
            wait_complete_ms: None,
        },
    )
    .ok()?;

    let mut view = page.view;
    if let Some(obj) = view.as_object_mut() {
        obj.insert("refs_registered".to_owned(), json!(page.refs_registered));
    }
    Some(view)
}

/// The first usable `ref` in a page block.
fn first_ref(page: Option<&Value>) -> Option<&str> {
    page?
        .get("interactive")?
        .as_array()?
        .iter()
        .find_map(|e| e.get("ref").and_then(Value::as_str))
}

/// Render the assembled `results` as the human/agent-facing text view.
///
/// Returns a `String` rather than printing so the AC
/// `home_text_view_is_bounded` can count its lines without capturing stdout.
fn render_text(results: &Value) -> String {
    let mut out = String::new();
    let get_str = |path: &[&str]| -> String {
        let mut cur = results;
        for key in path {
            match cur.get(*key) {
                Some(v) => cur = v,
                None => return String::new(),
            }
        }
        cur.as_str().unwrap_or_default().to_owned()
    };

    let version = get_str(&["version"]);
    let _ = writeln!(out, "ff-rdp {version} — {}", get_str(&["description"]));
    let _ = writeln!(out, "bin: {}", get_str(&["bin"]));

    // Daemon line.
    let daemon = &results["daemon"];
    if daemon.get("running").and_then(Value::as_bool) == Some(true) {
        let pid = daemon.get("pid").and_then(Value::as_u64).unwrap_or(0);
        let port = daemon
            .get("firefox_port")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let _ = writeln!(out, "daemon: running (pid {pid}, firefox port {port})");
    } else {
        out.push_str("daemon: not running\n");
    }

    // Browser line.
    let browser = &results["browser"];
    let host = browser.get("host").and_then(Value::as_str).unwrap_or("");
    let port = browser.get("port").and_then(Value::as_u64).unwrap_or(0);
    if browser.get("reachable").and_then(Value::as_bool) == Some(true) {
        let version = browser
            .get("firefox_version")
            .and_then(Value::as_u64)
            .map_or_else(|| "version unknown".to_owned(), |v| format!("Firefox {v}"));
        let _ = writeln!(out, "browser: reachable at {host}:{port} ({version})");
    } else {
        let _ = writeln!(out, "browser: not reachable at {host}:{port}");
    }

    // Tabs.
    if let Some(tabs) = results.get("tabs").and_then(Value::as_array)
        && !tabs.is_empty()
    {
        out.push_str("\nTABS\n");
        for tab in tabs.iter().take(TEXT_MAX_TABS) {
            let marker = if tab.get("selected").and_then(Value::as_bool) == Some(true) {
                "*"
            } else {
                " "
            };
            let index = tab.get("index").and_then(Value::as_u64).unwrap_or(0);
            let title = tab.get("title").and_then(Value::as_str).unwrap_or("");
            let url = tab.get("url").and_then(Value::as_str).unwrap_or("");
            let _ = writeln!(out, "  {marker} {index}  {title}  {url}");
        }
        if tabs.len() > TEXT_MAX_TABS {
            let _ = writeln!(out, "    … {} more tabs", tabs.len() - TEXT_MAX_TABS);
        }
    }

    // Page.
    if let Some(page) = results.get("page").filter(|p| !p.is_null()) {
        render_section(
            &mut out,
            page,
            "landmarks",
            "LANDMARKS",
            TEXT_MAX_LANDMARKS,
            |e| {
                let role = e.get("role").and_then(Value::as_str).unwrap_or("?");
                let label = e.get("label").and_then(Value::as_str).unwrap_or("");
                if label.is_empty() {
                    role.to_owned()
                } else {
                    format!("{role} \"{label}\"")
                }
            },
        );
        render_section(
            &mut out,
            page,
            "headings",
            "HEADINGS",
            TEXT_MAX_HEADINGS,
            |e| {
                let level = e.get("level").and_then(Value::as_u64).unwrap_or(0);
                let text = e.get("text").and_then(Value::as_str).unwrap_or("");
                format!("h{level} {text}")
            },
        );
        render_section(
            &mut out,
            page,
            "interactive",
            "INTERACTIVE",
            TEXT_MAX_INTERACTIVE,
            |e| {
                let r = e.get("ref").and_then(Value::as_str).unwrap_or("--");
                let role = e.get("role").and_then(Value::as_str).unwrap_or("?");
                let name = e.get("name").and_then(Value::as_str).unwrap_or("");
                format!("[{r}] {role} \"{name}\"")
            },
        );
    }

    // Hints.
    if let Some(hints) = results.get("hints").and_then(Value::as_array)
        && !hints.is_empty()
    {
        out.push('\n');
        for hint in hints {
            if let Some(h) = hint.as_str() {
                let _ = writeln!(out, "-> {h}");
            }
        }
    }

    out
}

/// Append one capped section of the page block to the text view.
fn render_section(
    out: &mut String,
    page: &Value,
    key: &str,
    heading: &str,
    max: usize,
    line: impl Fn(&Value) -> String,
) {
    let Some(entries) = page.get(key).and_then(Value::as_array) else {
        return;
    };
    if entries.is_empty() {
        return;
    }
    let _ = writeln!(out, "\n{heading}");
    for entry in entries.iter().take(max) {
        let _ = writeln!(out, "  {}", line(entry));
    }
    if entries.len() > max {
        let _ = writeln!(out, "    … {} more", entries.len() - max);
    }
}

/// Assemble the whole `results` payload.
///
/// Split out from [`run`] so the hint and text-rendering logic can be driven
/// from a fixture in tests without a Firefox anywhere.
fn build_results(
    bin: &str,
    version: &str,
    daemon: &Value,
    browser: &Value,
    tabs: &[Value],
    page: Option<Value>,
) -> Value {
    let browser_reachable = browser.get("reachable").and_then(Value::as_bool) == Some(true);
    let has_loaded_page = tabs
        .iter()
        .any(|t| !is_blank_url(t.get("url").and_then(Value::as_str).unwrap_or_default()));
    let hints = hints_for(HintState {
        browser_reachable,
        has_loaded_page,
        first_ref: first_ref(page.as_ref()),
    });

    json!({
        "bin": bin,
        "description": DESCRIPTION,
        "version": version,
        "daemon": daemon,
        "browser": browser,
        "tabs": tabs,
        "page": page.unwrap_or(Value::Null),
        "hints": hints,
    })
}

/// Bare `ff-rdp` (and the hidden `ff-rdp home`).
///
/// Always `Ok(())`: every layer that can be down is reported as state. The one
/// way this returns an error is an invalid `--format`/`--jq`, which is a
/// caller mistake rather than an observation about the machine.
pub fn run(cli: &Cli, args: &HomeArgs) -> Result<(), AppError> {
    let bin = std::env::current_exe().map_or_else(
        |_| "ff-rdp".to_owned(),
        |p| collapse_home(&p.to_string_lossy()),
    );

    let daemon = daemon_block(&cli.host, cli.port);
    let daemon_running = daemon.get("running").and_then(Value::as_bool) == Some(true);
    let (browser, tabs) = browser_and_tabs(cli);

    let interactive_limit = if args.hook {
        HOOK_INTERACTIVE_LIMIT
    } else {
        DEFAULT_INTERACTIVE_LIMIT
    };
    let page = if browser.get("reachable").and_then(Value::as_bool) == Some(true)
        && focused_tab(&tabs)
            .and_then(|t| t.get("url").and_then(Value::as_str))
            .is_some_and(|u| !is_blank_url(u))
    {
        page_block(cli, interactive_limit, daemon_running)
    } else {
        None
    };

    // The hook runs on every session; landmarks are the least actionable of
    // the three sections, so they are what goes first when the budget is tight.
    let page = match (args.hook, page) {
        (true, Some(mut view)) => {
            if let Some(obj) = view.as_object_mut() {
                obj.remove("landmarks");
            }
            Some(view)
        }
        (_, other) => other,
    };

    let results = build_results(
        &bin,
        env!("CARGO_PKG_VERSION"),
        &daemon,
        &browser,
        &tabs,
        page,
    );

    let invocation: Vec<String> = std::env::args().collect();
    let wants_envelope =
        cli.jq.is_some() || (format_is_explicit(&invocation) && cli.format != "text");
    if !wants_envelope {
        print!("{}", render_text(&results));
        return Ok(());
    }

    // iter-212 design note: the home view is the *one* JSON payload that
    // carries `hints`. It is the orientation surface and the hook consumes it,
    // so a caller reading `results.hints` through `--jq` is the intended use —
    // unlike every other command, where hints stay out of JSON because agents
    // read those through pipes.
    let envelope = output::envelope(&results, 1, &json!({}));
    OutputPipeline::from_cli(cli)?.finalize(&envelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_browser() -> Value {
        json!({"reachable": false, "firefox_version": Value::Null, "host": "localhost", "port": 6000, "detail": "connection refused"})
    }

    fn live_browser() -> Value {
        json!({"reachable": true, "firefox_version": 143, "host": "localhost", "port": 6000, "detail": Value::Null})
    }

    fn no_daemon() -> Value {
        json!({"running": false, "pid": Value::Null, "proxy_port": Value::Null, "firefox_port": 6000})
    }

    fn tab(index: u64, url: &str, selected: bool) -> Value {
        json!({"index": index, "title": "T", "url": url, "selected": selected})
    }

    /// AC `home_without_browser_exits_zero_and_names_launch` (the pure half:
    /// the e2e sibling in `tests/e2e/home.rs` pins the exit code).
    #[test]
    fn unit_212_home_without_browser_reports_state_and_names_launch() {
        let results = build_results(
            "~/.cargo/bin/ff-rdp",
            "0.3.0",
            &no_daemon(),
            &no_browser(),
            &[],
            None,
        );
        assert_eq!(results["browser"]["reachable"], json!(false));
        let hints = results["hints"].as_array().expect("hints array");
        assert!(
            hints
                .iter()
                .any(|h| h.as_str().is_some_and(|s| s.starts_with("ff-rdp launch"))),
            "a machine with no browser must be told to launch one: {hints:?}"
        );
        assert!(
            hints.len() <= MAX_HINTS,
            "at most {MAX_HINTS} hints, got {}",
            hints.len()
        );
    }

    /// A reachable browser sitting on `about:blank` is not a page — offering
    /// `click --ref` there would be offering a handle that does not exist.
    #[test]
    fn unit_212_blank_tab_asks_for_a_navigate_not_a_click() {
        let results = build_results(
            "ff-rdp",
            "0.3.0",
            &no_daemon(),
            &live_browser(),
            &[tab(1, "about:blank", true)],
            None,
        );
        let hints: Vec<&str> = results["hints"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(
            hints.iter().any(|h| h.starts_with("ff-rdp navigate")),
            "{hints:?}"
        );
        assert!(!hints.iter().any(|h| h.contains("--ref")), "{hints:?}");
    }

    /// With a page loaded and refs registered, the first hint is a command the
    /// agent can paste verbatim — a real ref, not the `<ref>` placeholder.
    #[test]
    fn unit_212_loaded_page_hints_name_a_concrete_ref() {
        let page = json!({
            "headings": [{"level": 1, "text": "Example"}],
            "interactive": [{"role": "link", "name": "More", "ref": "e3"}],
            "refs_registered": true,
        });
        let results = build_results(
            "ff-rdp",
            "0.3.0",
            &no_daemon(),
            &live_browser(),
            &[tab(1, "https://example.com/", true)],
            Some(page),
        );
        let hints: Vec<&str> = results["hints"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(
            hints[0], "ff-rdp click --ref e3",
            "expected a runnable command naming a concrete ref"
        );
        assert!(hints.len() <= MAX_HINTS, "{hints:?}");
        // Placeholders for the values only the caller can supply stay in <>.
        assert!(
            hints.iter().any(|h| h.contains("--query \"<text>\"")),
            "{hints:?}"
        );
    }

    /// Without a daemon there are no refs, so the hint has to be the command
    /// that produces them rather than one that consumes them.
    #[test]
    fn unit_212_loaded_page_without_refs_points_at_a11y_summary() {
        let page = json!({
            "headings": [{"level": 1, "text": "Example"}],
            "interactive": [{"role": "link", "name": "More"}],
            "refs_registered": false,
        });
        let results = build_results(
            "ff-rdp",
            "0.3.0",
            &no_daemon(),
            &live_browser(),
            &[tab(1, "https://example.com/", true)],
            Some(page),
        );
        let hints: Vec<&str> = results["hints"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(hints[0].starts_with("ff-rdp a11y summary"), "{hints:?}");
        assert!(
            !hints.iter().any(|h| h.contains("click --ref")),
            "an inert ref handle must never be offered as a command: {hints:?}"
        );
    }

    /// AC `home_text_view_is_bounded`: a fixture page with 300 links must
    /// still render in under 80 lines. The `page` block's own cap does most of
    /// the work; this pins that headings and landmarks are capped too.
    #[test]
    fn unit_212_home_text_view_is_bounded_on_a_300_link_page() {
        let interactive: Vec<Value> = (0..300)
            .map(|i| json!({"role": "link", "name": format!("link {i}"), "ref": format!("e{i}")}))
            .collect();
        let headings: Vec<Value> = (0..120)
            .map(|i| json!({"level": 2, "text": format!("heading {i}")}))
            .collect();
        let landmarks: Vec<Value> = (0..40)
            .map(|i| json!({"role": "region", "label": format!("region {i}"), "tag": "section"}))
            .collect();
        let tabs: Vec<Value> = (0..30)
            .map(|i| tab(i + 1, &format!("https://example.com/{i}"), i == 0))
            .collect();
        let results = build_results(
            "~/.cargo/bin/ff-rdp",
            "0.3.0",
            &no_daemon(),
            &live_browser(),
            &tabs,
            Some(json!({
                "landmarks": landmarks,
                "headings": headings,
                "interactive": interactive,
                "refs_registered": true,
            })),
        );
        let text = render_text(&results);
        let lines = text.lines().count();
        assert!(
            lines <= 80,
            "text view must stay under 80 lines, got {lines}:\n{text}"
        );
        // …and it must still be useful: the identity line, the tab, a ref, and
        // the hints all survive the cap.
        assert!(text.starts_with("ff-rdp 0.3.0 — "), "{text}");
        assert!(text.contains("TABS"), "{text}");
        assert!(text.contains("[e0]"), "{text}");
        assert!(text.contains("-> ff-rdp click --ref e0"), "{text}");
        assert!(text.contains("… 275 more"), "{text}");
    }

    /// The no-browser text view is the one an agent sees most often on a cold
    /// machine; it must name the binary, say plainly that nothing is up, and
    /// end in runnable commands.
    #[test]
    fn unit_212_text_view_without_a_browser_is_self_explanatory() {
        let results = build_results(
            "~/.cargo/bin/ff-rdp",
            "0.3.0",
            &no_daemon(),
            &no_browser(),
            &[],
            None,
        );
        let text = render_text(&results);
        assert!(text.contains("bin: ~/.cargo/bin/ff-rdp"), "{text}");
        assert!(text.contains("daemon: not running"), "{text}");
        assert!(
            text.contains("browser: not reachable at localhost:6000"),
            "{text}"
        );
        assert!(text.contains("-> ff-rdp launch --headless"), "{text}");
        assert!(
            !text.contains("TABS"),
            "no tabs section when there are none: {text}"
        );
    }

    /// Bare `ff-rdp` renders text; an explicit `--format json` (or any `--jq`)
    /// still produces the envelope. The `--` separator ends flag scanning, so
    /// a literal `--format` in a trailing positional is not mistaken for one.
    #[test]
    fn unit_212_format_is_explicit_only_when_typed() {
        let argv = |args: &[&str]| -> Vec<String> {
            std::iter::once("ff-rdp")
                .chain(args.iter().copied())
                .map(ToOwned::to_owned)
                .collect()
        };
        assert!(!format_is_explicit(&argv(&[])));
        assert!(!format_is_explicit(&argv(&["--jq", ".results"])));
        assert!(format_is_explicit(&argv(&["--format", "json"])));
        assert!(format_is_explicit(&argv(&["--format=text"])));
        assert!(!format_is_explicit(&argv(&["--", "--format", "json"])));
    }

    #[test]
    fn unit_212_collapse_home_only_matches_a_path_boundary() {
        let Some(home) = dirs::home_dir() else {
            return; // no home dir on this runner — nothing to assert
        };
        let home = home.to_string_lossy().into_owned();
        let inside = format!("{home}/.cargo/bin/ff-rdp");
        assert!(collapse_home(&inside).starts_with('~'), "{inside}");
        assert_eq!(collapse_home(&home), "~");
        // A sibling directory that merely shares the prefix must not collapse.
        let sibling = format!("{home}x/bin/ff-rdp");
        assert_eq!(collapse_home(&sibling), sibling);
        assert_eq!(
            collapse_home("/usr/local/bin/ff-rdp"),
            "/usr/local/bin/ff-rdp"
        );
    }

    #[test]
    fn unit_212_blank_url_recognises_the_empty_states() {
        for url in ["", "about:blank", "about:newtab", "about:home"] {
            assert!(is_blank_url(url), "{url} should count as blank");
        }
        assert!(!is_blank_url("https://example.com/"));
        assert!(!is_blank_url("file:///tmp/page.html"));
    }

    #[test]
    fn unit_212_focused_tab_prefers_the_selected_one() {
        let tabs = vec![
            tab(1, "https://a.example/", false),
            tab(2, "https://b.example/", true),
        ];
        assert_eq!(focused_tab(&tabs).expect("a tab")["index"], json!(2));
        let unselected = vec![tab(1, "https://a.example/", false)];
        assert_eq!(focused_tab(&unselected).expect("a tab")["index"], json!(1));
        assert!(focused_tab(&[]).is_none());
    }

    #[test]
    fn unit_212_normalize_tabs_numbers_from_one_and_keeps_four_fields() {
        let raw = json!([
            {"title": "A", "url": "https://a.example/", "selected": false, "actor": "server1.conn0.x"},
            {"title": "B", "url": "https://b.example/", "selected": true, "actor": "server1.conn0.y"},
        ]);
        let tabs = normalize_tabs(&raw);
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0]["index"], json!(1));
        assert_eq!(tabs[1]["selected"], json!(true));
        assert!(
            tabs[0].get("actor").is_none(),
            "the actor id is not part of the home view"
        );
        assert!(normalize_tabs(&json!({})).is_empty());
    }

    /// The description an agent reads at session start comes from the same
    /// constant the generated skill section opens with (Theme C).
    #[test]
    fn unit_212_description_is_the_shared_one() {
        let results = build_results("ff-rdp", "0.3.0", &no_daemon(), &no_browser(), &[], None);
        assert_eq!(results["description"], json!(DESCRIPTION));
        assert!(
            super::super::skill_doc::generate_block().contains(DESCRIPTION),
            "the home view and the skill must quote the same description"
        );
    }

    /// The page-loaded hints are drawn from the shared `IDIOMS` table, so a
    /// change there reaches both surfaces.
    #[test]
    fn unit_212_hints_cover_the_shared_idioms() {
        let hints = hints_for(HintState {
            browser_reachable: true,
            has_loaded_page: true,
            first_ref: Some("e1"),
        });
        let joined = hints.join("\n");
        for (_, _, command) in IDIOMS {
            let expected = command.replace("{ref}", "e1");
            assert!(
                joined.contains(&expected),
                "idiom command {expected:?} missing from hints:\n{joined}"
            );
        }
    }

    /// A code-review catch (iter-212): the `--with-page` idiom's `command`
    /// field was a copy of the `--query` idiom's shape and never actually
    /// contained `--with-page` — so the session hook demonstrated `--query`
    /// twice and `--with-page` never, and `unit_212_hints_cover_the_shared_idioms`
    /// above did not catch it because it derives its expectation from the same
    /// table. This pins the missing half: each idiom's own flag must appear in
    /// its own example command.
    #[test]
    fn unit_212_each_idiom_command_demonstrates_its_own_flag() {
        for (syntax, why, command) in IDIOMS {
            let flag = syntax
                .split_whitespace()
                .next()
                .expect("syntax has a leading flag token");
            assert!(
                command.contains(flag),
                "idiom {syntax:?} ({why:?}) has command {command:?}, which never uses {flag}"
            );
        }
    }
}
