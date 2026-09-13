//! Fact links keep their source anchors across query filtering and ref registration.
//! daemon-parity: live_255_fact_links_direct verifies the same source rows without handles.

use std::collections::HashMap;
use std::process::{Command, Output};

use serde_json::Value;

use crate::common::{
    FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_network_tests_enabled,
    live_tests_enabled,
};

fn run(ff: &LiveFirefox, direct: bool, args: &[&str]) -> Output {
    let mut command = Command::new(ff_rdp_bin());
    command.args([
        "--host",
        "127.0.0.1",
        "--port",
        &ff.port().to_string(),
        "--timeout",
        "20000",
    ]);
    if direct {
        command.arg("--no-daemon");
    }
    let output = command.args(args).output().expect("run checkout binary");
    assert!(output.status.success(), "{args:?}: {output:?}");
    output
}

fn json(ff: &LiveFirefox, direct: bool, args: &[&str]) -> Value {
    serde_json::from_slice(&run(ff, direct, args).stdout).expect("JSON envelope")
}

fn exercise(direct: bool) {
    assert!(live_tests_enabled());
    let ff = LiveFirefox::headless_on_random_port();
    if !direct {
        assert!(ff.with_daemon().is_some());
    }
    let server = FixtureServer::start(HashMap::from([
        ("/".into(), FixtureRoute::html("<!doctype html><title>Fact links</title><main><article>
            <h1>Source facts</h1><table class='infobox'>
            <tr><th>Developer</th><td><a id='first' href='/first'>First</a> and <a href='/second'>Second</a></td></tr>
            <tr><th>Stable release</th><td>3.14</td></tr></table>
            <dl><dt>Influenced by</dt><dd><a href='/first'>Alpha</a></dd><dd><a href='/second'>Beta</a></dd></dl>
            <a itemprop='publisher' href='/second'>Publisher</a>
            <p>A factual article for checking the structured information from all supported source shapes.</p>
            </article></main>")),
        ("/first".into(), FixtureRoute::html("<!doctype html><title>First destination</title><h1>First destination</h1>")),
        ("/second".into(), FixtureRoute::html("<!doctype html><title>Second destination</title><h1>Second destination</h1>")),
    ])).expect("fixture server");
    let all = json(
        &ff,
        direct,
        &["navigate", &server.base_url(), "--with-page"],
    );
    let facts = all["results"]["page"]["facts"]
        .as_array()
        .expect("facts array");
    let pairs: Vec<_> = facts
        .iter()
        .map(|fact| {
            (
                fact["key"].as_str().unwrap(),
                fact["value"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        pairs,
        [
            ("Developer", "First and Second"),
            ("Stable release", "3.14"),
            ("Influenced by", "Alpha; Beta"),
            ("publisher", "Publisher")
        ]
    );
    assert!(facts[1].get("links").is_none());
    assert_eq!(facts[2]["links"].as_array().unwrap().len(), 2);
    assert_eq!(facts[3]["links"][0]["name"], "Publisher");
    assert_eq!(facts[3]["links"][0]["href"], "/second");
    let filtered = json(
        &ff,
        direct,
        &[
            "navigate",
            &server.base_url(),
            "--with-page",
            "--query",
            "Developer",
        ],
    );
    assert_eq!(
        filtered["results"]["page"]["interactive"],
        serde_json::json!([])
    );
    let fact = &filtered["results"]["page"]["facts"][0];
    let links = fact["links"].as_array().unwrap();
    assert_eq!(links.len(), 2);
    for link in links {
        assert!(link.get("__resolver").is_none(), "{filtered}");
        assert_eq!(link.get("ref").is_some(), !direct, "{filtered}");
    }
    let text = run(
        &ff,
        direct,
        &[
            "--format",
            "text",
            "navigate",
            &server.base_url(),
            "--with-page",
            "--query",
            "Developer",
        ],
    );
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.contains("Developer: First and Second"), "{text}");
    assert!(
        text.contains("First → /first") && text.contains("Second → /second"),
        "{text}"
    );
    assert_eq!(text.contains("[e"), !direct, "{text}");
    if !direct {
        // Recollect after the text command navigated: a prior generation's ref
        // is intentionally invalid, even when the URL happens to be identical.
        let fresh = json(
            &ff,
            direct,
            &[
                "navigate",
                &server.base_url(),
                "--with-page",
                "--query",
                "Developer",
            ],
        );
        let id = fresh["results"]["page"]["facts"][0]["links"][1]["ref"]
            .as_str()
            .unwrap();
        let clicked = json(&ff, direct, &["click", "--ref", id, "--with-page"]);
        assert!(
            clicked.to_string().contains("Second destination"),
            "{clicked}"
        );
    }
    let miss = run(
        &ff,
        direct,
        &[
            "--format",
            "text",
            "navigate",
            &server.base_url(),
            "--with-page",
            "--query",
            "nonexistent-key",
            "--page-chars",
            "0",
        ],
    );
    let miss = String::from_utf8(miss.stdout).unwrap();
    assert!(miss.contains("Collected fact keys considered (collection truncated: false): Developer, Stable release, Influenced by, publisher"), "{miss}");
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_255_fact_links_daemon() {
    exercise(false);
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_255_fact_links_direct() {
    exercise(true);
}

fn exercise_link_budgets(direct: bool) {
    assert!(live_tests_enabled());
    let ff = LiveFirefox::headless_on_random_port();
    if !direct {
        assert!(ff.with_daemon().is_some());
    }
    let images = |count: usize| "<a href='/destination'><img alt='icon'></a>".repeat(count);
    let counts = format!(
        "<table class='infobox'><tr><th>Budget exact</th><td>Value{}</td></tr>
        <tr><th>Budget over</th><td>Value{}</td></tr></table>
        <dl><dt>Budget sources</dt><dd>One{}</dd><dd>Two{}</dd></dl>
        <div itemprop='Budget microdata' content='Short'>{}</div>
        <div itemprop='Budget global' content='Short'>{}</div>",
        images(8),
        images(9),
        images(5),
        images(5),
        images(8),
        images(1)
    );
    let sized = |key: &str, name: &str, href: &str, id: &str| {
        format!(
            "<a style='display:block;width:30px;height:30px' itemprop='Budget {key}' content='Short' id='{id}' href='{href}'>{name}</a>"
        )
    };
    let fields = [
        sized("name exact", &"n".repeat(256), "/destination", "n"),
        sized("name over", &"n".repeat(257), "/destination", "no"),
        sized("href exact", "", &format!("/{}", "h".repeat(2047)), "h"),
        sized("href over", "", &format!("/{}", "h".repeat(2048)), "ho"),
        sized("selector exact", "", "/destination", &"s".repeat(2047)),
        sized("selector over", "", "/destination", &"s".repeat(2048)),
        sized("after omitted", "Working", "/destination", "working"),
    ]
    .join("");
    // Four links cost exactly 8192 UTF-16 units including private selectors:
    // empty name + one-character href + 2047-character #id, across two rows.
    let budget_anchor =
        |index: usize| format!("<a id='a{index}{}' href='/'><img></a>", "b".repeat(2044));
    let text_budget = format!(
        "<div itemprop='Budget first' content='Short'>{}{}</div>
        <div itemprop='Budget exact' content='Short'>{}{}</div>
        <div itemprop='Budget over' content='Short'><a href='/'><img></a></div>",
        budget_anchor(0),
        budget_anchor(1),
        budget_anchor(2),
        budget_anchor(3)
    );
    let server = FixtureServer::start(HashMap::from([
        ("/counts".into(), FixtureRoute::html(&counts)),
        ("/fields".into(), FixtureRoute::html(&fields)),
        ("/text-budget".into(), FixtureRoute::html(&text_budget)),
        (
            "/destination".into(),
            FixtureRoute::html("<title>Budget destination</title><h1>Budget destination</h1>"),
        ),
    ]))
    .expect("budget fixtures");
    let collect = |path: &str| {
        json(
            &ff,
            direct,
            &[
                "navigate",
                &format!("{}{path}", server.base_url()),
                "--with-page",
                "--query",
                "Budget",
                "--page-chars",
                "0",
            ],
        )
    };
    let link_count = |fact: &Value| fact["links"].as_array().map_or(0, Vec::len);
    let counts_view = collect("/counts");
    let facts = counts_view["results"]["page"]["facts"].as_array().unwrap();
    assert_eq!(
        facts.iter().map(link_count).collect::<Vec<_>>(),
        [8, 8, 8, 8, 0],
        "{counts_view}"
    );
    assert_eq!(
        facts
            .iter()
            .map(|f| f["links_truncated"] == true)
            .collect::<Vec<_>>(),
        [false, true, true, false, true]
    );
    assert_eq!(facts[2]["value"], "One; Two");
    assert!(
        counts_view["results"]["page"]["interactive"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let text_view = collect("/text-budget");
    let facts = text_view["results"]["page"]["facts"].as_array().unwrap();
    assert_eq!(
        facts.iter().map(link_count).collect::<Vec<_>>(),
        [2, 2, 0],
        "{text_view}"
    );
    assert!(facts[1].get("links_truncated").is_none());
    assert_eq!(facts[2]["links_truncated"], true);
    let fields_view = collect("/fields");
    let facts = fields_view["results"]["page"]["facts"].as_array().unwrap();
    assert_eq!(
        facts.iter().map(link_count).collect::<Vec<_>>(),
        [1, 0, 1, 0, 1, 0, 1],
        "{fields_view}"
    );
    for (index, fact) in facts.iter().enumerate() {
        assert_eq!(fact["value"], "Short");
        assert_eq!(fact["links_truncated"] == true, [1, 3, 5].contains(&index));
        for link in fact["links"].as_array().into_iter().flatten() {
            assert_eq!(link.get("ref").is_some(), !direct);
            assert!(link.get("__resolver").is_none());
        }
    }
    assert_eq!(facts[0]["links"][0]["name"], "n".repeat(256));
    assert_eq!(
        facts[2]["links"][0]["href"],
        format!("/{}", "h".repeat(2047))
    );
    if !direct {
        let id = facts[4]["links"][0]["ref"].as_str().unwrap();
        let clicked = json(&ff, false, &["click", "--ref", id, "--with-page"]);
        assert!(
            clicked.to_string().contains("Budget destination"),
            "{clicked}"
        );
    }
    let text = run(
        &ff,
        direct,
        &[
            "--format",
            "text",
            "navigate",
            &format!("{}/fields", server.base_url()),
            "--with-page",
            "--query",
            "Budget",
            "--page-chars",
            "0",
        ],
    );
    let text = String::from_utf8(text.stdout).unwrap();
    assert_eq!(
        text.matches("Some fact links omitted (collection limits).")
            .count(),
        3,
        "{text}"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_255_fact_link_budgets_daemon() {
    exercise_link_budgets(false);
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_255_fact_link_budgets_direct() {
    exercise_link_budgets(true);
}

#[test]
#[ignore = "requires live Firefox and network — set FF_RDP_LIVE_TESTS=1 and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_255_python_fact_click_reaches_psf_formation() {
    assert!(live_tests_enabled() && live_network_tests_enabled());
    let ff = LiveFirefox::headless_on_random_port();
    assert!(ff.with_daemon().is_some());
    let python = json(
        &ff,
        false,
        &[
            "navigate",
            "https://en.wikipedia.org/wiki/Python_(programming_language)",
            "--with-page",
            "--query",
            "Developer",
        ],
    );
    let facts = python["results"]["page"]["facts"]
        .as_array()
        .expect("Python facts");
    let developer = facts
        .iter()
        .find(|fact| fact["key"].as_str() == Some("Developer"))
        .expect("Developer fact");
    let link = developer["links"]
        .as_array()
        .expect("fact links")
        .iter()
        .find(|link| {
            link["href"]
                .as_str()
                .is_some_and(|href| href.contains("/wiki/Python_Software_Foundation"))
        })
        .expect("PSF link");
    let id = link["ref"].as_str().expect("clickable PSF handle");
    let psf = json(
        &ff,
        false,
        &[
            "click",
            "--ref",
            id,
            "--with-page",
            "--query",
            "founded formed",
        ],
    );
    assert!(
        psf["results"]["page"]["facts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|fact| fact["key"].as_str() == Some("Formation")),
        "{psf}"
    );
    let url = json(&ff, false, &["eval", "location.href"]);
    assert!(
        url.to_string()
            .contains("https://en.wikipedia.org/wiki/Python_Software_Foundation"),
        "{url}"
    );
    eprintln!("ITER255_REAL_PYTHON {python}\nITER255_REAL_PSF {psf}\nITER255_REAL_URL {url}");
}
