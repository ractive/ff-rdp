//! Private 275 fixture with cancellable nonblocking I/O and retained actual joins.
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

type Worker = JoinHandle<Result<&'static str, String>>;
#[derive(Clone, Default)]
pub(super) struct Journal {
    rows: Arc<Mutex<Vec<Value>>>,
    write_failed: Arc<AtomicBool>,
}
impl Journal {
    fn record(&self, row: &Value) {
        self.rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(row.clone());
        // Serialize before taking stderr: one complete buffer avoids streaming
        // JSON fragments between other threads' panic/harness output.
        let line = format!("275-fixture {row}\n");
        if std::io::stderr().lock().write_all(line.as_bytes()).is_err() {
            self.write_failed.store(true, Ordering::Release);
        }
    }
    pub(super) fn snapshot(&self) -> Vec<Value> {
        self.rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}
#[derive(Clone, Copy)]
// Faults are constructed by the browser-free e2e target; this shared private
// module is also compiled into the live target, which always uses None.
#[allow(dead_code)]
pub(super) enum Fault {
    None,
    WorkerPanic,
    AcceptPanic,
}
#[derive(Clone, Debug)]
pub(super) struct Report {
    pub(super) success: bool,
    pub(super) workers: usize,
}
pub(super) struct Fixture {
    port: u16,
    name: String,
    cancel: Arc<AtomicBool>,
    workers: Arc<Mutex<Vec<(usize, Worker)>>>,
    accept: Option<JoinHandle<Result<(), String>>>,
    journal: Journal,
    report: Option<Report>,
}
impl Fixture {
    pub(super) fn start(
        name: &str,
        routes: HashMap<String, String>,
        fault: Fault,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let workers = Arc::new(Mutex::new(Vec::new()));
        let journal = Journal::default();
        let stop = Arc::clone(&cancel);
        let registry = Arc::clone(&workers);
        let events = journal.clone();
        let label = name.to_owned();
        let responses = Arc::new(routes.into_iter().map(|(path,body)| (path,format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).into_bytes())).collect::<HashMap<_,_>>());
        let accept=std::thread::Builder::new().name(format!("275-{name}-accept")).spawn(move || {
            let mut id=0;
            while !stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream,_))=>{
                        id+=1;
                        if id>128 { return Err("fixture worker limit128".into()); }
                        // No active worker I/O can block: both directions are
                        // nonblocking, cancellation checked at every iteration.
                        stream.set_nonblocking(true).map_err(|e|e.to_string())?;
                        let worker_stop=Arc::clone(&stop);
                        let worker_events=events.clone();
                        let worker_responses=Arc::clone(&responses);
                        let worker_label=label.clone();
                        let worker=std::thread::Builder::new().name(format!("275-{label}-{id}")).spawn(move || {
                            worker_events.record(&json!({"fixture":worker_label,"stage":"worker-start","id":id}));
                            let result=serve(stream,&worker_responses,&worker_stop,&worker_events,&worker_label,id,fault);
                            worker_events.record(&json!({"fixture":worker_label,"stage":"worker-return","id":id,"result":format!("{result:?}")}));
                            result
                        }).map_err(|e|e.to_string())?;
                        // Registry is owned outside the accept thread. Even an
                        // accept panic cannot discard outstanding join handles.
                        registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((id,worker));
                        if matches!(fault,Fault::AcceptPanic) { panic!("injected275 accept panic after registration"); }
                    }
                    Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(2)),
                    Err(e)=>return Err(e.to_string()),
                }
            }
            events.record(&json!({"fixture":label,"stage":"accept-return","result":"Ok"}));
            Ok(())
        })?;
        Ok(Self {
            port,
            name: name.to_owned(),
            cancel,
            workers,
            accept: Some(accept),
            journal,
            report: None,
        })
    }
    pub(super) fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    pub(super) fn journal(&self) -> Journal {
        self.journal.clone()
    }
    pub(super) fn finish(&mut self) -> Report {
        if let Some(report) = &self.report {
            return report.clone();
        }
        self.cancel.store(true, Ordering::Release);
        let accept_result = self.accept.take().map(std::thread::JoinHandle::join);
        let mut success = matches!(&accept_result, Some(Ok(Ok(()))));
        self.journal.record(&json!({"fixture":self.name,"stage":"accept-join","success":success,"result":format!("{accept_result:?}")}));
        let workers = std::mem::take(
            &mut *self
                .workers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        let count = workers.len();
        // Every join is attempted, including after another join panics/fails.
        for (id, handle) in workers {
            let result = handle.join();
            let ok = matches!(&result, Ok(Ok(_)));
            success &= ok;
            self.journal.record(&json!({"fixture":self.name,"stage":"worker-join","id":id,"success":ok,"result":format!("{result:?}")}));
        }
        success &= !self.journal.write_failed.load(Ordering::Acquire);
        self.journal.record(&json!({"fixture":self.name,"stage":"cleanup-complete","success":success,"workers":count,"duringUnwind":std::thread::panicking()}));
        success &= !self.journal.write_failed.load(Ordering::Acquire);
        let report = Report {
            success,
            workers: count,
        };
        self.report = Some(report.clone());
        report
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Never panic from Drop, especially when a test assertion is unwinding.
        // Normal callers must assert finish().success before declaring success.
        let _ = self.finish();
    }
}
#[allow(clippy::too_many_arguments)]
fn serve(
    mut stream: TcpStream,
    responses: &HashMap<String, Vec<u8>>,
    cancel: &AtomicBool,
    journal: &Journal,
    name: &str,
    id: usize,
    fault: Fault,
) -> Result<&'static str, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut request = Vec::new();
    loop {
        if cancel.load(Ordering::Acquire) {
            return Ok("cancelled-read");
        }
        if Instant::now() >= deadline {
            return Err("request-read deadline5s".into());
        }
        let mut bytes = [0u8; 1024];
        match stream.read(&mut bytes) {
            Ok(0) => return Ok("peer-closed-read"),
            Ok(n) => {
                request.extend_from_slice(&bytes[..n]);
                if request.windows(4).any(|s| s == b"\r\n\r\n") {
                    break;
                }
                if request.len() > 8192 {
                    return Err("header limit8192".into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    if matches!(fault, Fault::WorkerPanic) {
        panic!("injected275 worker panic");
    }
    let request = String::from_utf8_lossy(&request);
    let path = request
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default();
    let bytes = responses.get(path).map_or(
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".as_slice(),
        Vec::as_slice,
    );
    journal.record(&json!({"fixture":name,"stage":"write-start","id":id,"bytes":bytes.len()}));
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut written = 0;
    while written < bytes.len() {
        if cancel.load(Ordering::Acquire) {
            return Ok("cancelled-write");
        }
        if Instant::now() >= deadline {
            return Err("response-write deadline5s".into());
        }
        let end = (written + 8192).min(bytes.len());
        match stream.write(&bytes[written..end]) {
            Ok(0) => return Err("zero-byte write".into()),
            Ok(n) => written += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok("response-complete")
}
