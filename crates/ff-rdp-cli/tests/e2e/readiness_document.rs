//! Declared RDP schedules driving the actual CLI. These are not recorded Firefox
//! fixtures, do not execute JavaScript, and do not attribute the old cascade failure.
use std::io::{BufReader, Read, Seek, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use ff_rdp_core::transport::recv_from;
use serde_json::{Value, json};

const FIXTURE: &str = "data:text/html,<style>@import url('data:text/css;base64,aDF7Y29sb3I6cmVkfQ==');</style><h1>test</h1>";

fn long_href() -> &'static str {
    static HREF: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HREF.get_or_init(|| {
        format!(
            "https://long.test/{}",
            "a".repeat(9990 - "https://long.test/".len())
        )
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    Stale,
    StaleChanged,
    IntermediateBlank,
    MissingEpochUnchanged,
    MissingEpochChanged,
    MissingAll,
    ExceptionalHref,
    SplitSample,
    QuerySwitched,
    Intended,
    Redirect,
    ExplicitBlank,
    UpperSchemeBlank,
    UpperContentBlank,
    SameUrlReload,
    BothRefreshedLoading,
    LongStringDirect,
    LongStringBoth,
}

impl Case {
    fn request(self) -> &'static str {
        match self {
            Self::LongStringDirect | Self::LongStringBoth => long_href(),
            Self::ExplicitBlank => "about:blank",
            Self::UpperSchemeBlank => "ABOUT:blank",
            Self::UpperContentBlank => "ABOUT:Blank",
            _ => FIXTURE,
        }
    }

    fn missing_epoch(self) -> bool {
        matches!(
            self,
            Self::MissingEpochUnchanged
                | Self::MissingEpochChanged
                | Self::MissingAll
                | Self::ExceptionalHref
        )
    }

    fn before_href(self) -> &'static str {
        if matches!(
            self,
            Self::SameUrlReload | Self::MissingEpochUnchanged | Self::Stale
        ) {
            FIXTURE
        } else {
            "about:blank"
        }
    }

    fn times_out(self) -> bool {
        matches!(
            self,
            Self::Stale
                | Self::StaleChanged
                | Self::MissingEpochUnchanged
                | Self::MissingAll
                | Self::ExceptionalHref
                | Self::UpperContentBlank
        )
    }

    fn sample(self, poll: usize) -> Value {
        let epoch = if matches!(
            self,
            Self::Stale | Self::StaleChanged | Self::MissingEpochUnchanged
        ) {
            100
        } else {
            200
        };
        let href = match self {
            Self::IntermediateBlank | Self::BothRefreshedLoading if poll == 1 => "about:blank",
            Self::ExplicitBlank | Self::UpperSchemeBlank | Self::UpperContentBlank => "about:blank",
            Self::Redirect => "https://redirect.test/landed",
            Self::LongStringDirect | Self::LongStringBoth => long_href(),
            _ => FIXTURE,
        };
        let state = if self == Self::BothRefreshedLoading && poll == 2 {
            "loading"
        } else {
            "complete"
        };
        json!({"readyState":state,"epoch":epoch,"href":href})
    }
}

fn send(stream: &mut TcpStream, value: &Value) {
    let body = serde_json::to_vec(value).unwrap();
    write!(stream, "{}:", body.len()).unwrap();
    stream.write_all(&body).unwrap();
}

fn run(port: u16, args: &[&str], timeout: &str) -> Output {
    fn bytes(mut file: std::fs::File) -> Vec<u8> {
        file.rewind().unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        bytes
    }
    let stdout = tempfile::tempfile().unwrap();
    let stderr = tempfile::tempfile().unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ff-rdp"))
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--no-daemon",
            "--timeout",
            timeout,
        ])
        .args(args)
        .env("FF_RDP_HOME", home.path())
        .env("RUST_LOG", "off")
        .stdout(Stdio::from(stdout.try_clone().unwrap()))
        .stderr(Stdio::from(stderr.try_clone().unwrap()))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(6);
    let timed_out = loop {
        if child.try_wait().unwrap().is_some() {
            break false;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            break true;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let out = Output {
        status: child.wait().unwrap(),
        stdout: bytes(stdout),
        stderr: bytes(stderr),
    };
    eprintln!("actual child wait: {}", super::support::output_note(&out));
    assert!(!timed_out, "outer CLI bound expired");
    out
}

fn serve(listener: &TcpListener, case: Case) -> Vec<Value> {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut transcript = Vec::new();
    let cascade_expected = matches!(case, Case::QuerySwitched | Case::Intended);
    for cascade in [false, true]
        .into_iter()
        .take(if cascade_expected { 2 } else { 1 })
    {
        let mut stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "script accept bound");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("accept: {e}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        send(
            &mut stream,
            &json!({"from":"root","applicationType":"browser","traits":{},"ua":"Firefox/156.0"}),
        );
        let mut dispatched = false;
        let mut polls = 0;
        let mut targets = 0;
        let doc = if cascade && case == Case::QuerySwitched {
            "query-blank"
        } else {
            "intended"
        };
        while let Ok(request) = recv_from(&mut reader) {
            assert!(Instant::now() < deadline, "bounded protocol script");
            transcript.push(json!({"cascade":cascade,"request":request}));
            let actor = request["to"].as_str().unwrap();
            let method = request["type"].as_str().unwrap();
            let reply = match method {
                "listTabs" => {
                    json!({"from":"root","tabs":[{"actor":"tab","selected":true,"url":"about:blank","title":"scripted"}]})
                }
                "getTarget" => {
                    targets += 1;
                    let name = if !cascade && targets == 1 {
                        "initial"
                    } else if case == Case::BothRefreshedLoading && targets == 2 {
                        "intermediate"
                    } else {
                        doc
                    };
                    json!({"from":"tab","frame":{"actor":name,"consoleActor":format!("{name}/console"),"inspectorActor":format!("{name}/inspector"),"innerWindowId":targets,"url":FIXTURE}})
                }
                "getWatcher" => json!({"from":"tab","actor":"watcher"}),
                "watchTargets" | "watchResources" => json!({"from":actor}),
                "unwatchTargets" | "unwatchResources" => continue,
                "listFrames" => json!({"from":actor,"frames":[{"isTopLevel":true,"url":FIXTURE}]}),
                "navigateTo" => {
                    assert!(!cascade && !dispatched);
                    assert_eq!(actor, "initial");
                    assert_eq!(request["url"], case.request());
                    dispatched = true;
                    json!({"from":actor})
                }
                "evaluateJSAsync" => {
                    assert!(!cascade);
                    let text = request["text"].as_str().unwrap();
                    let mut exception = None;
                    let result = if !dispatched && text == "performance.timing.navigationStart" {
                        if case.missing_epoch() {
                            json!({"type":"undefined"})
                        } else {
                            json!(100)
                        }
                    } else if !dispatched && text == "window.location.href" {
                        if case == Case::ExceptionalHref {
                            exception = Some(json!({"message":"baseline unavailable"}));
                            json!("about:blank") // Value cannot rescue an exceptional baseline.
                        } else if case == Case::MissingAll {
                            json!({"type":"undefined"})
                        } else {
                            json!(case.before_href())
                        }
                    } else if text.contains("var h = window.location.href") {
                        Value::Null // Declared schedule: no same-document shortcut.
                    } else if text.contains("document.readyState") {
                        assert!(dispatched);
                        polls += 1;
                        let sample = case.sample(polls);
                        transcript.push(json!({"sample":sample,"actor":actor}));
                        if text.contains("JSON.stringify") {
                            assert!(
                                text.contains("performance.timing.navigationStart")
                                    && text.contains("window.location.href")
                            );
                            let encoded = sample.to_string();
                            if matches!(case, Case::LongStringDirect | Case::LongStringBoth) {
                                assert_eq!(long_href().len(), 9990);
                                assert!(encoded.len() >= 10000);
                                json!({"type":"longString","actor":"sample-string","length":encoded.len(),"initial":&encoded[..1000]})
                            } else {
                                json!(encoded)
                            }
                        } else {
                            // Before-control adapter for the old boolean predicate. This
                            // interprets declared values only; it is not a JavaScript engine.
                            let epoch = if case.missing_epoch() { 0 } else { 100 };
                            assert!(text.contains(&format!("navigationStart > {epoch}")));
                            json!(
                                sample["readyState"] == "complete"
                                    && sample["epoch"].as_i64().unwrap() > epoch
                            )
                        }
                    } else {
                        assert!(dispatched);
                        assert_eq!(text, "window.location.href");
                        // Legacy split caller or timeout diagnostic. The actual trace
                        // distinguishes these; a queued unused reply is not our proof.
                        let later = case.sample(polls);
                        json!(if case == Case::SplitSample {
                            "about:blank"
                        } else if case == Case::BothRefreshedLoading {
                            if actor == "intermediate/console" {
                                "about:blank"
                            } else {
                                FIXTURE
                            }
                        } else {
                            later["href"].as_str().unwrap()
                        })
                    };
                    send(&mut stream, &json!({"from":actor,"resultID":"sample"}));
                    let mut response = json!({"from":actor,"type":"evaluationResult","resultID":"sample","result":result});
                    if let Some(exception) = exception {
                        response["exception"] = exception;
                    }
                    response
                }
                "substring" => {
                    assert_eq!(actor, "sample-string");
                    let encoded = case.sample(polls).to_string();
                    assert_eq!(request["start"], 0);
                    assert_eq!(request["end"], encoded.len());
                    if case == Case::LongStringBoth {
                        // Arrive while the blocking substring is outstanding:
                        // readiness must replay these into document status.
                        send(
                            &mut stream,
                            &json!({"from":"watcher","type":"resources-available-array","array":[["network-event",[{
                                "actor":"net","resourceId":7,"method":"GET","url":long_href(),"isXHR":false,"cause":{"type":"document"},"startedDateTime":"2026-09-25T00:00:00Z","timeStamp":1
                            }]]]}),
                        );
                        send(
                            &mut stream,
                            &json!({"from":"watcher","type":"resources-updated-array","array":[["network-event",[{
                                "resourceId":7,"resourceUpdates":{"status":"200"}
                            }]]]}),
                        );
                    }
                    json!({"from":actor,"substring":encoded})
                }
                "release" => {
                    assert_eq!(actor, "sample-string");
                    json!({"from":actor})
                }
                "getWalker" => {
                    assert_eq!(actor, format!("{doc}/inspector"));
                    json!({"from":actor,"walker":{"actor":format!("{doc}/walker")}})
                }
                "getPageStyle" => {
                    json!({"from":actor,"pageStyle":{"actor":format!("{doc}/style")}})
                }
                "documentElement" => {
                    assert_eq!(actor, format!("{doc}/walker"));
                    json!({"from":actor,"node":{"actor":format!("{doc}/root"),"nodeType":1,"nodeName":"HTML"}})
                }
                "querySelector" => {
                    assert_eq!(actor, format!("{doc}/walker"));
                    assert_eq!(request["node"], format!("{doc}/root"));
                    assert_eq!(request["selector"], "h1");
                    if case == Case::QuerySwitched {
                        json!({"from":actor})
                    } else {
                        json!({"from":actor,"node":{"actor":"intended/h1","nodeType":1,"nodeName":"H1"}})
                    }
                }
                "getApplied" => {
                    assert_eq!(request["node"], "intended/h1");
                    json!({"from":actor,"error":"scriptedSelectorReached","message":"scripted selector reached; CSS not simulated"})
                }
                _ => panic!("unexpected request: {request}"),
            };
            send(&mut stream, &reply);
        }
    }
    transcript
}

fn experiment(case: Case) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || serve(&listener, case));
    let both = matches!(case, Case::BothRefreshedLoading | Case::LongStringBoth);
    let nav = run(
        port,
        &[
            "navigate",
            "--allow-unsafe-urls",
            case.request(),
            "--wait-strategy",
            if both { "both" } else { "readystate" },
        ],
        if both { "3000" } else { "1000" },
    );
    let cascade = matches!(case, Case::QuerySwitched | Case::Intended)
        .then(|| run(port, &["cascade", "h1", "--prop", "color"], "1000"));
    let transcript = server.join().unwrap();
    eprintln!(
        "{case:?} SCRIPTED TRANSCRIPT {}",
        serde_json::to_string(&transcript).unwrap()
    );
    assert_eq!(
        transcript
            .iter()
            .filter(|r| r["request"]["type"] == "navigateTo")
            .count(),
        1
    );
    if case.times_out() {
        assert_eq!(
            nav.status.code(),
            Some(124),
            "{}",
            super::support::output_note(&nav)
        );
        let response: Value = serde_json::from_slice(&nav.stdout).unwrap();
        assert!(
            response["error"]
                .as_str()
                .unwrap()
                .contains("document.readyState")
        );
        return;
    }
    assert!(
        nav.status.success(),
        "{}",
        super::support::output_note(&nav)
    );
    let response: Value = serde_json::from_slice(&nav.stdout).unwrap();
    let expected = case.sample(3)["href"].as_str().unwrap().to_owned();
    assert_eq!(response["results"]["committed_url"], expected);
    // Exactly the baseline href request, with no post-readiness href read.
    assert_eq!(
        transcript
            .iter()
            .filter(|r| r["request"]["text"] == "window.location.href")
            .count(),
        1,
        "successful URL must come from the accepted atomic sample"
    );
    if matches!(case, Case::LongStringDirect | Case::LongStringBoth) {
        assert_eq!(
            transcript
                .iter()
                .filter(|r| r["request"]["type"] == "substring")
                .count(),
            1
        );
        assert_eq!(
            transcript
                .iter()
                .filter(|r| r["request"]["type"] == "release")
                .count(),
            1
        );
        assert_eq!(
            transcript
                .iter()
                .filter(|r| r.get("sample").is_some())
                .count(),
            1
        );
    }
    if case == Case::LongStringBoth {
        assert_eq!(
            response["results"]["status"], 200,
            "substring-time events must survive replay"
        );
    }
    if case == Case::BothRefreshedLoading {
        let samples: Vec<_> = transcript
            .iter()
            .filter(|r| r.get("sample").is_some())
            .collect();
        assert_eq!(
            samples.len(),
            3,
            "refreshed loading sample must be rejected"
        );
        assert_eq!(samples[0]["actor"], "intermediate/console");
        assert_eq!(samples[1]["actor"], "intended/console");
        assert_eq!(samples[1]["sample"]["readyState"], "loading");
    }
    if let Some(cascade) = cascade {
        assert!(!cascade.status.success());
        assert!(String::from_utf8_lossy(&cascade.stdout).contains(
            if case == Case::QuerySwitched {
                "no element matching selector 'h1'"
            } else {
                "scripted selector reached; CSS not simulated"
            }
        ));
    }
}

#[test]
fn stale_until_deadline() {
    experiment(Case::Stale);
}
#[test]
fn known_stale_epoch_rejects_changed_href() {
    experiment(Case::StaleChanged);
}
#[test]
fn intermediate_blank_is_not_completion() {
    experiment(Case::IntermediateBlank);
}
#[test]
fn missing_epoch_unchanged_href_is_not_progress() {
    experiment(Case::MissingEpochUnchanged);
}
#[test]
fn missing_epoch_changed_href_is_progress() {
    experiment(Case::MissingEpochChanged);
}
#[test]
fn missing_all_baselines_cannot_complete() {
    experiment(Case::MissingAll);
}
#[test]
fn exceptional_href_is_not_a_baseline() {
    experiment(Case::ExceptionalHref);
}
#[test]
fn accepted_sample_supplies_href() {
    experiment(Case::SplitSample);
}
#[test]
fn separate_cascade_can_select_another_document() {
    experiment(Case::QuerySwitched);
}
#[test]
fn intended_selector_reaches_style_query() {
    experiment(Case::Intended);
}
#[test]
fn redirect_is_supported() {
    experiment(Case::Redirect);
}
#[test]
fn explicit_blank_is_supported() {
    experiment(Case::ExplicitBlank);
}
#[test]
fn explicit_blank_scheme_case_is_supported() {
    experiment(Case::UpperSchemeBlank);
}
#[test]
fn blank_content_remains_case_sensitive() {
    experiment(Case::UpperContentBlank);
}
#[test]
fn same_url_fresh_epoch_is_supported() {
    experiment(Case::SameUrlReload);
}
#[test]
fn both_refresh_requires_new_complete_sample() {
    experiment(Case::BothRefreshedLoading);
}

#[test]
fn long_string_direct_keeps_previously_inline_href() {
    experiment(Case::LongStringDirect);
}
#[test]
fn long_string_both_keeps_previously_inline_href() {
    experiment(Case::LongStringBoth);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FallbackStatusCase {
    Observed,
    MissingStatus,
    Unrelated,
    RequestedRedirect,
    OldSameUrl,
    OtherContext,
    Ambiguous,
    TerminalError,
    Neterror,
    MissingContext,
    MissingWindow,
    MissingNavigationFlag,
    NotNavigation,
    UpdateFirst,
    Replayed,
}

fn send_fallback_network(stream: &mut TcpStream, case: FallbackStatusCase, committed: &str) {
    let url = if case == FallbackStatusCase::Unrelated {
        "https://other.test/"
    } else {
        committed
    };
    let inner = if case == FallbackStatusCase::OldSameUrl {
        99
    } else {
        1
    };
    let context = if case == FallbackStatusCase::OtherContext {
        777
    } else {
        42
    };
    let mut resources = vec![
        json!({"actor":"net7","resourceId":7,"method":"GET","url":url,"cause":{"type":"document"},"isXHR":false,"startedDateTime":"2026-09-25T00:00:00Z","timeStamp":1,"isNavigationRequest":true,"innerWindowId":inner,"browsingContextID":context}),
    ];
    if case == FallbackStatusCase::Ambiguous {
        let mut duplicate = resources[0].clone();
        duplicate["resourceId"] = json!(8);
        duplicate["actor"] = json!("net8");
        resources.push(duplicate);
    }
    let missing_field = match case {
        FallbackStatusCase::MissingContext => Some("browsingContextID"),
        FallbackStatusCase::MissingWindow => Some("innerWindowId"),
        FallbackStatusCase::MissingNavigationFlag => Some("isNavigationRequest"),
        _ => None,
    };
    if let Some(field) = missing_field {
        resources[0].as_object_mut().unwrap().remove(field);
    }
    if case == FallbackStatusCase::NotNavigation {
        resources[0]["isNavigationRequest"] = json!(false);
    }
    let update = json!({"from":"watcher","type":"resources-updated-array","array":[["network-event",[{"resourceId":7,"resourceUpdates":{"status":"200"}}]]]});
    if case == FallbackStatusCase::UpdateFirst {
        send(stream, &update);
    }
    send(
        stream,
        &json!({"from":"watcher","type":"resources-available-array","array":[["network-event",resources]]}),
    );
    if !matches!(
        case,
        FallbackStatusCase::MissingStatus | FallbackStatusCase::UpdateFirst
    ) {
        send(stream, &update);
    }
}

fn status_fallback_case(case: FallbackStatusCase) {
    const REQUEST: &str = "https://status.test?fresh=1";
    const COMMITTED: &str = "https://status.test/?fresh=1";
    const REDIRECT: &str = "https://redirect.test/final";
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        send(
            &mut stream,
            &json!({"from":"root","applicationType":"browser","traits":{},"ua":"Firefox/156.0"}),
        );
        let mut trace = Vec::new();
        let mut dispatched = false;
        let mut fallback = false;
        let mut seq = 0;
        let mut replayed = false;
        while let Ok(request) = recv_from(&mut reader) {
            trace.push(request.clone());
            let actor = request["to"].as_str().unwrap();
            let method = request["type"].as_str().unwrap();
            let reply = match method {
                "listTabs" => {
                    json!({"from":"root","tabs":[{"actor":"tab","selected":true,"url":if fallback && case == FallbackStatusCase::Neterror {"about:neterror?e=dnsNotFound&u=https%3A%2F%2Fstatus.test"} else {"about:blank"},"title":"scripted","browsingContextID":42}]})
                }
                "getTarget" => {
                    json!({"from":"tab","frame":{"actor":"target","consoleActor":"console","innerWindowId":if dispatched {2} else {1},"browsingContextID":42,"url":if dispatched {COMMITTED} else {"about:blank"}}})
                }
                "getWatcher" => json!({"from":"tab","actor":"watcher"}),
                "watchTargets" | "watchResources" => json!({"from":actor}),
                "unwatchResources" => {
                    fallback = true;
                    continue;
                }
                "unwatchTargets" => continue,
                "listFrames" => {
                    json!({"from":actor,"frames":[{"isTopLevel":true,"url":COMMITTED}]})
                }
                "navigateTo" => {
                    assert!(!dispatched);
                    assert_eq!(request["url"], REQUEST);
                    dispatched = true;
                    if case != FallbackStatusCase::Replayed {
                        send_fallback_network(&mut stream, case, COMMITTED);
                    }
                    json!({"from":actor})
                }
                "evaluateJSAsync" => {
                    let text = request["text"].as_str().unwrap();
                    seq += 1;
                    if case == FallbackStatusCase::Replayed
                        && !replayed
                        && text.contains("JSON.stringify({readyState:")
                    {
                        // Arrive inside a blocking evaluation, before its ack.
                        // The replay observer must retain both packets.
                        send_fallback_network(&mut stream, case, COMMITTED);
                        replayed = true;
                    }
                    if fallback
                        && case == FallbackStatusCase::TerminalError
                        && text.contains("JSON.stringify({readyState:")
                    {
                        send(
                            &mut stream,
                            &json!({"from":actor,"error":"wrongState","message":"scripted terminal fallback error"}),
                        );
                        continue;
                    }
                    let value = if !dispatched && text == "performance.timing.navigationStart" {
                        json!(100)
                    } else if !dispatched && text == "window.location.href" {
                        json!("about:blank")
                    } else if text.contains("var h = window.location.href") {
                        Value::Null
                    } else if text.contains("JSON.stringify({readyState:") {
                        assert!(dispatched);
                        let href = if case == FallbackStatusCase::RequestedRedirect {
                            REDIRECT
                        } else {
                            COMMITTED
                        };
                        json!(json!({"readyState":if fallback {"complete"} else {"loading"},"epoch":200,"href":href}).to_string())
                    } else {
                        panic!("unexpected evaluation: {text}");
                    };
                    let id = format!("status-{seq}");
                    send(&mut stream, &json!({"from":actor,"resultID":id}));
                    json!({"from":actor,"type":"evaluationResult","resultID":id,"result":value})
                }
                _ => panic!("unexpected request: {request}"),
            };
            send(&mut stream, &reply);
        }
        if case == FallbackStatusCase::Replayed {
            assert!(replayed);
        }
        trace
    });
    let out = run(port, &["navigate", REQUEST], "1000");
    let trace = server.join().unwrap();
    eprintln!("fallback case={case:?} trace={trace:?}");
    assert_eq!(
        trace.iter().filter(|r| r["type"] == "navigateTo").count(),
        1
    );
    assert!(trace.iter().any(|r| r["type"] == "unwatchResources"));
    assert_eq!(
        trace
            .iter()
            .filter(|r| r["type"] == "evaluateJSAsync" && r["text"] == "window.location.href")
            .count(),
        1,
        "only pre-dispatch baseline href evaluation"
    );
    if case == FallbackStatusCase::TerminalError {
        assert!(!out.status.success());
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("wrongState")
                || String::from_utf8_lossy(&out.stdout).contains("wrongState")
        );
        return;
    }
    if case == FallbackStatusCase::Neterror {
        assert_eq!(
            out.status.code(),
            Some(7),
            "{}",
            super::support::output_note(&out)
        );
        let value: Value = serde_json::from_slice(if out.stdout.is_empty() {
            &out.stderr
        } else {
            &out.stdout
        })
        .unwrap();
        assert_eq!(value["error_type"], "nav_dns_fail", "{value}");
        let navigate = trace
            .iter()
            .position(|r| r["type"] == "navigateTo")
            .unwrap();
        assert_eq!(
            trace[navigate + 1..]
                .iter()
                .filter(|r| r["type"] == "listTabs")
                .count(),
            1,
            "preserve existing neterror check, no additional query"
        );
        return;
    }
    assert!(
        out.status.success(),
        "{}",
        super::support::output_note(&out)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    let result = &value["results"];
    assert_eq!(
        result["committed_url"],
        if case == FallbackStatusCase::RequestedRedirect {
            REDIRECT
        } else {
            COMMITTED
        }
    );
    assert_eq!(result["ready_state"], "complete");
    if matches!(
        case,
        FallbackStatusCase::Observed
            | FallbackStatusCase::UpdateFirst
            | FallbackStatusCase::Replayed
    ) {
        assert_eq!(result["status"], 200, "{result}");
        assert_eq!(result["status_reason"], Value::Null);
    } else {
        assert_eq!(
            result["status"],
            Value::Null,
            "must not borrow a different document's status: {result}"
        );
        assert_eq!(
            result["status_reason"],
            if case == FallbackStatusCase::MissingStatus {
                "no_status_reported"
            } else {
                "no_document_request"
            },
            "{result}"
        );
    }
}

#[test]
fn fallback_retains_observed_status() {
    status_fallback_case(FallbackStatusCase::Observed);
}
#[test]
fn fallback_missing_status_stays_null() {
    status_fallback_case(FallbackStatusCase::MissingStatus);
}
#[test]
fn fallback_unrelated_status_stays_null() {
    status_fallback_case(FallbackStatusCase::Unrelated);
}
#[test]
fn fallback_redirect_does_not_borrow_requested_status() {
    status_fallback_case(FallbackStatusCase::RequestedRedirect);
}
#[test]
fn fallback_old_same_url_status_stays_null() {
    status_fallback_case(FallbackStatusCase::OldSameUrl);
}
#[test]
fn fallback_other_context_status_stays_null() {
    status_fallback_case(FallbackStatusCase::OtherContext);
}
#[test]
fn fallback_ambiguous_same_url_status_stays_null() {
    status_fallback_case(FallbackStatusCase::Ambiguous);
}
#[test]
fn fallback_terminal_error_survives_observed_status() {
    status_fallback_case(FallbackStatusCase::TerminalError);
}

#[test]
fn fallback_observed_status_does_not_hide_neterror() {
    status_fallback_case(FallbackStatusCase::Neterror);
}

#[test]
fn fallback_ownership_and_replay_table() {
    let mut failed = Vec::new();
    for case in [
        FallbackStatusCase::MissingContext,
        FallbackStatusCase::MissingWindow,
        FallbackStatusCase::MissingNavigationFlag,
        FallbackStatusCase::NotNavigation,
        FallbackStatusCase::UpdateFirst,
        FallbackStatusCase::Replayed,
    ] {
        if std::panic::catch_unwind(|| status_fallback_case(case)).is_err() {
            failed.push(case);
        }
    }
    assert!(failed.is_empty(), "failed bounded table cases: {failed:?}");
}
