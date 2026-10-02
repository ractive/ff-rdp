//! Bounded, non-Firefox controls for the two closing1 follow-ups.
//! Browser replies are scripted; CLI command parsing, ref resolution,
//! navigation, page collection and home assembly execute their real paths.
//! Every command opens its own connection, so the scripted peer accepts one
//! connection per command and keeps its browser state across them.
#[path = "../common/home_ref_flow.rs"]
mod home_ref_flow;

use std::io::{BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use ff_rdp_core::transport::{encode_frame, recv_from};
use serde_json::{Value, json};

const ORIGIN: &str = "https://fixture.test/";
const DESTINATION: &str = "https://fixture.test/clicked";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scenario {
    Home,
    WrongHeading,
    Unready,
}

fn send(stream: &mut TcpStream, packet: &Value) {
    stream
        .write_all(encode_frame(&packet.to_string()).as_bytes())
        .expect("scripted peer write");
}

fn target() -> Value {
    json!({"actor":"target", "consoleActor":"console", "innerWindowId":1,
        "browsingContextID":16, "isTopLevelTarget":true, "isPopup":false, "targetType":"frame",
        "url":ORIGIN, "title":"t212 home"})
}

fn evaluate(stream: &mut TcpStream, result: &Value, sequence: u64) {
    let id = sequence.to_string();
    send(stream, &json!({"from":"console","resultID":id}));
    send(
        stream,
        &json!({"from":"console","type":"evaluationResult",
        "resultID":id,"result":result}),
    );
}

fn sentinel(value: &Value) -> Value {
    json!(format!("__FF_RDP_JSON__{value}"))
}

/// One actual acquired peer thread. Cleanup interrupts its owned stream (or
/// wakes its one accept on setup failure), and always joins it.
struct Peer {
    port: u16,
    socket: Arc<Mutex<Option<TcpStream>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Vec<String>>>,
}

impl Peer {
    fn start(scenario: Scenario) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("owned mock listener");
        let port = listener.local_addr().unwrap().port();
        let socket = Arc::new(Mutex::new(None));
        let acquired = Arc::clone(&socket);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            let mut actions = 0;
            let mut destination = false;
            let mut sequence = 0;
            let mut transcript = Vec::new();
            loop {
                let (mut stream, _) = listener.accept().expect("owned peer accept");
                if stopped.load(Ordering::SeqCst) {
                    break;
                }
                *acquired.lock().unwrap() = Some(stream.try_clone().unwrap());
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                send(
                    &mut stream,
                    &json!({"from":"root","applicationType":"browser",
                "traits":{},"ua":"Firefox/156.0"}),
                );
                while let Ok(request) = recv_from(&mut reader) {
                    let kind = request["type"].as_str().expect("request type");
                    let to = request["to"].as_str().unwrap_or("root");
                    match kind {
                        "listTabs" => {
                            let url = if destination { DESTINATION } else { ORIGIN };
                            transcript.push(format!("listTabs:{url}"));
                            send(
                                &mut stream,
                                &json!({"from":"root","tabs":[{
                            "actor":"tab","browsingContextID":16,"selected":true,
                            "url":url,"title":if destination {"t212 clicked"} else {"t212 home"}}]}),
                            );
                        }
                        "getTarget" => {
                            // Transition seam on a direct connection: the first
                            // target acquisition after the click dispatched hands
                            // over the destination document.
                            if actions > 0 && !destination {
                                destination = true;
                                transcript.push("destination-acquired".into());
                            }
                            let mut form = target();
                            if destination {
                                form["url"] = json!(DESTINATION);
                            }
                            send(&mut stream, &json!({"from":"tab","frame":form}));
                        }
                        "getWatcher" => send(&mut stream, &json!({"from":"tab","actor":"watcher"})),
                        "watchTargets" => {
                            send(
                                &mut stream,
                                &json!({"from":"watcher","type":"target-available-form","target":target()}),
                            );
                            send(&mut stream, &json!({"from":"watcher"}));
                        }
                        "watchResources" => send(&mut stream, &json!({"from":"watcher"})),
                        "unwatchResources" | "unwatchTargets" => {
                            transcript.push(kind.to_owned());
                            // Both protocol requests are one-way; invent no reply.
                        }
                        "listFrames" => {
                            // Synchronous transition seam: the first actual target
                            // metadata acquisition after dispatch releases takeover.
                            // A bare click reaches the next home's listTabs first;
                            // --with-page acquires before that home invocation.
                            if actions > 0 && !destination {
                                destination = true;
                                transcript.push("destination-acquired".into());
                            }
                            send(
                                &mut stream,
                                &json!({"from":to,"frames":[{"isTopLevel":true,
                            "url":if destination {DESTINATION} else {ORIGIN}}]}),
                            );
                        }
                        "evaluateJSAsync" => {
                            sequence += 1;
                            let js = request["text"].as_str().expect("evaluation source");
                            let value = if js.contains("headings") && js.contains("interactive") {
                                let heading = if destination {
                                    if scenario == Scenario::WrongHeading {
                                        "Wrong destination"
                                    } else {
                                        "Arrived"
                                    }
                                } else {
                                    "Ambient context"
                                };
                                transcript.push(format!("page:{heading}"));
                                sentinel(
                                    &json!({"headings":[{"level":1,"text":heading}],"landmarks":[],
                                "interactive":if destination {json!([])} else {json!([{"role":"link",
                                    "name":"Follow me","href":"/clicked","ref":"e1"}])},
                                "reader_missing":false,"readerable":false,"text":"","source":"fallback"}),
                                )
                            } else if js.contains("dispatchEvent") {
                                assert!(
                                    js.contains("data-ffrdp-ref"),
                                    "the click resolved the in-page ref"
                                );
                                actions += 1;
                                assert_eq!(actions, 1);
                                transcript.push("click-resolved-ref".into());
                                // Latch the announcement before the click's eval
                                // completion. No target acquisition is performed here.
                                send(
                                    &mut stream,
                                    &json!({"from":"target","type":"tabNavigated",
                                "state":"start","url":DESTINATION}),
                                );
                                sentinel(&json!({"clicked":true,"matched":true,"reachable":true,
                                "tag":"A","text":"Follow me","obscured_by":null}))
                            } else if js == "document.readyState === 'complete'" {
                                transcript.push("destination-readiness".into());
                                json!(scenario != Scenario::Unready)
                            } else if js.contains("querySelectorAll") {
                                json!(r#"{"matchCount":1,"hidden":false}"#)
                            } else if js.contains("getComputedStyle") {
                                sentinel(&json!({"ready":true,"tag":"A","text":"Follow me"}))
                            } else if js.contains("getBoundingClientRect") {
                                json!("[0,0,10,10]")
                            } else {
                                panic!("unhandled home/click eval: {js}");
                            };
                            evaluate(&mut stream, &value, sequence);
                        }
                        "startListeners" => {
                            send(&mut stream, &json!({"from":to,"startedListeners":[]}));
                        }
                        "release" => {}
                        other => panic!("unexpected scripted peer request: {other}: {request}"),
                    }
                }
            }
            transcript.push("peer-return".into());
            transcript
        });
        Self {
            port,
            socket,
            stop,
            worker: Some(worker),
        }
    }

    fn finish(&mut self) -> Vec<String> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(socket) = self.socket.lock().unwrap().as_ref() {
            let _ = socket.shutdown(Shutdown::Both);
        }
        // Wake our own accept so the loop observes `stop`.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        self.worker
            .take()
            .expect("peer owned once")
            .join()
            .expect("peer actual join")
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        if self.worker.is_some() {
            // Panic cleanup must not turn an unjoined endpoint into a pass.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.finish()));
        }
    }
}

fn command(port: u16, home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ff-rdp"));
    command
        .env("FF_RDP_HOME", home)
        .env_remove("RUST_LOG")
        .env_remove("FF_RDP_LIVE_TESTS")
        .env_remove("FF_RDP_LIVE_NETWORK_TESTS")
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--timeout",
            "1000",
        ]);
    command
}

fn archive(label: &str, name: &str, bytes: &[u8]) {
    if let Some(root) = std::env::var_os("FF_RDP_HOME") {
        let directory = PathBuf::from(root).join("closing-284-controls").join(label);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(name), bytes).unwrap();
    }
}

fn run_command(command: &mut Command, label: &str, index: usize) -> Output {
    let program = command.get_program().to_string_lossy().into_owned();
    let argv: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let environment: Vec<(String, Option<String>)> = command
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    let child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("actual command spawn");
    let pid = child.id();
    let output = child
        .wait_with_output()
        .expect("actual command output wait");
    archive(label, &format!("command-{index}.json"),
        serde_json::to_vec_pretty(&json!({"program":program,"argv":argv,"environment":environment,
            "pid":pid,"actual_wait":true,"exit":output.status.code(),
            "stdout":String::from_utf8_lossy(&output.stdout),"stderr":String::from_utf8_lossy(&output.stderr)})).unwrap().as_slice());
    output
}

fn panic_text(error: &(dyn std::any::Any + Send)) -> &str {
    error
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| error.downcast_ref::<&str>().copied())
        .unwrap_or("non-string panic")
}

fn home_control(scenario: Scenario, expected_rejection: Option<&str>) {
    let label = format!("{scenario:?}");
    let home = tempfile::tempdir().unwrap();
    let mut peer = Peer::start(scenario);
    let mut outputs = Vec::new();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        home_ref_flow::check(ORIGIN, |args| {
            let output = run_command(
                command(peer.port, home.path()).args(args),
                &label,
                outputs.len(),
            );
            let success = output.status.success();
            let value = serde_json::from_slice::<Value>(&output.stdout);
            outputs.push(output);
            assert!(success, "actual command failed: {:?}", outputs.last());
            value.expect("actual CLI JSON")
        });
    }));
    // Even the expected-before URL assertion is deferred until these actual
    // joins complete; every failed transcript remains available to the runner.
    let transcript = peer.finish();
    archive(
        &label,
        "peer.json",
        serde_json::to_vec_pretty(&transcript).unwrap().as_slice(),
    );
    let click = transcript
        .iter()
        .position(|s| s == "click-resolved-ref")
        .expect("real ref click");
    let acquired = transcript
        .iter()
        .position(|s| s == "destination-acquired")
        .expect("destination acquisition");
    let listed = transcript
        .iter()
        .enumerate()
        .skip(click + 1)
        .find(|(_, s)| s.starts_with("listTabs:"))
        .map(|(i, _)| i);
    if let Err(error) = &result {
        if panic_text(error.as_ref())
            .contains("the click must have followed the link the ref pointed at")
        {
            assert!(
                listed.is_some_and(|i| i < acquired),
                "before requires old listTabs before acquisition: {transcript:?}"
            );
            assert!(transcript.iter().any(|s| s == "page:Arrived"));
        }
    } else {
        assert!(
            acquired < listed.expect("second home listing"),
            "destination precedes second home: {transcript:?}"
        );
    }
    match (result, expected_rejection) {
        (Ok(()), None) => {}
        (Err(error), Some(needle)) => assert!(
            panic_text(error.as_ref()).contains(needle),
            "wrong negative oracle: {}",
            panic_text(error.as_ref())
        ),
        (Err(error), None) => std::panic::resume_unwind(error),
        (Ok(()), Some(needle)) => panic!("negative accepted instead of {needle}"),
    }
}

#[test]
fn home_ref_original_sequence_requires_destination() {
    home_control(Scenario::Home, None);
}

#[test]
fn home_ref_rejects_wrong_destination() {
    home_control(
        Scenario::WrongHeading,
        Some("click --ref must reach the fixture destination before reading home"),
    );
}

#[test]
fn home_ref_rejects_unready_destination() {
    home_control(
        Scenario::Unready,
        Some("the destination must be ready before the subsequent home observation"),
    );
}
