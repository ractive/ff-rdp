//! The actual iteration212 home/ref/click/home assertions, shared by its
//! live scenario and the synchronized mock-RDP regression. The runner invokes
//! real CLI commands in both cases; this helper supplies no browser outcomes.
use serde_json::Value;

pub fn check(url: &str, mut run: impl FnMut(&[&str]) -> Value) {
    let home = run(&["--format", "json"]);
    let results = &home["results"];

    assert_eq!(
        results["browser"]["reachable"],
        Value::Bool(true),
        "a live Firefox must read as reachable: {home}"
    );
    let tabs = results["tabs"].as_array().expect("tabs array");
    assert!(!tabs.is_empty(), "the loaded tab must be listed: {home}");
    let listed = tabs[0]["url"]
        .as_str()
        .unwrap_or_else(|| panic!("tab url must be a string: {home}"));
    assert!(
        listed.starts_with(url),
        "tabs[0].url must name the page that is loaded ({url}), got {listed}: {home}"
    );
    assert_eq!(
        tabs[0]["index"],
        Value::from(1),
        "tab indices are 1-based, matching `--tab N`: {home}"
    );

    let page = &results["page"];
    assert!(
        !page.is_null(),
        "a loaded page must produce a page block: {home}"
    );
    let headings = page["headings"].as_array().expect("headings array");
    assert!(
        headings
            .iter()
            .any(|h| h["text"].as_str() == Some("Ambient context")),
        "the page block must describe the loaded document: {home}"
    );

    let refs_registered = page["refs_registered"].as_bool().unwrap_or(false);
    assert!(
        refs_registered,
        "on the daemon route the page block must carry live refs: {home}"
    );
    let first_ref = page["interactive"]
        .as_array()
        .and_then(|entries| entries.first())
        .and_then(|entry| entry["ref"].as_str())
        .unwrap_or_else(|| panic!("the first interactive entry must carry a ref: {home}"));

    // The ref is only useful if `click` accepts it — the AC's actual claim.
    // Bare click waits for element readiness, not destination readiness.
    // Request the documented act-and-see wait before inspecting home again.
    let clicked = run(&["click", "--ref", first_ref, "--with-page"]);
    assert!(
        clicked["results"]["clicked"] != Value::Bool(false),
        "click --ref {first_ref} must act on the element the home view named: {clicked}"
    );

    assert_eq!(
        clicked["meta"]["page_ready"],
        Value::Bool(true),
        "the destination must be ready before the subsequent home observation: {clicked}"
    );
    assert!(
        clicked["results"]["page"]["headings"]
            .as_array()
            .is_some_and(|headings| headings.iter().any(|h| h["text"] == "Arrived")),
        "click --ref must reach the fixture destination before reading home: {clicked}"
    );

    let after = run(&["--format", "json"]);
    let after_url = after["results"]["tabs"][0]["url"]
        .as_str()
        .unwrap_or_default();
    assert!(
        after_url.ends_with("/clicked"),
        "the click must have followed the link the ref pointed at, got {after_url}: {after}"
    );

    // …and the hints an agent reads name that same ref, verbatim.
    let hints = results["hints"].as_array().expect("hints array");
    assert!(
        hints
            .iter()
            .filter_map(Value::as_str)
            .any(|h| h.contains(&format!("--ref {first_ref}"))),
        "the hints must offer the ref the page block minted: {hints:?}"
    );
}
