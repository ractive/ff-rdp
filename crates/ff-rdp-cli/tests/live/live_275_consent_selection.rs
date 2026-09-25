//! Actual-document consent selection. Local fixtures are not historical Guardian reproductions.
//! daemon-parity: consent_selection_daemon covers the proxied path.
#[path = "../support/fixture275_selection.rs"]
mod fixture;
#[path = "../support/initial_tab275.rs"]
mod initial_tab;
#[path = "../support/consent275_oracle.rs"]
mod oracle;

use crate::common::{IsolatedLiveFirefox, ff_rdp_bin, output_note};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Output, Stdio};

// Record at the product's actual selector calls, without replacing its result
// or click implementation. Same-origin parent communication is synchronous.
// This controlled instrumentation is not a nonperturbing site capture.
const FRAME_SCRIPT: &str = r#"
window.clickCount=0;
const originalAll=document.querySelectorAll.bind(document);
function sample(kind) {
  const el=document.getElementById('accept');
  const rect=el ? el.getBoundingClientRect() : null;
  return {kind,id:document.body.dataset.id,href:location.href,documentURI:document.documentURI,
    epoch:performance.timeOrigin,readyState:document.readyState,token:document.body.dataset.token,
    controls:Array.from(originalAll('button')).map(e=>({label:e.textContent,disabled:e.disabled})),
    count:window.clickCount,present:!!el,w:rect&&rect.width,h:rect&&rect.height};
}
document.querySelectorAll=function(selector) {
  if(selector==='button, [role="button"], a') parent.proof.push(sample('selector'));
  return originalAll(selector);
};
const accept=document.getElementById('accept');
if(accept) accept.addEventListener('click',()=>{
  parent.proof.push(sample('click-before'));
  window.clickCount++;
  accept.remove();
  parent.proof.push(sample('click-after'));
});
window.snapshot=()=>sample('readback');
"#;

const TOP_SCRIPT: &str = r"
window.proof=[]; window.nativeCount=0;
function nativeSample(kind) {
  const el=document.getElementById('bbccookies-continue-button');
  const r=el?el.getBoundingClientRect():null;
  return {kind,id:'native',href:location.href,documentURI:document.documentURI,
    epoch:performance.timeOrigin,readyState:document.readyState,token:document.body.dataset.token,
    present:!!el,count:window.nativeCount,w:r&&r.width,h:r&&r.height};
}
const originalOne=document.querySelector.bind(document);
document.querySelector=function(selector) {
  if(selector==='#bbccookies-continue-button') proof.push(nativeSample('selector'));
  return originalOne(selector);
};
window.addEventListener('DOMContentLoaded',()=>{
  const el=document.getElementById('bbccookies-continue-button');
  if(el)el.addEventListener('click',()=>{
    proof.push(nativeSample('click-before'));nativeCount++;el.remove();proof.push(nativeSample('click-after'));
  });
});
window.readProof=()=>({href:location.href,epoch:performance.timeOrigin,readyState:document.readyState,
  token:document.body.dataset.token,proof:window.proof,native:nativeSample('readback'),
  frames:Array.from(document.getElementsByTagName('iframe')).map(f=>f.contentWindow.snapshot())});
";

#[derive(Clone, Copy)]
enum Case {
    Later,
    First,
    AllMiss,
    NoCmp,
    Native,
}
impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::Later => "later",
            Self::First => "first",
            Self::AllMiss => "all-miss",
            Self::NoCmp => "no-cmp",
            Self::Native => "bbc.com-native",
        }
    }
    fn first_accepts(self) -> bool {
        matches!(self, Self::First | Self::Native)
    }
    fn second_accepts(self) -> bool {
        matches!(self, Self::Later | Self::First | Self::Native)
    }
    fn frame_path(self, id: &str) -> String {
        format!(
            "/{}/{}-{id}",
            self.name(),
            if matches!(self, Self::NoCmp) {
                "unrecognized"
            } else {
                "sourcepoint"
            }
        )
    }
}

fn pages() -> HashMap<String, String> {
    let mut routes = HashMap::new();
    for case in [
        Case::Later,
        Case::First,
        Case::AllMiss,
        Case::NoCmp,
        Case::Native,
    ] {
        let name = case.name();
        for (id, accepts) in [
            ("first", case.first_accepts()),
            ("second", case.second_accepts()),
        ] {
            let button = if accepts {
                "<button id='accept'>Accept all</button>"
            } else {
                "<button>Reject only</button>"
            };
            routes.insert(case.frame_path(id),format!("<!doctype html><html><body data-id='{id}' data-token='{name}-{id}'>{button}<script>{FRAME_SCRIPT}</script></body></html>"));
        }
        let native = if matches!(case, Case::Native) {
            "<button id='bbccookies-continue-button'>Accept cookies</button>"
        } else {
            ""
        };
        routes.insert(format!("/{name}"),format!("<!doctype html><html><head><script>{TOP_SCRIPT}</script></head><body data-token='{name}'>{native}<iframe src='{}'></iframe><iframe src='{}'></iframe></body></html>",case.frame_path("first"),case.frame_path("second")));
    }
    routes
}

fn output(command: &mut Command, label: &str) -> Output {
    let dir = std::env::var_os("FF_RDP_275_SELECTION_DIR").map(std::path::PathBuf::from);
    let temporary = dir
        .is_none()
        .then(|| tempfile::tempdir().expect("capture directory"));
    let dir = dir
        .as_deref()
        .unwrap_or_else(|| temporary.as_ref().unwrap().path());
    let stdout = dir.join(format!("{label}.stdout"));
    let stderr = dir.join(format!("{label}.stderr"));
    let open = |p: &std::path::Path| {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(p)
            .expect("exclusive command output")
    };
    command
        .stdout(Stdio::from(open(&stdout)))
        .stderr(Stdio::from(open(&stderr)));
    eprintln!("275 command stage={label} command={command:?}");
    let mut child = command.spawn().expect("spawn command");
    eprintln!("275 command stage={label} pid={}", child.id());
    let status = child.wait().expect("actual command wait");
    let result = Output {
        status,
        stdout: std::fs::read(stdout).expect("read stdout"),
        stderr: std::fs::read(stderr).expect("read stderr"),
    };
    eprintln!(
        "275 command stage={label} actual_wait={status} {}",
        output_note(&result)
    );
    result
}

fn command(session: &IsolatedLiveFirefox, daemon: bool) -> Command {
    let mut cmd = session.command();
    cmd.args([
        "--host",
        "127.0.0.1",
        "--port",
        &session.firefox().port().to_string(),
        "--timeout",
        "30000",
    ]);
    if !daemon {
        cmd.arg("--no-daemon");
    }
    cmd
}

fn readback(session: &IsolatedLiveFirefox, daemon: bool, label: &str) -> Value {
    let out = output(
        command(session, daemon).args(["eval", "JSON.stringify(window.readProof())"]),
        label,
    );
    assert!(out.status.success(), "{}", output_note(&out));
    let v: Value = serde_json::from_slice(&out.stdout).expect("eval envelope");
    serde_json::from_str(v["results"].as_str().expect("serialized fixture state"))
        .expect("fixture state")
}

fn run(daemon: bool) {
    const PHASE: &str = "after";
    assert_eq!(std::env::var("FF_RDP_LIVE_TESTS").as_deref(), Ok("1"));
    let mut fixture =
        fixture::Fixture::start("selection", pages(), fixture::Fault::None).expect("fixture start");
    let mut session = IsolatedLiveFirefox::launch(&ff_rdp_bin()).expect("owned Firefox launch");
    eprintln!("275 launch={:?}", session.receipt());
    initial_tab::observe(session.receipt().pid, session.receipt().port).unwrap_or_else(|failure| {
        panic!(
            "initial tab prerequisite: {} {:?}",
            failure.reason, failure.report
        )
    });
    if daemon {
        session.with_daemon().expect("daemon autostart");
    }
    let cases = [
        Case::Later,
        Case::First,
        Case::AllMiss,
        Case::NoCmp,
        Case::Native,
    ];
    for case in cases {
        let prefix = format!(
            "{}-{}",
            if daemon { "daemon" } else { "direct" },
            case.name()
        );
        let url = format!("{}/{}", fixture.base_url(), case.name());
        let nav = output(
            command(&session, daemon).args(["navigate", &url]),
            &format!("{prefix}-navigate"),
        );
        assert!(nav.status.success(), "{}", output_note(&nav));
        let before = readback(&session, daemon, &format!("{prefix}-before"));
        oracle::qualify_before(&before, case.name(), &fixture.base_url())
            .expect("qualified static fixture inputs");
        let action = output(
            command(&session, daemon)
                .env(
                    "RUST_LOG",
                    "ff_rdp_cli::frame_targets=debug,ff_rdp_core::transport=trace",
                )
                .env("FF_RDP_TRACE_RAW", "1")
                .args(["consent", "accept"]),
            &format!("{prefix}-consent"),
        );
        let after = readback(&session, daemon, &format!("{prefix}-after"));
        eprintln!(
            "275 boundary {}",
            json!({"phase":PHASE,"route":if daemon{"daemon"}else{"direct"},"case":case.name(),"before":before,"after":after})
        );
        let logs = String::from_utf8_lossy(&action.stderr);
        let expected =
            oracle::expectation(&before, case.name(), &fixture.base_url(), &logs, daemon)
                .expect("one qualified actual target enumeration");
        eprintln!(
            "275 expectation {}",
            json!({"case":case.name(),"route_daemon":daemon,
            "before_href":before["href"],"before_epoch":before["epoch"],"expected":expected.record()})
        );
        let envelope: Value = serde_json::from_slice(&action.stdout).expect("consent envelope");
        oracle::validate(&before, &after, &expected, action.status.code(), &envelope)
            .unwrap_or_else(|error| panic!("{error}: {}", output_note(&action)));
    }
    session.finish().expect("owned browser cleanup");
    let report = fixture.finish();
    assert!(report.success, "fixture joins: {report:?}");
    assert!(report.workers > 0);
    let rows = fixture.journal().snapshot();
    assert_eq!(
        rows.iter().filter(|r| r["stage"] == "worker-join").count(),
        report.workers
    );
    let _ = std::io::stderr()
        .lock()
        .write_all(format!("275 completed phase={PHASE} route_daemon={daemon}\n").as_bytes());
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn consent_selection_direct() {
    run(false);
}
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn consent_selection_daemon() {
    run(true);
}
