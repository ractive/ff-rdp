//! Scripted protocol controls, not recorded Firefox fixtures. Exercise the real
//! event wait with independently observed snapshot state and request destinations.
use super::*;
use ff_rdp_core::transport::{encode_frame, recv_from};
use std::io::{BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn send(stream: &mut TcpStream, value: &Value) {
    stream
        .write_all(encode_frame(&value.to_string()).as_bytes())
        .unwrap();
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Case {
    Eager,
    SameDocument,
    NavigateSameDocument,
    EagerPending,
    SameDocumentPending,
    StillLive,
    Direct,
}

#[test]
fn watched_pending_eager_reaches_fresh_atomic_sample() {
    control(Case::Eager);
}

#[test]
fn watched_pending_same_document_reacquires_before_eval() {
    control(Case::SameDocument);
    control(Case::NavigateSameDocument);
}

#[test]
fn watched_pending_until_original_deadline() {
    control(Case::EagerPending);
    control(Case::SameDocumentPending);
}

#[test]
fn watched_still_live_and_direct_same_document_complete() {
    control(Case::StillLive);
    control(Case::Direct);
}

fn control(case: Case) {
    let eager = matches!(case, Case::Eager | Case::EagerPending);
    let pending_forever = matches!(case, Case::EagerPending | Case::SameDocumentPending);
    let survives = matches!(case, Case::StillLive | Case::Direct);
    let phase = Arc::new(AtomicUsize::new(0)); // old live -> Pending -> fresh live
    let stop = Arc::new(AtomicBool::new(false));
    let side = TcpListener::bind("127.0.0.1:0").unwrap();
    side.set_nonblocking(true).unwrap();
    let endpoint =
        crate::daemon::client::TargetEndpoint::new(side.local_addr().unwrap().port(), "token");
    let snapshot_phase = Arc::clone(&phase);
    let snapshot_stop = Arc::clone(&stop);
    let snapshots = std::thread::spawn(move || {
        let mut states = Vec::new();
        while !snapshot_stop.load(Ordering::SeqCst) {
            let (mut stream, _) = match side.accept() {
                Ok(connection) => connection,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(e) => panic!("snapshot accept: {e}"),
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            assert_eq!(recv_from(&mut reader).unwrap()["auth"], "token");
            send(
                &mut stream,
                &json!({"protocol_version":crate::daemon::server::DAEMON_PROTOCOL_VERSION}),
            );
            let request = recv_from(&mut reader).unwrap();
            assert_eq!(request["type"], "resolve-tab-target");
            assert_eq!(request["descriptor"], "tab");
            let mut state = snapshot_phase.load(Ordering::SeqCst);
            if state == 1 && !pending_forever && states.iter().filter(|&&s| s == 1).count() >= 2 {
                state = 2;
                snapshot_phase.store(state, Ordering::SeqCst);
            }
            states.push(state);
            let response = if state == 1 {
                json!({"from":"daemon","type":"resolve-tab-target","state":"pending"})
            } else {
                let (actor, console, inner) = if state == 0 {
                    ("old", "old/console", 1)
                } else {
                    ("fresh", "fresh/console", 2)
                };
                json!({"from":"daemon","type":"resolve-tab-target","state":"live","target":{
                    "actor":actor,"consoleActor":console,"innerWindowId":inner}})
            };
            send(&mut stream, &response);
        }
        states
    });
    let main = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = main.local_addr().unwrap().port();
    let wire_phase = Arc::clone(&phase);
    let wire = std::thread::spawn(move || {
        let (mut stream, _) = main.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        send(
            &mut stream,
            &json!({"from":"root","applicationType":"browser"}),
        );
        let mut requests = Vec::new();
        let mut forbidden = Vec::new();
        while let Ok(request) = recv_from(&mut reader) {
            requests.push(request.clone());
            let actor = request["to"].as_str().unwrap_or("");
            match request["type"].as_str().unwrap() {
                "navigateTo" => {
                    assert_eq!(actor, "old");
                    assert_eq!(request["url"], "https://new.test/");
                    if !survives {
                        wire_phase.store(1, Ordering::SeqCst);
                        send(
                            &mut stream,
                            &json!({"from":"watcher","type":"target-destroyed-form","target":{"actor":"old","innerWindowId":1}}),
                        );
                    }
                    send(&mut stream, &json!({"from":"old"}));
                }
                "listFrames" => {
                    assert!(actor == "old" || actor == "fresh");
                    send(
                        &mut stream,
                        &json!({"from":actor,"frames":[{"isTopLevel":true,"url":"https://new.test/"}]}),
                    );
                }
                "evaluateJSAsync" => {
                    let state = wire_phase.load(Ordering::SeqCst);
                    let expected = if state == 0 {
                        "old/console"
                    } else {
                        "fresh/console"
                    };
                    if state == 1 || actor != expected {
                        forbidden.push(request);
                        // Stop on the observed forbidden request, not a timeout.
                        break;
                    }
                    let text = request["text"].as_str().unwrap();
                    let result = if eager {
                        assert_eq!(text, READINESS_SAMPLE);
                        json!(
                            json!({"readyState":"complete","epoch":43,"href":"https://new.test/"})
                                .to_string()
                        )
                    } else {
                        assert!(
                            text.contains("document.readyState !== 'complete'")
                                && text.contains("https://old.test/")
                        );
                        json!("https://new.test/")
                    };
                    send(&mut stream, &json!({"from":actor,"resultID":"result"}));
                    send(
                        &mut stream,
                        &json!({"from":actor,"type":"evaluationResult","resultID":"result","result":result}),
                    );
                }
                _ => {
                    forbidden.push(request);
                    break;
                }
            }
        }
        (requests, forbidden)
    });
    let mut transport = RdpTransport::connect("127.0.0.1", port, Duration::from_secs(2)).unwrap();
    let tab = "tab".into();
    let initial = if case == Case::Direct {
        "old/console".into()
    } else {
        super::super::connect_tab::resolve_target_snapshot(
            &mut transport,
            Some(&endpoint),
            &tab,
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap()
        .unwrap()
        .console_actor
    };
    let start = Instant::now();
    let mut probe = ReadyStateProbe {
        target_endpoint: (case != Case::Direct).then_some(endpoint),
        console_actor: Some(initial),
        tab_actor: &tab,
        pre_epoch: Some(42.0),
        first_probe_at: start + Duration::from_millis(30),
        probe_interval: Duration::from_millis(30),
        poll_enabled: eager || case == Case::NavigateSameDocument,
        // The eager case has no href baseline; only the atomic epoch sample can complete it.
        pre_href: if eager {
            String::new()
        } else {
            "https://old.test/".into()
        },
        trust_event_url: true,
    };
    transport
        .send(&json!({"to":"old","type":"navigateTo","url":"https://new.test/"}))
        .unwrap();
    let (_tx, rx) = std::sync::mpsc::channel();
    let bus = Arc::new(Mutex::new(ResourceCommand::new("watcher".into())));
    let result = wait_for_doc_complete(
        &mut transport,
        &bus,
        &rx,
        650,
        WaitLevel::Complete,
        start,
        Some(&mut probe),
        "https://new.test/",
        false,
    );
    let elapsed = start.elapsed();
    drop(transport);
    stop.store(true, Ordering::SeqCst);
    let states = snapshots.join().unwrap();
    let (requests, forbidden) = wire.join().unwrap();
    // stderr-ok: retain scripted request destinations and outcomes for regression/mutation evidence.
    eprintln!(
        "pending control {case:?}: states={states:?} elapsed={elapsed:?} result={result:?} requests={requests:?}"
    );
    assert!(
        forbidden.is_empty(),
        "{case:?}: observed forbidden request(s): {forbidden:?}"
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r["type"] == "navigateTo")
            .count(),
        1
    );
    assert_eq!(
        requests.iter().filter(|r| r["type"] == "getTarget").count(),
        0
    );
    if pending_forever {
        assert!(
            probe.console_actor.is_none(),
            "Pending must retire the probe identity"
        );
        assert!(
            matches!(result, Err(AppError::Timeout(ref message)) if message == "navigate: page did not fire dom-complete within the timeout — use --no-wait to skip or increase --timeout"),
            "{result:?}"
        );
        assert!(
            elapsed >= Duration::from_millis(650) && elapsed < Duration::from_millis(950),
            "{elapsed:?}"
        );
        assert!(states.iter().filter(|&&state| state == 1).count() >= 2);
        assert_eq!(
            requests
                .iter()
                .filter(|r| r["type"] == "evaluateJSAsync")
                .count(),
            0
        );
    } else {
        let result = result.unwrap();
        assert_eq!(result.committed_url, "https://new.test/");
        assert_eq!(result.ready_state, "complete");
        assert!(elapsed < Duration::from_millis(650));
        assert_eq!(
            requests
                .iter()
                .filter(|r| r["type"] == "evaluateJSAsync")
                .count(),
            1
        );
        if !survives {
            assert_eq!(states.first(), Some(&0));
            assert_eq!(states.last(), Some(&2));
            assert_eq!(states.iter().filter(|&&state| state == 1).count(), 2);
        }
    }
}
