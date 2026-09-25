//! Actual CLI protocol controls, not recorded Firefox fixtures or JavaScript execution.
use ff_rdp_core::transport::recv_from;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::time::Duration;

#[path = "../support/fixture275_selection.rs"]
mod fixture;
#[path = "../support/initial_tab275.rs"]
mod initial_tab;
#[path = "../support/consent275_oracle.rs"]
mod oracle;

mod initial_tab_controls {
    use super::initial_tab::{self, Failure, Report};
    use ff_rdp_core::transport::{encode_frame, recv_from};
    use serde_json::{Value, json};
    use std::io::{self, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread::{self, JoinHandle};
    use std::time::{Duration, Instant};

    const NORMAL_BOUND: Duration = Duration::from_secs(2);
    const SHORT_BOUND: Duration = Duration::from_millis(160);
    const SERVER_BOUND: Duration = Duration::from_secs(3);

    fn greeting() -> Value {
        json!({"from":"root","applicationType":"browser"})
    }

    fn ready() -> Value {
        json!({"from":"root","tabs":[{"actor":"ready-tab","url":"about:blank","selected":true}]})
    }

    fn empty() -> Value {
        json!({"from":"root","tabs":[]})
    }

    fn change() -> Value {
        json!({"from":"root","type":"tabListChanged"})
    }

    struct Peer {
        reader: BufReader<TcpStream>,
        writer: TcpStream,
        requests: Vec<Value>,
        request_arrivals: Vec<Instant>,
        partial_body_bytes: usize,
    }

    impl Peer {
        fn send(&mut self, packet: &Value) {
            self.writer
                .write_all(encode_frame(&packet.to_string()).as_bytes())
                .unwrap();
        }

        fn request(&mut self) {
            let request = recv_from(&mut self.reader).expect("one actual observer request");
            self.request_arrivals.push(Instant::now());
            self.requests.push(request.clone());
            assert_eq!(request, json!({"to":"root","type":"listTabs"}));
        }

        fn closed(&mut self) {
            let mut byte = [0u8];
            let result = self.reader.read(&mut byte);
            // Windows may report an aborted connection after the observer closes
            // with unread fixture bytes. Like reset/EOF, this is terminal; a
            // timeout or any further request byte still fails this assertion.
            assert!(
                matches!(result, Ok(0))
                    || matches!(&result, Err(e) if matches!(e.kind(),
                        io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted)),
                "observer must close without another query: {result:?}"
            );
        }

        fn no_request_for(&mut self, duration: Duration) {
            self.reader
                .get_mut()
                .set_read_timeout(Some(duration))
                .unwrap();
            let mut byte = [0u8];
            let result = self.reader.read(&mut byte);
            assert!(
                matches!(&result, Err(error) if matches!(error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)),
                "no overlapping or accelerated query: {result:?}"
            );
            self.reader
                .get_mut()
                .set_read_timeout(Some(SERVER_BOUND))
                .unwrap();
        }
    }

    struct Worker(Option<JoinHandle<Peer>>);

    impl Worker {
        fn finish(mut self) -> Peer {
            let result = self.0.take().unwrap().join();
            eprintln!(
                "275 initial-tab fixture actual-worker-join success={}",
                result.is_ok()
            );
            result.expect("bounded fixture worker returned successfully")
        }
    }

    impl Drop for Worker {
        fn drop(&mut self) {
            if let Some(worker) = self.0.take() {
                let result = worker.join();
                eprintln!(
                    "275 initial-tab fixture unwind-actual-worker-join success={}",
                    result.is_ok()
                );
            }
        }
    }

    struct Run {
        result: Result<Report, Failure>,
        requests: Vec<Value>,
        request_arrivals: Vec<Instant>,
        partial_body_bytes: usize,
        elapsed: Duration,
    }

    fn run(budget: Option<Duration>, script: impl FnOnce(&mut Peer) + Send + 'static) -> Run {
        run_after_greeting_delay(budget, Duration::ZERO, script)
    }

    fn run_after_greeting_delay(
        budget: Option<Duration>,
        greeting_delay: Duration,
        script: impl FnOnce(&mut Peer) + Send + 'static,
    ) -> Run {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let worker = Worker(Some(thread::spawn(move || {
            let deadline = Instant::now() + SERVER_BOUND;
            let stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "observer did not connect");
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream.set_read_timeout(Some(SERVER_BOUND)).unwrap();
            stream.set_write_timeout(Some(SERVER_BOUND)).unwrap();
            let mut peer = Peer {
                reader: BufReader::new(stream.try_clone().unwrap()),
                writer: stream,
                requests: Vec::new(),
                request_arrivals: Vec::new(),
                partial_body_bytes: 0,
            };
            thread::sleep(greeting_delay);
            peer.send(&greeting());
            script(&mut peer);
            assert!(
                matches!(listener.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock),
                "exactly one accepted connection; no queued reconnect"
            );
            peer
        })));
        let started = Instant::now();
        let result = match budget {
            Some(budget) => initial_tab::observe_with_timeout(42, port, budget),
            None => initial_tab::observe(42, port),
        };
        let elapsed = started.elapsed();
        // Join BEFORE result assertions, including observer errors. Drop joins
        // as a non-panicking fallback if the observer itself unexpectedly panics.
        let peer = worker.finish();
        Run {
            result,
            requests: peer.requests,
            request_arrivals: peer.request_arrivals,
            partial_body_bytes: peer.partial_body_bytes,
            elapsed,
        }
    }

    fn assert_ready(run: Run, requests: usize, notifications: usize) {
        let report = run.result.unwrap();
        assert_eq!(report.requests, requests);
        assert_eq!(run.requests.len(), requests);
        assert_eq!(report.notifications, notifications);
        assert_eq!(report.tabs.len(), 1);
        assert_eq!(report.tabs[0].actor.as_ref(), "ready-tab");
        assert!(
            report
                .observations
                .iter()
                .any(|row| row["event"] == "ready")
        );
    }

    fn assert_absolute_scheduled_arrivals(run: &Run) {
        let report = run.result.as_ref().unwrap();
        let requests: Vec<_> = report
            .observations
            .iter()
            .filter(|row| row["event"] == "list-request")
            .collect();
        assert_eq!(requests.len(), 2);
        assert_eq!(run.request_arrivals.len(), requests.len());
        assert_eq!(requests[0]["data"]["slot"], 0);
        let slot = u32::try_from(requests[1]["data"]["slot"].as_u64().unwrap()).unwrap();
        assert!((1..=8).contains(&slot));
        let due = report.start + (NORMAL_BOUND / 10) * slot;
        let arrival = run.request_arrivals[1];
        eprintln!(
            "275 notification control slot={slot} due_us={} received_us={}",
            due.duration_since(report.start).as_micros(),
            arrival.duration_since(report.start).as_micros()
        );
        // Completion of actual socket decoding must not precede the absolute
        // slot. This is not a cooldown from the reply. Peer scheduling/decoding
        // can delay this timestamp; it does not prove the kernel's arrival time.
        assert!(arrival >= due, "request arrived before its absolute slot");
    }

    #[test]
    fn initial_tab_immediate_reply() {
        assert_ready(
            run(None, |peer| {
                peer.request();
                peer.send(&ready());
                peer.closed();
            }),
            1,
            0,
        );
    }

    #[test]
    fn initial_tab_empty_then_real_change() {
        let run = run(Some(NORMAL_BOUND), |peer| {
            peer.request();
            peer.send(&empty());
            peer.send(&change());
            peer.request();
            peer.send(&ready());
            peer.closed();
        });
        assert_absolute_scheduled_arrivals(&run);
        assert_ready(run, 2, 1);
    }

    #[test]
    fn initial_tab_interleaved_change_is_retained() {
        let run = run(Some(NORMAL_BOUND), |peer| {
            peer.request();
            peer.send(&change());
            peer.send(&empty());
            peer.request();
            peer.send(&ready());
            peer.closed();
        });
        assert_absolute_scheduled_arrivals(&run);
        assert_ready(run, 2, 1);
    }

    #[test]
    fn initial_tab_later_descriptor_without_notification() {
        // The archived predecessor forbade this second snapshot. That policy
        // relied on notification coverage Firefox does not guarantee at startup.
        assert_ready(
            run(Some(NORMAL_BOUND), |peer| {
                peer.request();
                peer.send(&empty());
                // Descriptor state becomes ready later, without any event.
                thread::sleep(Duration::from_millis(20));
                let later = ready();
                peer.request();
                peer.send(&later);
                peer.closed();
            }),
            2,
            0,
        );
    }

    #[test]
    fn initial_tab_unrelated_pushes_do_not_renew_deadline() {
        let run = run(Some(SHORT_BOUND), |peer| {
            peer.request();
            // Leave the request outstanding while flooding unrelated pushes.
            for _ in 0..50 {
                let packet =
                    encode_frame(&json!({"from":"root","type":"workerListChanged"}).to_string());
                if let Err(error) = peer.writer.write_all(packet.as_bytes()) {
                    assert!(
                        matches!(
                            error.kind(),
                            io::ErrorKind::BrokenPipe
                                | io::ErrorKind::ConnectionReset
                                | io::ErrorKind::ConnectionAborted
                        ),
                        "{error}"
                    );
                    break;
                }
                thread::sleep(Duration::from_millis(30));
            }
            peer.closed();
        });
        let failure = run.result.unwrap_err();
        assert_eq!(failure.report.requests, 1);
        assert_eq!(run.requests.len(), 1);
        assert_eq!(failure.report.notifications, 0);
        assert!(
            failure
                .report
                .observations
                .iter()
                .any(|row| row["data"]["type"] == "workerListChanged")
        );
        assert!(
            failure.reason.contains("timed out") || failure.reason.contains("deadline"),
            "{}",
            failure.reason
        );
        assert!(
            run.elapsed < Duration::from_secs(1),
            "pushes must not reset160ms bound: {:?}",
            run.elapsed
        );
    }

    #[test]
    fn initial_tab_two_scheduled_cycles_record_events() {
        assert_ready(
            run(Some(NORMAL_BOUND), |peer| {
                peer.request();
                for _ in 0..2 {
                    peer.send(&empty());
                    peer.send(&change());
                    peer.request();
                }
                peer.send(&ready());
                peer.closed();
            }),
            3,
            2,
        );
    }

    #[test]
    fn initial_tab_malformed_descriptor_is_terminal() {
        let run = run(Some(NORMAL_BOUND), |peer| {
            peer.request();
            peer.send(&json!({"from":"root","tabs":[{"actor":"","selected":true}]}));
            peer.closed();
        });
        let failure = run.result.unwrap_err();
        assert!(failure.reason.contains("descriptor"));
        assert_eq!(failure.report.requests, 1);
        assert_eq!(run.requests.len(), 1);
        assert!(failure.report.tabs.is_empty());
    }

    #[test]
    fn initial_tab_eof_is_terminal() {
        let run = run(Some(NORMAL_BOUND), |peer| {
            peer.request();
            peer.writer.shutdown(std::net::Shutdown::Both).unwrap();
        });
        let failure = run.result.unwrap_err();
        assert!(failure.reason.contains("receive"));
        assert_eq!(failure.report.requests, 1);
        assert_eq!(run.requests.len(), 1);
        assert!(failure.report.tabs.is_empty());
    }

    #[test]
    fn initial_tab_request_cap_stops_always_empty_replies() {
        let run = run(Some(NORMAL_BOUND), |peer| {
            peer.request();
            for request in 1..=9 {
                peer.send(&empty());
                if request < 9 {
                    peer.request();
                }
            }
            peer.closed();
        });
        let failure = run.result.unwrap_err();
        assert!(failure.reason.contains("request cap"));
        assert_eq!(failure.report.requests, 9);
        assert_eq!(failure.report.notifications, 0);
        assert_eq!(run.requests.len(), 9);
        assert!(failure.report.tabs.is_empty());
    }

    #[test]
    fn initial_tab_unanswered_request_never_overlaps() {
        let run = run(Some(SHORT_BOUND), |peer| {
            peer.request();
            // Notifications cannot complete the outstanding request.
            for _ in 0..12 {
                peer.send(&change());
            }
            peer.closed();
        });
        let failure = run.result.unwrap_err();
        assert_eq!(failure.report.requests, 1);
        assert_eq!(run.requests.len(), 1);
        assert_eq!(failure.report.notifications, 12);
        assert!(failure.reason.contains("timed out") || failure.reason.contains("deadline"));
        assert!(run.elapsed < Duration::from_secs(1));
    }

    #[test]
    fn initial_tab_slow_reply_skips_elapsed_slots() {
        let run = run(Some(NORMAL_BOUND), |peer| {
            peer.request();
            // The 200,400,600ms opportunities elapse while this request owns
            // its reply. The next observation must use a future fixed slot.
            thread::sleep(Duration::from_millis(650));
            peer.send(&empty());
            peer.request();
            peer.send(&ready());
            peer.closed();
        });
        let report = run.result.as_ref().unwrap();
        let slots: Vec<_> = report
            .observations
            .iter()
            .filter(|row| row["event"] == "list-request")
            .map(|row| row["data"]["slot"].as_u64().unwrap())
            .collect();
        assert_eq!(slots[0], 0);
        assert!(slots[1] >= 4, "missed slots are not caught up: {slots:?}");
        assert_ready(run, 2, 0);
    }

    #[test]
    fn initial_tab_slow_greeting_does_not_restart_schedule() {
        let run =
            run_after_greeting_delay(Some(NORMAL_BOUND), Duration::from_millis(650), |peer| {
                peer.request();
                peer.send(&empty());
                peer.request();
                peer.send(&ready());
                peer.closed();
            });
        let report = run.result.as_ref().unwrap();
        let scheduled = report
            .observations
            .iter()
            .find(|row| row["event"] == "scheduled")
            .unwrap();
        assert!(scheduled["data"]["slot"].as_u64().unwrap() >= 4);
        assert_ready(run, 2, 0);
    }

    #[test]
    fn initial_tab_exhausted_slots_do_not_create_late_query() {
        let run = run(Some(NORMAL_BOUND), |peer| {
            peer.request();
            thread::sleep(Duration::from_millis(1700));
            peer.send(&empty());
            peer.closed();
        });
        let failure = run.result.unwrap_err();
        assert!(failure.reason.contains("slots exhausted"));
        assert_eq!(failure.report.requests, 1);
        assert_eq!(run.requests.len(), 1);
        assert!(failure.report.tabs.is_empty());
    }

    #[test]
    fn initial_tab_protocol_error_is_terminal() {
        let run = run(Some(NORMAL_BOUND), |peer| {
            peer.request();
            peer.send(&json!({"from":"root","error":"denied","message":"controlled"}));
            peer.closed();
        });
        let failure = run.result.unwrap_err();
        assert!(failure.reason.contains("protocol error"));
        assert_eq!(failure.report.requests, 1);
        assert_eq!(run.requests.len(), 1);
        assert!(failure.report.tabs.is_empty());
    }

    #[test]
    fn initial_tab_malformed_reply_is_terminal() {
        let run = run(Some(NORMAL_BOUND), |peer| {
            peer.request();
            peer.send(&json!({"from":"root","tabs":"invalid"}));
            peer.closed();
        });
        let failure = run.result.unwrap_err();
        assert!(failure.reason.contains("malformed list"));
        assert_eq!(failure.report.requests, 1);
        assert_eq!(run.requests.len(), 1);
        assert!(failure.report.tabs.is_empty());
    }

    #[test]
    fn initial_tab_malformed_frame_is_terminal() {
        let run = run(Some(NORMAL_BOUND), |peer| {
            peer.request();
            peer.writer.write_all(b"x:").unwrap();
            peer.closed();
        });
        let failure = run.result.unwrap_err();
        assert!(failure.reason.contains("receive"));
        assert_eq!(failure.report.requests, 1);
        assert_eq!(run.requests.len(), 1);
        assert!(failure.report.tabs.is_empty());
    }

    #[test]
    fn initial_tab_duplicate_descriptor_is_terminal() {
        let run = run(Some(NORMAL_BOUND), |peer| {
            peer.request();
            let tab = ready()["tabs"][0].clone();
            peer.send(&json!({"from":"root","tabs":[tab,tab]}));
            peer.closed();
        });
        let failure = run.result.unwrap_err();
        assert!(failure.reason.contains("duplicate descriptor"));
        assert_eq!(failure.report.requests, 1);
        assert_eq!(run.requests.len(), 1);
        assert!(failure.report.tabs.is_empty());
    }

    #[test]
    fn initial_tab_partial_prefix_has_one_absolute_deadline() {
        let run = run(Some(SHORT_BOUND), |peer| {
            peer.request();
            peer.writer.write_all(b"10").unwrap();
            peer.closed();
        });
        let failure = run.result.unwrap_err();
        assert_eq!(failure.report.requests, 1);
        assert_eq!(run.requests.len(), 1);
        assert!(failure.report.bytes_read >= encode_frame(&greeting().to_string()).len() + 2);
        assert!(failure.reason.contains("timed out") || failure.reason.contains("deadline"));
        assert!(failure.report.tabs.is_empty());
        assert!(run.elapsed < Duration::from_secs(1));
    }

    #[test]
    fn initial_tab_partial_frame_survives_scheduled_slots() {
        assert_ready(
            run(Some(NORMAL_BOUND), |peer| {
                peer.request();
                let frame = encode_frame(&ready().to_string());
                peer.writer.write_all(&frame.as_bytes()[..1]).unwrap();
                peer.no_request_for(Duration::from_millis(250));
                let middle = frame.find(':').unwrap() + 8;
                peer.writer.write_all(&frame.as_bytes()[1..middle]).unwrap();
                peer.no_request_for(Duration::from_millis(250));
                peer.writer.write_all(&frame.as_bytes()[middle..]).unwrap();
                peer.closed();
            }),
            1,
            0,
        );
    }

    #[test]
    fn initial_tab_late_success_is_rejected() {
        let run = run(Some(SHORT_BOUND), |peer| {
            peer.request();
            thread::sleep(Duration::from_millis(240));
            let result = peer
                .writer
                .write_all(encode_frame(&ready().to_string()).as_bytes());
            assert!(
                result.is_ok()
                    || matches!(result, Err(ref error) if matches!(
                error.kind(), io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted))
            );
            peer.closed();
        });
        let failure = run.result.unwrap_err();
        assert_eq!(failure.report.requests, 1);
        assert_eq!(run.requests.len(), 1);
        assert!(failure.reason.contains("timed out") || failure.reason.contains("deadline"));
        assert!(failure.report.tabs.is_empty());
        assert!(run.elapsed < Duration::from_secs(1));
    }

    #[test]
    fn initial_tab_partial_frame_has_one_absolute_deadline() {
        let run = run(Some(SHORT_BOUND), |peer| {
            peer.request();
            peer.writer.write_all(b"10000:").unwrap();
            for _ in 0..50 {
                match peer.writer.write_all(b" ") {
                    Ok(()) => peer.partial_body_bytes += 1,
                    Err(error) => {
                        assert!(
                            matches!(
                                error.kind(),
                                io::ErrorKind::BrokenPipe
                                    | io::ErrorKind::ConnectionReset
                                    | io::ErrorKind::ConnectionAborted
                            ),
                            "{error}"
                        );
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(30));
            }
            peer.closed();
        });
        let failure = run.result.unwrap_err();
        assert_eq!(failure.report.requests, 1);
        assert_eq!(run.requests.len(), 1);
        assert!(run.partial_body_bytes > 0);
        assert!(
            failure.report.bytes_read
                > encode_frame(&greeting().to_string()).len() + b"10000:".len(),
            "actual partial body consumed"
        );
        assert!(failure.report.tabs.is_empty());
        assert!(
            run.elapsed < Duration::from_secs(1),
            "partial reads must not renew160ms bound: {:?}",
            run.elapsed
        );
        assert!(
            failure.reason.contains("timed out") || failure.reason.contains("deadline"),
            "{}",
            failure.reason
        );
    }
}

// The oracle controls never invoke the CLI or execute JavaScript. Their expected
// winners and event sequences are handwritten independently of the oracle.
mod oracle_controls {
    use super::oracle;
    use serde_json::{Value, json};

    const BASE: &str = "http://127.0.0.1:12345";

    fn before(case: &str, accepts: [bool; 3]) -> Value {
        let top = format!("{BASE}/{case}");
        let native = json!({"kind":"readback","id":"native","href":top,"documentURI":top,
            "epoch":100,"readyState":"complete","token":case,"count":0,
            "present":accepts[2],"w":if accepts[2] {Some(80)} else {None},"h":if accepts[2] {Some(20)} else {None}});
        let frames: Vec<_> = ["first", "second"].iter().zip(accepts).map(|(id, accepts)| {
            let prefix = if case == "no-cmp" { "unrecognized" } else { "sourcepoint" };
            let href = format!("{top}/{prefix}-{id}");
            json!({"kind":"readback","id":id,"href":href,"documentURI":href,"epoch":101,
                "readyState":"complete","token":format!("{case}-{id}"),"count":0,"present":accepts,
                "w":if accepts {Some(80)} else {None},"h":if accepts {Some(20)} else {None},
                "controls":[{"label":if accepts {"Accept all"} else {"Reject only"},"disabled":false}]})
        }).collect();
        json!({"href":top,"epoch":100,"readyState":"complete","token":case,"proof":[],"native":native,"frames":frames})
    }

    fn document(state: &Value, id: &str) -> Value {
        match id {
            "native" => state["native"].clone(),
            "first" => state["frames"][0].clone(),
            "second" => state["frames"][1].clone(),
            _ => panic!("unknown test document"),
        }
    }

    fn target(id: &str, url: &str, top: bool) -> String {
        format!(
            "TargetEvent {{ actor: ActorId(\"{id}-target\"), url: Some(\"{url}\"), title: Some(\"\"), target_type: \"frame\", is_top_level: {top}, console_actor: Some(ActorId(\"{id}-console\")), inspector_actor: None, browsing_context_id: Some(1), process_id: Some(2) }}"
        )
    }

    fn trace_from(targets: &[String]) -> String {
        format!(
            "2026-09-25T10:00:00Z DEBUG ff_rdp_cli::frame_targets: FRAME_TARGETS_BEGIN pid=42 via_daemon=true\n2026-09-25T10:00:01Z DEBUG ff_rdp_cli::frame_targets: FRAME_TARGETS_END pid=42 via_daemon=true elapsed_ns=100 result=Ok([{}])\n",
            targets.join(", ")
        )
    }

    fn targets(state: &Value, order: [&str; 2]) -> Vec<String> {
        // Top deliberately sits between the frames: only non-top relative order matters.
        vec![
            target(
                order[0],
                document(state, order[0])["href"].as_str().unwrap(),
                false,
            ),
            target("top", state["href"].as_str().unwrap(), true),
            target(
                order[1],
                document(state, order[1])["href"].as_str().unwrap(),
                false,
            ),
        ]
    }

    fn observed(before: &Value, selectors: &[&str], actions: &[&str]) -> Value {
        let mut after = before.clone();
        let mut rows = Vec::new();
        for id in selectors {
            let mut row = document(before, id);
            row["kind"] = json!("selector");
            rows.push(row);
        }
        for id in actions {
            let mut row = document(before, id);
            row["kind"] = json!("click-before");
            rows.push(row.clone());
            row["kind"] = json!("click-after");
            row["count"] = json!(1);
            row["present"] = json!(false);
            row["w"] = Value::Null;
            row["h"] = Value::Null;
            if *id != "native" {
                row["controls"] = json!([]);
            }
            rows.push(row.clone());
            row["kind"] = json!("readback");
            match *id {
                "native" => after["native"] = row,
                "first" => after["frames"][0] = row,
                "second" => after["frames"][1] = row,
                _ => panic!("unknown observed action"),
            }
        }
        after["proof"] = json!(rows);
        after
    }

    fn envelope(expected: &Value) -> Value {
        let result = json!({"cmp":expected["cmp"],"action":if expected["accepted"].is_null() {Value::Null} else {json!("accepted")},"status":expected["status"]});
        if expected["error"].is_null() {
            json!({"results":result})
        } else {
            let mut result = result;
            result["error_type"] = expected["error"].clone();
            result
        }
    }

    fn positive(case: &str, controls: [bool; 3], order: [&str; 2], literal: &Value) {
        let before = before(case, controls);
        let expected = oracle::expectation(
            &before,
            case,
            BASE,
            &trace_from(&targets(&before, order)),
            true,
        )
        .unwrap();
        assert_eq!(expected.order, order);
        let mut actual = expected.record();
        actual.as_object_mut().unwrap().remove("order");
        assert_eq!(&actual, literal, "handwritten {case} expectation");
        let selectors: Vec<_> = literal["selectors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap())
            .collect();
        let actions: Vec<_> = literal["accepted"].as_str().into_iter().collect();
        let after = observed(&before, &selectors, &actions);
        let exit = i32::try_from(literal["exit"].as_i64().unwrap()).unwrap();
        oracle::validate(&before, &after, &expected, Some(exit), &envelope(literal)).unwrap();
    }

    fn order_negatives(order: [&str; 2]) {
        let all_miss = before("all-miss", [false, false, false]);
        let expected = oracle::expectation(
            &all_miss,
            "all-miss",
            BASE,
            &trace_from(&targets(&all_miss, order)),
            true,
        )
        .unwrap();
        let miss_result = json!({"cmp":"sourcepoint","action":null,"status":"detected_not_actioned","error_type":"consent_not_actioned"});
        assert!(
            oracle::validate(
                &all_miss,
                &observed(&all_miss, &[order[0]], &[]),
                &expected,
                Some(1),
                &miss_result
            )
            .is_err(),
            "first-only miss traversal"
        );

        let first = before("first", [true, true, false]);
        let expected = oracle::expectation(
            &first,
            "first",
            BASE,
            &trace_from(&targets(&first, order)),
            true,
        )
        .unwrap();
        let success =
            json!({"results":{"cmp":"sourcepoint","action":"accepted","status":"accepted"}});
        assert!(
            oracle::validate(
                &first,
                &observed(&first, &order, &order),
                &expected,
                Some(0),
                &success
            )
            .is_err(),
            "multiple actions"
        );
        assert!(
            oracle::validate(
                &first,
                &observed(&first, &[order[1]], &[order[1]]),
                &expected,
                Some(0),
                &success
            )
            .is_err(),
            "wrong first-success winner"
        );

        let native = before("bbc.com-native", [true, true, true]);
        let expected = oracle::expectation(
            &native,
            "bbc.com-native",
            BASE,
            &trace_from(&targets(&native, order)),
            true,
        )
        .unwrap();
        let native_result =
            json!({"results":{"cmp":"bbc","action":"accepted","status":"accepted"}});
        assert!(
            oracle::validate(
                &native,
                &observed(&native, &[order[0]], &[order[0]]),
                &expected,
                Some(0),
                &native_result
            )
            .is_err(),
            "iframe cannot bypass native"
        );
    }

    #[test]
    fn order_first_second() {
        let order = ["first", "second"];
        positive(
            "later",
            [false, true, false],
            order,
            &json!({"selectors":["first","second"],"accepted":"second","classification":"later-action-after-miss","exit":0,"status":"accepted","cmp":"sourcepoint","error":null}),
        );
        positive(
            "first",
            [true, true, false],
            order,
            &json!({"selectors":["first"],"accepted":"first","classification":"first-selected-action","exit":0,"status":"accepted","cmp":"sourcepoint","error":null}),
        );
        positive(
            "all-miss",
            [false, false, false],
            order,
            &json!({"selectors":["first","second"],"accepted":null,"classification":"all-recognized-miss","exit":1,"status":"detected_not_actioned","cmp":"sourcepoint","error":"consent_not_actioned"}),
        );
        positive(
            "no-cmp",
            [false, false, false],
            order,
            &json!({"selectors":[],"accepted":null,"classification":"no-cmp","exit":1,"status":"no_cmp_detected","cmp":null,"error":"consent_no_cmp"}),
        );
        positive(
            "bbc.com-native",
            [true, true, true],
            order,
            &json!({"selectors":["native"],"accepted":"native","classification":"native-action","exit":0,"status":"accepted","cmp":"bbc","error":null}),
        );
        order_negatives(order);
    }

    #[test]
    fn order_second_first() {
        let order = ["second", "first"];
        positive(
            "later",
            [false, true, false],
            order,
            &json!({"selectors":["second"],"accepted":"second","classification":"first-selected-action","exit":0,"status":"accepted","cmp":"sourcepoint","error":null}),
        );
        positive(
            "first",
            [true, true, false],
            order,
            &json!({"selectors":["second"],"accepted":"second","classification":"first-selected-action","exit":0,"status":"accepted","cmp":"sourcepoint","error":null}),
        );
        positive(
            "all-miss",
            [false, false, false],
            order,
            &json!({"selectors":["second","first"],"accepted":null,"classification":"all-recognized-miss","exit":1,"status":"detected_not_actioned","cmp":"sourcepoint","error":"consent_not_actioned"}),
        );
        positive(
            "no-cmp",
            [false, false, false],
            order,
            &json!({"selectors":[],"accepted":null,"classification":"no-cmp","exit":1,"status":"no_cmp_detected","cmp":null,"error":"consent_no_cmp"}),
        );
        positive(
            "bbc.com-native",
            [true, true, true],
            order,
            &json!({"selectors":["native"],"accepted":"native","classification":"native-action","exit":0,"status":"accepted","cmp":"bbc","error":null}),
        );
        order_negatives(order);
    }

    #[test]
    fn rejects_invalid_observations() {
        let order = ["first", "second"];
        let no_cmp = before("no-cmp", [false, false, false]);
        let expected = oracle::expectation(
            &no_cmp,
            "no-cmp",
            BASE,
            &trace_from(&targets(&no_cmp, order)),
            true,
        )
        .unwrap();
        let absent = json!({"cmp":null,"action":null,"status":"no_cmp_detected","error_type":"consent_no_cmp"});
        let mut changed = observed(&no_cmp, &[], &[]);
        changed["frames"][1]["token"] = json!("replacement-document");
        assert!(
            oracle::validate(&no_cmp, &changed, &expected, Some(1), &absent).is_err(),
            "unselected document identity"
        );
        let success =
            json!({"results":{"cmp":"sourcepoint","action":"accepted","status":"accepted"}});
        assert!(
            oracle::validate(
                &no_cmp,
                &observed(&no_cmp, &[], &[]),
                &expected,
                Some(0),
                &success
            )
            .is_err(),
            "absence is not acceptance"
        );

        let first = before("first", [true, true, false]);
        let expected = oracle::expectation(
            &first,
            "first",
            BASE,
            &trace_from(&targets(&first, order)),
            true,
        )
        .unwrap();
        let mut missing_effect = observed(&first, &["first"], &["first"]);
        missing_effect["frames"][0]["present"] = json!(true);
        assert!(
            oracle::validate(&first, &missing_effect, &expected, Some(0), &success).is_err(),
            "accepted without removal"
        );

        let later = before("later", [false, true, false]);
        let expected = oracle::expectation(
            &later,
            "later",
            BASE,
            &trace_from(&targets(&later, order)),
            true,
        )
        .unwrap();
        assert!(
            oracle::validate(
                &later,
                &observed(&later, &["second"], &["second"]),
                &expected,
                Some(0),
                &success
            )
            .is_err(),
            "skipped known miss"
        );
    }

    #[test]
    fn rejects_invalid_trace() {
        let before = before("later", [false, true, false]);
        let targets = targets(&before, ["first", "second"]);
        let valid = trace_from(&targets);
        let lines: Vec<_> = valid.lines().collect();
        let mut duplicate = targets.clone();
        duplicate[2] = targets[0].clone();
        let mut extra = targets.clone();
        extra.push(target("extra", &format!("{BASE}/unexpected"), false));
        let wrong_url = target("second", &format!("{BASE}/wrong"), false).replace(
            "title: Some(\"\")",
            &format!("title: Some(\"{BASE}/later/sourcepoint-second\")"),
        );
        for (label, invalid) in [
            ("missing END", lines[0].to_string()),
            ("duplicate END", format!("{valid}{}\n", lines[1])),
            (
                "wrong route",
                valid.replace("via_daemon=true", "via_daemon=false"),
            ),
            ("duplicate frame", trace_from(&duplicate)),
            ("missing frame", trace_from(&targets[..2])),
            ("unexpected fourth target", trace_from(&extra)),
            (
                "title is not URL field",
                trace_from(&[targets[0].clone(), targets[1].clone(), wrong_url]),
            ),
        ] {
            assert!(
                oracle::expectation(&before, "later", BASE, &invalid, true).is_err(),
                "{label}"
            );
        }
    }

    // One explicitly selected offline replay; ignored in ordinary workspace gates.
    // It opens only existing files, starts no process, and writes no qualification.
    #[test]
    #[ignore = "offline private records: set FF_RDP_275_REPLAY_ROOT to the iter275 evidence directory"]
    fn replay_retained_after_records() {
        let root = std::path::PathBuf::from(
            std::env::var_os("FF_RDP_275_REPLAY_ROOT").expect("explicit private replay root"),
        );
        let read_json = |path: &std::path::Path| -> Value {
            serde_json::from_slice(&std::fs::read(path).expect("original JSON evidence"))
                .expect("original JSON")
        };
        let read_state = |path: &std::path::Path| -> Value {
            serde_json::from_str(
                read_json(path)["results"]
                    .as_str()
                    .expect("serialized original state"),
            )
            .expect("original state")
        };
        let wait = regex::Regex::new(
            r"^actual_wait=exit status: ([0-9]+) status=Some\(([0-9]+)\) stdout=",
        )
        .unwrap();
        for (route, case, selected, accepted, classification) in [
            (
                "direct",
                "later",
                &["first", "second"][..],
                Some("second"),
                "later-action-after-miss",
            ),
            (
                "direct",
                "first",
                &["first"][..],
                Some("first"),
                "first-selected-action",
            ),
            (
                "direct",
                "all-miss",
                &["first", "second"][..],
                None,
                "all-recognized-miss",
            ),
            ("direct", "no-cmp", &[][..], None, "no-cmp"),
            (
                "direct",
                "bbc.com-native",
                &["native"][..],
                Some("native"),
                "native-action",
            ),
            (
                "daemon",
                "later",
                &["second"][..],
                Some("second"),
                "first-selected-action",
            ),
        ] {
            let directory = root.join(format!("after-{route}"));
            let prefix = format!("{route}-{case}");
            let before = read_state(&directory.join(format!("{prefix}-before.stdout")));
            let after = read_state(&directory.join(format!("{prefix}-after.stdout")));
            let base = before["href"]
                .as_str()
                .unwrap()
                .strip_suffix(&format!("/{case}"))
                .expect("case URL");
            let logs = std::fs::read_to_string(directory.join(format!("{prefix}-consent.stderr")))
                .expect("actual command stderr");
            let expected =
                oracle::expectation(&before, case, base, &logs, route == "daemon").unwrap();
            assert_eq!(
                expected.selectors, selected,
                "retained selector expectation"
            );
            assert_eq!(expected.accepted, accepted, "retained accepted frame");
            assert_eq!(
                expected.classification, classification,
                "no invented continuation"
            );
            let test_log = std::fs::read_to_string(directory.join("test.stderr"))
                .expect("original wait receipt");
            let marker = format!("275 command stage={prefix}-consent ");
            let waits: Vec<_> = test_log
                .lines()
                .filter_map(|line| line.strip_prefix(&marker))
                .filter_map(|line| wait.captures(line))
                .collect();
            assert_eq!(waits.len(), 1, "one original actual command wait");
            assert_eq!(waits[0][1], waits[0][2]);
            let exit = waits[0][1].parse::<i32>().unwrap();
            let envelope = read_json(&directory.join(format!("{prefix}-consent.stdout")));
            oracle::validate(&before, &after, &expected, Some(exit), &envelope).unwrap();
            let original = read_json(&directory.join("qualification.json"));
            if route == "daemon" {
                assert_eq!(original["child_actual_wait"], 101);
                assert_eq!(original["cases"].as_array().unwrap().len(), 1);
                assert!(
                    original["verdict"]
                        .as_str()
                        .unwrap()
                        .starts_with("FAILED qualification")
                );
            } else {
                assert_eq!(original["child_actual_wait"], 0);
                assert_eq!(original["cases"].as_array().unwrap().len(), 5);
            }
            eprintln!(
                "275 offline replay {}",
                json!({"route":route,"case":case,"expected":expected.record(),
                "original_arm_verdict":original["verdict"],"original_arm_exit":original["child_actual_wait"],
                "limits":"Read-only semantic replay; original arm verdict/exit unchanged. Sole daemon case is first-selected action, not positive later continuation; four daemon cases unexecuted."})
            );
        }
    }
}

fn send(stream: &mut TcpStream, value: &Value) {
    let body = serde_json::to_vec(value).unwrap();
    write!(stream, "{}:", body.len()).unwrap();
    stream.write_all(&body).unwrap();
}

#[derive(Clone, Copy)]
enum Reply {
    Boolean(bool),
    Exception,
    Malformed,
    Disconnect,
}
#[derive(Clone, Copy)]
struct Scenario {
    native: Option<Reply>,
    first: Option<Reply>,
    second: Option<Reply>,
    recognized: bool,
    allow_no_cmp: bool,
}
impl Default for Scenario {
    fn default() -> Self {
        Self {
            native: None,
            first: Some(Reply::Boolean(false)),
            second: Some(Reply::Boolean(true)),
            recognized: true,
            allow_no_cmp: false,
        }
    }
}
#[derive(Clone, Copy)]
enum Expected {
    Accepted(&'static str),
    NotActioned(&'static str),
    NoCmp,
    AllowedNoCmp,
    Disconnected,
}

fn selection(scenario: Scenario, expected_actors: &[&str], expected: Expected) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let allow_no_cmp = scenario.allow_no_cmp;
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(peer) => break peer,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(std::time::Instant::now() < deadline, "CLI never connected");
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => panic!("accept: {error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        send(
            &mut stream,
            &json!({"from":"root","applicationType":"browser","traits":{},"ua":"Firefox/156"}),
        );
        let top_url = if scenario.native.is_some() {
            "http://fixture.test/bbc.com"
        } else {
            "http://top.test"
        };
        let mut requests = Vec::new();
        while !reader.fill_buf().unwrap().is_empty() {
            let request = recv_from(&mut reader).unwrap();
            requests.push(request);
            let request = requests.last().unwrap();
            let actor = request["to"].as_str().unwrap();
            match request["type"].as_str().unwrap() {
                "watchResources" => send(&mut stream, &json!({"from":actor})),
                "listTabs" => send(
                    &mut stream,
                    &json!({"from":"root","tabs":[{"actor":"tab","selected":true,"url":top_url}]}),
                ),
                "getTarget" => send(
                    &mut stream,
                    &json!({"from":"tab","frame":{"actor":"top","consoleActor":"top-console","url":top_url}}),
                ),
                "getWatcher" => {
                    assert_eq!(request["isServerTargetSwitchingEnabled"], true);
                    send(&mut stream, &json!({"from":"tab","actor":"watcher"}));
                }
                "watchTargets" => {
                    send(&mut stream, &json!({"from":"watcher"}));
                    let first_url = if scenario.recognized {
                        "http://cmp.test/sourcepoint-first"
                    } else {
                        "http://unrecognized.test/first"
                    };
                    let second_url = if scenario.recognized {
                        "http://cmp.test/sourcepoint-second"
                    } else {
                        "http://unrecognized.test/second"
                    };
                    for (id, top, url, has_console) in [
                        ("top", true, top_url, true),
                        ("first", false, first_url, scenario.first.is_some()),
                        ("second", false, second_url, scenario.second.is_some()),
                    ] {
                        let mut target = json!({"actor":id,"targetType":"frame","isTopLevelTarget":top,"url":url});
                        if has_console {
                            target["consoleActor"] = json!(format!("{id}-console"));
                        }
                        send(
                            &mut stream,
                            &json!({"from":"watcher","type":"target-available-form","target":target}),
                        );
                    }
                }
                "evaluateJSAsync" => {
                    assert!(request.get("frameActor").is_none());
                    let reply = match actor {
                        "top-console" => scenario.native,
                        "first-console" => scenario.first,
                        "second-console" => scenario.second,
                        _ => panic!("unexpected evaluation actor {actor}"),
                    }
                    .expect("console exists for attempted action");
                    let js = request["text"].as_str().unwrap();
                    assert!(js.contains("return false"));
                    assert!(js.contains("target.click();\n  return true;"));
                    if matches!(reply, Reply::Disconnect) {
                        stream.shutdown(std::net::Shutdown::Both).unwrap();
                        break;
                    }
                    send(&mut stream, &json!({"from":actor,"resultID":"result"}));
                    let mut response = json!({"from":actor,"type":"evaluationResult","resultID":"result","result":match reply {Reply::Boolean(value)=>json!(value),Reply::Malformed=>json!("accepted"),Reply::Exception=>json!(true),Reply::Disconnect=>unreachable!()}});
                    if matches!(reply, Reply::Exception) {
                        response["exception"] =
                            json!({"type":"object","actor":"error","class":"Error"});
                        response["hasException"] = json!(true);
                    }
                    send(&mut stream, &response);
                }
                other => panic!("unexpected {other}: {request}"),
            }
        }
        requests
    });
    let home = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff-rdp"));
    cmd.args([
        "--host",
        "127.0.0.1",
        "--no-daemon",
        "--port",
        &port.to_string(),
        "--timeout",
        "3000",
        "consent",
        "accept",
    ])
    .env("FF_RDP_HOME", home.path());
    if allow_no_cmp {
        cmd.arg("--allow-no-cmp");
    }
    let output = cmd.output().unwrap();
    let requests = server.join().unwrap();
    eprintln!(
        "275 actual CLI wait={} stdout={} stderr={} requests={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        json!(requests)
    );
    let actors: Vec<_> = requests
        .iter()
        .filter(|r| r["type"] == "evaluateJSAsync")
        .map(|r| r["to"].as_str().unwrap())
        .collect();
    assert_eq!(
        actors, expected_actors,
        "exact action order and terminal stopping"
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    match expected {
        Expected::Accepted(cmp) => {
            assert!(
                output.status.success(),
                "{}",
                super::support::output_note(&output)
            );
            assert_eq!(value["results"]["cmp"], cmp);
            assert_eq!(value["results"]["status"], "accepted");
            assert_eq!(value["results"]["action"], "accepted");
        }
        Expected::NotActioned(cmp) => {
            assert_eq!(output.status.code(), Some(1));
            assert_eq!(value["cmp"], cmp);
            assert!(value["action"].is_null());
            assert_eq!(value["status"], "detected_not_actioned");
            assert_eq!(value["error_type"], "consent_not_actioned");
            let error = value["error"].as_str().unwrap();
            assert!(error.contains("acceptance could not be confirmed"));
            assert!(!error.contains("banner is still"));
            assert!(!error.contains("dom --frames"));
            assert!(error.contains("click --frame"));
        }
        Expected::NoCmp => {
            assert_eq!(output.status.code(), Some(1));
            assert!(value["cmp"].is_null());
            assert!(value["action"].is_null());
            assert_eq!(value["status"], "no_cmp_detected");
            assert_eq!(value["error_type"], "consent_no_cmp");
        }
        Expected::AllowedNoCmp => {
            assert!(
                output.status.success(),
                "{}",
                super::support::output_note(&output)
            );
            assert!(value["results"]["cmp"].is_null());
            assert!(value["results"]["action"].is_null());
            assert_eq!(value["results"]["status"], "no_cmp_detected");
        }
        Expected::Disconnected => {
            assert_eq!(output.status.code(), Some(6));
            assert!(
                matches!(
                    value["error_type"].as_str(),
                    Some("Transport" | "RemoteClosed")
                ),
                "{value}"
            );
            assert!(value.get("status").is_none());
        }
    }
    assert_eq!(
        requests
            .iter()
            .filter(|r| r["type"] == "watchTargets")
            .count(),
        1
    );
}

#[test]
fn earlier_missing_control_does_not_hide_later_action() {
    selection(
        Scenario::default(),
        &["first-console", "second-console"],
        Expected::Accepted("sourcepoint"),
    );
}
#[test]
fn earlier_missing_console_does_not_hide_later_action() {
    selection(
        Scenario {
            first: None,
            ..Scenario::default()
        },
        &["second-console"],
        Expected::Accepted("sourcepoint"),
    );
}
#[test]
fn all_recognized_misses_remain_failure() {
    selection(
        Scenario {
            second: Some(Reply::Boolean(false)),
            ..Scenario::default()
        },
        &["first-console", "second-console"],
        Expected::NotActioned("sourcepoint"),
    );
}
#[test]
fn no_cmp_is_distinct_and_has_no_action_request() {
    selection(
        Scenario {
            recognized: false,
            ..Scenario::default()
        },
        &[],
        Expected::NoCmp,
    );
}
#[test]
fn allow_no_cmp_only_accepts_absence() {
    selection(
        Scenario {
            recognized: false,
            allow_no_cmp: true,
            ..Scenario::default()
        },
        &[],
        Expected::AllowedNoCmp,
    );
    selection(
        Scenario {
            second: Some(Reply::Boolean(false)),
            allow_no_cmp: true,
            ..Scenario::default()
        },
        &["first-console", "second-console"],
        Expected::NotActioned("sourcepoint"),
    );
}
#[test]
fn native_success_precedes_all_iframe_actions() {
    selection(
        Scenario {
            native: Some(Reply::Boolean(true)),
            ..Scenario::default()
        },
        &["top-console"],
        Expected::Accepted("bbc"),
    );
}
#[test]
fn native_pre_action_miss_allows_iframe_selection() {
    selection(
        Scenario {
            native: Some(Reply::Boolean(false)),
            ..Scenario::default()
        },
        &["top-console", "first-console", "second-console"],
        Expected::Accepted("sourcepoint"),
    );
}
#[test]
fn native_pre_action_miss_without_recognized_iframe_is_no_cmp() {
    selection(
        Scenario {
            native: Some(Reply::Boolean(false)),
            recognized: false,
            ..Scenario::default()
        },
        &["top-console"],
        Expected::NoCmp,
    );
}
#[test]
fn first_success_stops_before_a_second_action() {
    selection(
        Scenario {
            first: Some(Reply::Boolean(true)),
            ..Scenario::default()
        },
        &["first-console"],
        Expected::Accepted("sourcepoint"),
    );
}
#[test]
fn iframe_exception_stops_with_unconfirmed_acceptance() {
    selection(
        Scenario {
            first: Some(Reply::Exception),
            ..Scenario::default()
        },
        &["first-console"],
        Expected::NotActioned("sourcepoint"),
    );
}
#[test]
fn iframe_malformed_result_stops_with_unconfirmed_acceptance() {
    selection(
        Scenario {
            first: Some(Reply::Malformed),
            ..Scenario::default()
        },
        &["first-console"],
        Expected::NotActioned("sourcepoint"),
    );
}
#[test]
fn iframe_transport_failure_propagates_without_fallback() {
    selection(
        Scenario {
            first: Some(Reply::Disconnect),
            ..Scenario::default()
        },
        &["first-console"],
        Expected::Disconnected,
    );
}
#[test]
fn native_exception_never_attempts_an_iframe() {
    selection(
        Scenario {
            native: Some(Reply::Exception),
            ..Scenario::default()
        },
        &["top-console"],
        Expected::NotActioned("bbc"),
    );
}
#[test]
fn native_malformed_result_never_attempts_an_iframe() {
    selection(
        Scenario {
            native: Some(Reply::Malformed),
            ..Scenario::default()
        },
        &["top-console"],
        Expected::NotActioned("bbc"),
    );
}
#[test]
fn native_transport_failure_propagates_without_iframe_action() {
    selection(
        Scenario {
            native: Some(Reply::Disconnect),
            ..Scenario::default()
        },
        &["top-console"],
        Expected::Disconnected,
    );
}

#[test]
fn fixture_multiroute_dispatch_and_actual_joins() {
    use std::io::Read;
    let mut server = fixture::Fixture::start(
        "multiroute",
        std::collections::HashMap::from([
            ("/first".into(), "first body".into()),
            ("/second".into(), "second body".into()),
        ]),
        fixture::Fault::None,
    )
    .unwrap();
    for (path, body) in [
        ("/first", "first body"),
        ("/second?run=1", "second body"),
        ("/absent", ""),
    ] {
        let mut socket =
            TcpStream::connect(server.base_url().trim_start_matches("http://")).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(socket, "GET {path} HTTP/1.1\r\n\r\n").unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).unwrap();
        assert!(response.ends_with(body));
        assert!(response.starts_with(if path == "/absent" {
            "HTTP/1.1 404"
        } else {
            "HTTP/1.1 200"
        }));
    }
    let report = server.finish();
    assert!(report.success);
    assert_eq!(report.workers, 3);
    let rows = server.journal().snapshot();
    assert_eq!(
        rows.iter()
            .filter(|r| r["stage"] == "worker-join" && r["success"] == true)
            .count(),
        3
    );
    assert_eq!(
        rows.iter()
            .filter(|r| r["stage"] == "accept-join" && r["success"] == true)
            .count(),
        1
    );
}
