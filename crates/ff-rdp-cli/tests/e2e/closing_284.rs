//! Bounded, non-Firefox controls for the two closing1 follow-ups.
//! Browser replies are scripted; CLI, daemon auth/ref storage, command parsing,
//! navigation, page collection and home assembly execute their real paths.
#[path = "../common/home_ref_flow.rs"]
mod home_ref_flow;

use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex, mpsc};
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
    Events,
    Fallback,
}

impl Scenario {
    fn is_navigation(self) -> bool {
        matches!(self, Self::Events | Self::Fallback)
    }
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
    worker: Option<JoinHandle<Vec<String>>>,
}

impl Peer {
    fn start(scenario: Scenario) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("owned mock listener");
        let port = listener.local_addr().unwrap().port();
        let socket = Arc::new(Mutex::new(None));
        let acquired = Arc::clone(&socket);
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("owned peer accept");
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
            let mut actions = 0;
            let mut destination = false;
            let mut teardown = false;
            let mut sequence = 0;
            let mut transcript = Vec::new();
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
                        teardown = true;
                        transcript.push(kind.to_owned());
                        // Both protocol requests are one-way; invent no reply.
                    }
                    "listFrames" => {
                        // Synchronous transition seam: the first actual target
                        // metadata acquisition after dispatch releases takeover.
                        // A bare click reaches the next home's listTabs first;
                        // --with-page acquires before that home invocation.
                        if actions > 0 && !scenario.is_navigation() {
                            destination = true;
                            transcript.push("destination-acquired".into());
                        }
                        send(
                            &mut stream,
                            &json!({"from":to,"frames":[{"isTopLevel":true,
                            "url":if destination {DESTINATION} else {ORIGIN}}]}),
                        );
                    }
                    "navigateTo" => {
                        assert!(scenario.is_navigation());
                        actions += 1;
                        assert_eq!(actions, 1, "dispatch is never retried");
                        assert_eq!(request["url"], DESTINATION);
                        destination = scenario == Scenario::Events;
                        transcript.push("navigate".into());
                        send(&mut stream, &json!({"from":"target"}));
                        send(
                            &mut stream,
                            &json!({"from":"watcher","type":"resources-available-array",
                            "array":[["network-event",[{"resourceId":7,"actor":"network7",
                            "url":DESTINATION,"method":"GET","cause":{"type":"document"},
                            "isNavigationRequest":true,"browsingContextID":16,"innerWindowId":1}]]]}),
                        );
                        send(
                            &mut stream,
                            &json!({"from":"watcher","type":"resources-updated-array",
                            "array":[["network-event",[{"resourceId":7,"resourceUpdates":{"status":"200"}}]]]}),
                        );
                        if scenario == Scenario::Events {
                            send(
                                &mut stream,
                                &json!({"from":"watcher","type":"resources-available-array",
                                "array":[["document-event",[{"name":"dom-complete","url":DESTINATION}]]]}),
                            );
                        }
                    }
                    "evaluateJSAsync" => {
                        sequence += 1;
                        let js = request["text"].as_str().expect("evaluation source");
                        let value = if scenario.is_navigation() {
                            let committed = destination || teardown;
                            if js.contains("readyState: document.readyState") {
                                json!(json!({"readyState":"complete","href":if committed {DESTINATION} else {ORIGIN},
                                    "epoch":if committed {2.0} else {1.0}}).to_string())
                            } else if js.contains("var h = window.location.href") {
                                if committed {
                                    json!(DESTINATION)
                                } else {
                                    Value::Null
                                }
                            } else if js.contains("performance.timing.navigationStart") {
                                json!(if committed { 2.0 } else { 1.0 })
                            } else if js.contains("window.location.href") {
                                json!(if committed { DESTINATION } else { ORIGIN })
                            } else {
                                panic!("unhandled navigation eval: {js}");
                            }
                        } else if js.contains("headings") && js.contains("interactive") {
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
                                    "name":"Follow me","href":"/clicked","__resolver":"#go"}])},
                                "reader_missing":false,"readerable":false,"text":"","source":"fallback"}),
                            )
                        } else if js.contains("dispatchEvent") {
                            assert!(js.contains("#go"), "real daemon resolved the minted ref");
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
            transcript.push("peer-return".into());
            transcript
        });
        Self {
            port,
            socket,
            worker: Some(worker),
        }
    }

    fn finish(&mut self) -> Vec<String> {
        if let Some(socket) = self.socket.lock().unwrap().as_ref() {
            let _ = socket.shutdown(Shutdown::Both);
        } else {
            // Unblock only our own accept if a command failed before connect.
            let _ = TcpStream::connect(("127.0.0.1", self.port));
        }
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

struct Daemon {
    child: Option<Child>,
    stderr: Option<JoinHandle<String>>,
    stdout: tempfile::NamedTempFile,
}

impl Daemon {
    fn start(port: u16, home: &Path) -> Self {
        // A regular file cannot backpressure the daemon like an undrained pipe.
        let stdout = tempfile::NamedTempFile::new_in(home).expect("owned daemon stdout file");
        let mut child = command(port, home)
            .arg("_daemon")
            .stdout(stdout.reopen().expect("daemon stdout writer"))
            .stderr(Stdio::piped())
            .spawn()
            .expect("real daemon spawn");
        let stderr = child.stderr.take().unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let stderr = std::thread::spawn(move || {
            let mut text = String::new();
            for line in BufReader::new(stderr).lines() {
                let line = line.expect("daemon stderr");
                if line.starts_with("daemon: listening on port ") {
                    let _ = ready_tx.send(());
                }
                text.push_str(&line);
                text.push('\n');
            }
            text
        });
        let guard = Self {
            child: Some(child),
            stderr: Some(stderr),
            stdout,
        };
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("actual daemon listening marker");
        guard
    }

    fn finish(&mut self) -> (Vec<u8>, String) {
        let status = self
            .child
            .take()
            .unwrap()
            .wait()
            .expect("actual daemon wait");
        let stderr = self
            .stderr
            .take()
            .unwrap()
            .join()
            .expect("stderr reader actual join");
        // Read only after the actual child wait and existing stderr-reader join.
        let stdout = std::fs::read(self.stdout.path()).expect("read actual daemon stdout");
        assert!(
            status.success(),
            "daemon exit {status}: stdout={} stderr={stderr}",
            String::from_utf8_lossy(&stdout)
        );
        assert!(
            stderr.contains("shut down after joining all acquired workers"),
            "stdout={} stderr={stderr}",
            String::from_utf8_lossy(&stdout)
        );
        (stdout, stderr)
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(stderr) = self.stderr.take() {
            let _ = stderr.join();
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

// Keep raw stderr intact in the archive; strip only terminal decoration for
// field assertions, since production installs the real formatter.
fn plain_trace(text: &str) -> String {
    let mut result = String::new();
    let mut escape = false;
    for ch in text.chars() {
        if ch == '\u{1b}' {
            escape = true;
        } else if escape {
            if ch == 'm' {
                escape = false;
            }
        } else {
            result.push(ch);
        }
    }
    result
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
    let mut daemon = Daemon::start(peer.port, home.path());
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
    let stop = run_command(
        command(peer.port, home.path()).args(["daemon", "stop"]),
        &label,
        outputs.len(),
    );
    assert!(stop.status.success(), "daemon stop failed: {stop:?}");
    let daemon_pid = daemon.child.as_ref().unwrap().id();
    let (stdout, stderr) = daemon.finish();
    let transcript = peer.finish();
    archive(&label, "daemon.stdout", &stdout);
    archive(&label, "daemon.stderr", stderr.as_bytes());
    archive(
        &label,
        "peer.json",
        serde_json::to_vec_pretty(&transcript).unwrap().as_slice(),
    );
    archive(
        &label,
        "joins.json",
        serde_json::to_vec_pretty(&json!({"daemon_pid":daemon_pid,
        "daemon_waited":true,"stderr_joined":true,"peer_joined":true}))
        .unwrap()
        .as_slice(),
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

#[test]
fn navigate_diagnostics_execute_real_event_and_fallback_paths() {
    for scenario in [Scenario::Events, Scenario::Fallback] {
        let label = format!("166-{scenario:?}");
        let home = tempfile::tempdir().unwrap();
        let mut peer = Peer::start(scenario);
        // A real CLI child installs main's tracing subscriber. RUST_LOG on the
        // surrounding libtest process alone is deliberately not our evidence.
        let output = run_command(command(peer.port, home.path())
            .env("RUST_LOG", "ff_rdp_core::transport=trace,ff_rdp_cli::navigation_166_diagnostic=debug,ff_rdp_cli::commands::navigate=debug")
            .args(["--no-daemon","navigate",DESTINATION]), &label, 0);
        let transcript = peer.finish();
        archive(
            &label,
            "peer.json",
            serde_json::to_vec_pretty(&transcript).unwrap().as_slice(),
        );
        archive(
            &label,
            "joins.json",
            b"{\"command_waited\":true,\"peer_joined\":true}\n",
        );
        assert!(
            output.status.success(),
            "actual navigate failed: {output:?}"
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["results"]["status"], 200);
        assert_eq!(value["results"]["committed_url"], DESTINATION);
        let stderr = plain_trace(&String::from_utf8_lossy(&output.stderr));
        for phase in [
            "events_begin",
            "events_end_before_teardown",
            "after_stream_stop_before_gc",
            "after_gc_before_unsubscribe",
            "after_unsubscribe_before_unwatch_targets",
            "after_teardown_and_timeout_restore",
            "readiness_before_neterror_check",
            "final_after_resolution",
        ] {
            assert!(
                stderr.contains(phase),
                "missing real diagnostic {phase}: {}",
                super::support::output_note(&output)
            );
        }
        assert!(
            stderr.contains("166 retained status evidence"),
            "missing retained status evidence: {}",
            super::support::output_note(&output)
        );
        assert!(
            stderr.contains("context=Some(16)") && stderr.contains("outgoing_window=Some(1)"),
            "ownership: {}",
            super::support::output_note(&output)
        );
        assert!(
            stderr.contains("resources-available-array") && stderr.contains("resourceId"),
            "raw receive trace: {}",
            super::support::output_note(&output)
        );
        if scenario == Scenario::Fallback {
            assert!(
                stderr.contains("fallback_before_direct_refresh")
                    && stderr.contains("fallback_after_direct_refresh"),
                "missing fallback phases: {}",
                super::support::output_note(&output)
            );
            assert!(
                stderr.contains("used_fallback=true"),
                "fallback branch: {}",
                super::support::output_note(&output)
            );
            assert!(
                stderr.contains("requests=[(7,") && stderr.contains("statuses=[(7, 200)]"),
                "retained candidates: {}",
                super::support::output_note(&output)
            );
        } else {
            assert!(
                stderr.contains("used_fallback=false"),
                "event branch: {}",
                super::support::output_note(&output)
            );
            assert!(
                !stderr.contains("fallback_before_direct_refresh"),
                "unexpected fallback phase: {}",
                super::support::output_note(&output)
            );
        }
    }
}
