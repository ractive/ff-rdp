//! Strict oracle for the five fixed 275 documents; not a generic CMP adapter.
use regex::Regex;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::LazyLock;

type Check<T = ()> = Result<T, String>;

macro_rules! require {
    ($condition:expr, $message:expr) => {
        if !$condition {
            return Err($message.to_string());
        }
    };
}

#[derive(Debug, PartialEq)]
pub struct Expectation {
    pub order: Vec<&'static str>,
    pub selectors: Vec<&'static str>,
    pub accepted: Option<&'static str>,
    pub classification: &'static str,
    pub exit: i32,
    pub status: &'static str,
    pub cmp: Option<&'static str>,
    pub error: Option<&'static str>,
}

impl Expectation {
    pub fn record(&self) -> Value {
        json!({"order":self.order,"selectors":self.selectors,"accepted":self.accepted,
            "classification":self.classification,"exit":self.exit,"status":self.status,
            "cmp":self.cmp,"error":self.error})
    }
}

fn case_controls(case: &str) -> Check<[bool; 2]> {
    match case {
        "later" => Ok([false, true]),
        "first" | "bbc.com-native" => Ok([true, true]),
        "all-miss" | "no-cmp" => Ok([false, false]),
        _ => Err(format!("unknown fixed fixture case {case}")),
    }
}

fn frame_url(base: &str, case: &str, id: &str) -> String {
    let prefix = if case == "no-cmp" {
        "unrecognized"
    } else {
        "sourcepoint"
    };
    format!("{base}/{case}/{prefix}-{id}")
}

fn positive(value: &Value) -> bool {
    value.as_f64().is_some_and(|n| n.is_finite() && n > 0.0)
}

fn geometry(sample: &Value, present: bool) -> Check {
    require!(sample["present"] == present, "control presence");
    if present {
        require!(
            positive(&sample["w"]) && positive(&sample["h"]),
            "positive control geometry"
        );
    } else {
        require!(
            sample.get("w") == Some(&Value::Null) && sample.get("h") == Some(&Value::Null),
            "absent control geometry"
        );
    }
    Ok(())
}

fn signature(sample: &Value, id: &str, href: &str, token: &str) -> Check {
    require!(
        sample["id"] == id && sample["kind"] == "readback",
        "document inventory"
    );
    require!(
        sample["href"] == href && sample["documentURI"] == href,
        "document URL"
    );
    require!(
        sample["token"] == token && sample["readyState"] == "complete",
        "ready document token"
    );
    require!(positive(&sample["epoch"]), "document epoch");
    require!(sample["count"] == 0, "initial counter");
    Ok(())
}

pub fn qualify_before(before: &Value, case: &str, base: &str) -> Check {
    let accepts = case_controls(case)?;
    let top = format!("{base}/{case}");
    require!(
        before["href"] == top && before["token"] == case,
        "top identity"
    );
    require!(
        before["readyState"] == "complete" && positive(&before["epoch"]),
        "ready top document"
    );
    require!(before["proof"] == json!([]), "empty initial proof");
    signature(&before["native"], "native", &top, case)?;
    require!(
        before["native"]["epoch"] == before["epoch"],
        "native top epoch"
    );
    geometry(&before["native"], case == "bbc.com-native")?;
    let frames = before["frames"].as_array().ok_or("initial frames")?;
    require!(frames.len() == 2, "initial frame count");
    for ((frame, id), accepts) in frames.iter().zip(["first", "second"]).zip(accepts) {
        signature(
            frame,
            id,
            &frame_url(base, case, id),
            &format!("{case}-{id}"),
        )?;
        require!(
            frame["controls"]
                == json!([{"label":if accepts { "Accept all" } else { "Reject only" },"disabled":false}]),
            "fixed frame controls"
        );
        geometry(frame, accepts)?;
    }
    Ok(())
}

// Deliberately fixed Debug grammar. No substring URL guesses, quoted escapes,
// arbitrary titles or partially consumed records are accepted as fixture proof.
static BEGIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
    r"^[0-9TZ:.+-]+ DEBUG ff_rdp_cli::frame_targets: FRAME_TARGETS_BEGIN pid=([0-9]+) via_daemon=(true|false)$"
).unwrap()
});
static END: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
    r"^[0-9TZ:.+-]+ DEBUG ff_rdp_cli::frame_targets: FRAME_TARGETS_END pid=([0-9]+) via_daemon=(true|false) elapsed_ns=([0-9]+) result=Ok\(\[(.*)\]\)$"
).unwrap()
});
static TARGET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
    r#"^TargetEvent \{ actor: ActorId\("(?P<actor>[A-Za-z0-9_./:-]+)"\), "#,
    r#"url: Some\("(?P<url>[A-Za-z0-9_./:-]+)"\), title: Some\(""\), "#,
    r#"target_type: "frame", is_top_level: (?P<top>true|false), "#,
    r#"console_actor: Some\(ActorId\("(?P<console>[A-Za-z0-9_./:-]+)"\)\), "#,
    r#"inspector_actor: (?:None|Some\(ActorId\("[A-Za-z0-9_./:-]+"\)\)), "#,
    r"browsing_context_id: (?:None|Some\((?P<context>[0-9]+)\)), process_id: (?:None|Some\((?P<process>[0-9]+)\)) \}"
)).unwrap()
});

fn returned_order(logs: &str, base: &str, case: &str, daemon: bool) -> Check<Vec<&'static str>> {
    let begins: Vec<_> = logs
        .lines()
        .filter(|l| l.contains("FRAME_TARGETS_BEGIN"))
        .collect();
    let ends: Vec<_> = logs
        .lines()
        .filter(|l| l.contains("FRAME_TARGETS_END"))
        .collect();
    require!(
        begins.len() == 1 && ends.len() == 1,
        "one actual enumeration BEGIN/END"
    );
    let begin = BEGIN.captures(begins[0]).ok_or("invalid BEGIN grammar")?;
    let end = END.captures(ends[0]).ok_or("invalid END grammar")?;
    require!(
        begin[1] == end[1] && begin[1].parse::<u32>().is_ok_and(|pid| pid > 0),
        "matching positive invocation pid"
    );
    let route = if daemon { "true" } else { "false" };
    require!(
        &begin[2] == route && &end[2] == route,
        "actual invocation route"
    );
    require!(end[3].parse::<u128>().is_ok(), "elapsed integer");
    require!(
        logs.find(begins[0]) < logs.find(ends[0]),
        "BEGIN before END"
    );
    let urls = [
        format!("{base}/{case}"),
        frame_url(base, case, "first"),
        frame_url(base, case, "second"),
    ];
    let mut seen = HashSet::new();
    let mut identities = HashSet::new();
    let mut order = Vec::new();
    let mut remaining = &end[4];
    loop {
        let target = TARGET
            .captures(remaining)
            .ok_or("invalid complete TargetEvent grammar")?;
        let index = urls
            .iter()
            .position(|url| url == &target["url"])
            .ok_or("unexpected target URL")?;
        require!(seen.insert(index), "duplicate target URL");
        require!(
            (&target["top"] == "true") == (index == 0),
            "target top flag"
        );
        require!(
            identities.insert(target["actor"].to_string())
                && identities.insert(target["console"].to_string()),
            "unique target/console identities"
        );
        for field in ["context", "process"] {
            if let Some(value) = target.name(field) {
                require!(
                    value.as_str().parse::<u64>().is_ok(),
                    "valid optional numeric target field"
                );
            }
        }
        if index != 0 {
            order.push(if index == 1 { "first" } else { "second" });
        }
        remaining = &remaining[target.get(0).unwrap().end()..];
        if remaining.is_empty() {
            break;
        }
        remaining = remaining
            .strip_prefix(", ")
            .ok_or("target record separator")?;
    }
    require!(
        seen.len() == 3 && order.len() == 2,
        "exact fixture target inventory"
    );
    Ok(order)
}

/// Inputs deliberately exclude action stdout/status, selector rows and after state.
pub fn expectation(
    before: &Value,
    case: &str,
    base: &str,
    logs: &str,
    daemon: bool,
) -> Check<Expectation> {
    qualify_before(before, case, base)?;
    let order = returned_order(logs, base, case, daemon)?;
    let mut selectors = Vec::new();
    let mut accepted = None;
    if case == "bbc.com-native" {
        selectors.push("native");
        accepted = Some("native");
    } else if case != "no-cmp" {
        for &id in &order {
            selectors.push(id);
            let index = usize::from(id == "second");
            if before["frames"][index]["present"] == true {
                accepted = Some(id);
                break;
            }
        }
    }
    let classification = match (case, accepted, selectors.len()) {
        ("bbc.com-native", _, _) => "native-action",
        ("no-cmp", _, _) => "no-cmp",
        (_, None, _) => "all-recognized-miss",
        (_, Some(_), 2) => "later-action-after-miss",
        _ => "first-selected-action",
    };
    Ok(Expectation {
        order,
        selectors,
        accepted,
        classification,
        exit: i32::from(accepted.is_none()),
        status: if accepted.is_some() {
            "accepted"
        } else if case == "no-cmp" {
            "no_cmp_detected"
        } else {
            "detected_not_actioned"
        },
        cmp: if case == "bbc.com-native" {
            Some("bbc")
        } else if case == "no-cmp" {
            None
        } else {
            Some("sourcepoint")
        },
        error: if accepted.is_some() {
            None
        } else if case == "no-cmp" {
            Some("consent_no_cmp")
        } else {
            Some("consent_not_actioned")
        },
    })
}

fn same_signature(initial: &Value, sample: &Value) -> Check {
    for field in ["id", "href", "documentURI", "epoch", "token", "readyState"] {
        require!(
            sample.get(field).is_some() && sample[field] == initial[field],
            format!("unchanged document field {field}")
        );
    }
    Ok(())
}

fn effect(initial: &Value, sample: &Value, clicked: bool) -> Check {
    same_signature(initial, sample)?;
    require!(
        sample["count"] == usize::from(clicked),
        "exact document action counter"
    );
    if clicked {
        geometry(sample, false)?;
        if initial["id"] != "native" {
            require!(
                sample["controls"] == json!([]),
                "clicked frame control removed"
            );
        }
    } else {
        for field in ["present", "w", "h"] {
            require!(
                sample.get(field).is_some() && sample[field] == initial[field],
                format!("unchanged control field {field}")
            );
        }
        if initial["id"] != "native" {
            require!(
                sample["controls"] == initial["controls"],
                "unchanged frame controls"
            );
        }
    }
    Ok(())
}

fn document<'a>(state: &'a Value, id: &str) -> Check<&'a Value> {
    match id {
        "native" => Ok(&state["native"]),
        "first" => Ok(&state["frames"][0]),
        "second" => Ok(&state["frames"][1]),
        _ => Err("unknown proof document".to_string()),
    }
}

pub fn validate(
    before: &Value,
    after: &Value,
    expected: &Expectation,
    exit: Option<i32>,
    envelope: &Value,
) -> Check {
    for field in ["href", "epoch", "token", "readyState"] {
        require!(
            after.get(field).is_some() && after[field] == before[field],
            format!("unchanged top {field}")
        );
    }
    require!(
        after["frames"]
            .as_array()
            .is_some_and(|frames| frames.len() == 2),
        "final frame inventory"
    );
    for id in ["first", "second", "native"] {
        let initial = document(before, id)?;
        let final_sample = document(after, id)?;
        require!(final_sample["kind"] == "readback", "final sample kind");
        effect(initial, final_sample, expected.accepted == Some(id))?;
    }
    let mut events: Vec<_> = expected
        .selectors
        .iter()
        .map(|&id| (id, "selector"))
        .collect();
    if let Some(id) = expected.accepted {
        events.extend([(id, "click-before"), (id, "click-after")]);
    }
    let rows = after["proof"].as_array().ok_or("action proof rows")?;
    require!(rows.len() == events.len(), "exact action event count");
    for (row, (id, kind)) in rows.iter().zip(events) {
        require!(
            row["id"] == id && row["kind"] == kind,
            "exact selector/action event ordering"
        );
        let initial = document(before, id)?;
        effect(initial, row, kind == "click-after")?;
        if kind == "click-before" {
            geometry(row, true)?;
        }
    }
    require!(exit == Some(expected.exit), "actual consent exit");
    let result = if expected.error.is_none() {
        &envelope["results"]
    } else {
        envelope
    };
    require!(
        ["status", "cmp", "action"]
            .iter()
            .all(|field| result.get(*field).is_some()),
        "complete consent result fields"
    );
    require!(
        result["status"] == expected.status && result["cmp"] == json!(expected.cmp),
        "distinct consent status and CMP"
    );
    require!(
        result["action"] == json!(expected.accepted.map(|_| "accepted")),
        "consent action result"
    );
    require!(
        envelope["error_type"] == json!(expected.error),
        "distinct consent error"
    );
    Ok(())
}
