//! Explicit initial-tab prerequisite for 275, not product/document readiness.
//! One connection; scheduled snapshots after successful empty replies only.
use ff_rdp_core::TabInfo;
use ff_rdp_core::transport::{encode_frame, recv_from};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{self, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const STARTUP_BOUND: Duration = Duration::from_secs(10);
const MAX_LIST_REQUESTS: usize = 9;

#[derive(Debug)]
pub struct Report {
    // Exact monotonic schedule origin for independent socket-arrival controls.
    pub start: Instant,
    pub pid: u32,
    pub port: u16,
    pub requests: usize,
    pub notifications: usize,
    pub tabs: Vec<TabInfo>,
    pub observations: Vec<Value>,
    pub bytes_read: usize,
    pub bytes_written: usize,
}

#[derive(Debug)]
pub struct Failure {
    pub reason: String,
    pub report: Box<Report>,
}

impl Report {
    fn record(&mut self, start: Instant, event: &str, data: &Value) {
        let row = json!({"pid":self.pid,"port":self.port,"elapsed_ms":start.elapsed().as_millis(),
            "event":event,"data":data});
        eprintln!("275 initial-tab {row}");
        self.observations.push(row);
    }
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "initial-tab absolute deadline"))
}

// Recompute the SAME deadline before every underlying syscall, including
// partial length prefixes/bodies and short writes. No idle-timeout renewal.
struct DeadlineIo {
    stream: TcpStream,
    deadline: Instant,
    transferred: Arc<AtomicUsize>,
}

impl Read for DeadlineIo {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.stream
            .set_read_timeout(Some(remaining(self.deadline)?))?;
        let read = self.stream.read(bytes)?;
        self.transferred.fetch_add(read, Ordering::Relaxed);
        Ok(read)
    }
}

impl Write for DeadlineIo {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream
            .set_write_timeout(Some(remaining(self.deadline)?))?;
        let written = self.stream.write(bytes)?;
        self.transferred.fetch_add(written, Ordering::Relaxed);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream
            .set_write_timeout(Some(remaining(self.deadline)?))?;
        self.stream.flush()
    }
}

fn receive(reader: &mut BufReader<DeadlineIo>, deadline: Instant) -> Result<Value, String> {
    remaining(deadline).map_err(|e| e.to_string())?;
    let value = recv_from(reader).map_err(|e| format!("initial-tab receive: {e}"))?;
    // A buffered complete frame is also ineligible after the absolute bound.
    remaining(deadline).map_err(|e| e.to_string())?;
    Ok(value)
}

fn list_tabs(
    writer: &mut DeadlineIo,
    report: &mut Report,
    start: Instant,
    slot: u32,
) -> Result<(), String> {
    if report.requests >= MAX_LIST_REQUESTS {
        return Err("initial-tab list request cap reached without a tab".to_owned());
    }
    let packet = json!({"to":"root","type":"listTabs"});
    writer
        .write_all(encode_frame(&packet.to_string()).as_bytes())
        .map_err(|e| format!("initial-tab list write: {e}"))?;
    remaining(writer.deadline).map_err(|e| e.to_string())?;
    report.requests += 1;
    report.record(
        start,
        "list-request",
        &json!({"ordinal":report.requests,"slot":slot,"packet":packet}),
    );
    Ok(())
}

pub fn observe(pid: u32, port: u16) -> Result<Report, Failure> {
    observe_with_timeout(pid, port, STARTUP_BOUND)
}

/// Injection only for finite browser-free controls; slots scale with the budget.
/// Native uses10s, with fixed start+0..8s opportunities and no catch-up bursts.
pub fn observe_with_timeout(pid: u32, port: u16, budget: Duration) -> Result<Report, Failure> {
    let start = Instant::now();
    let reads = Arc::new(AtomicUsize::new(0));
    let written_bytes = Arc::new(AtomicUsize::new(0));
    let mut report = Report {
        start,
        pid,
        port,
        requests: 0,
        notifications: 0,
        tabs: Vec::new(),
        observations: Vec::new(),
        bytes_read: 0,
        bytes_written: 0,
    };
    report.record(
        start,
        "begin",
        &json!({"budget_ms":budget.as_millis(),"slot_interval_ms":(budget / 10).as_millis(),
            "max_list_requests":MAX_LIST_REQUESTS,"policy":"scheduled-only"}),
    );
    let result = (|| -> Result<(), String> {
        if pid == 0 || port == 0 {
            return Err("initial-tab requires the owned positive pid/port".to_owned());
        }
        let deadline = report
            .start
            .checked_add(budget)
            .ok_or("initial-tab deadline overflow")?;
        let interval = budget / 10;
        if interval.is_zero() {
            return Err("initial-tab budget too small for scheduled observations".to_owned());
        }
        let endpoint = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
        let stream =
            TcpStream::connect_timeout(&endpoint, remaining(deadline).map_err(|e| e.to_string())?)
                .map_err(|e| format!("initial-tab connect: {e}"))?;
        report.record(
            start,
            "connected",
            &json!({"endpoint":endpoint.to_string()}),
        );
        let mut reader = BufReader::new(DeadlineIo {
            stream: stream.try_clone().map_err(|e| e.to_string())?,
            deadline,
            transferred: Arc::clone(&reads),
        });
        let mut writer = DeadlineIo {
            stream,
            deadline,
            transferred: Arc::clone(&written_bytes),
        };
        let greeting = receive(&mut reader, deadline)?;
        report.record(start, "greeting", &greeting);
        if greeting["from"] != "root" || greeting["applicationType"] != "browser" {
            return Err("initial-tab invalid browser greeting".to_owned());
        }
        // Slot0 is the first possible observation after the greeting. Connect
        // and greeting time count against the same absolute budget.
        list_tabs(&mut writer, &mut report, start, 0)?;
        let mut next_slot = 1;
        loop {
            let packet = receive(&mut reader, deadline)?;
            report.record(start, "packet", &packet);
            let from = packet
                .get("from")
                .and_then(Value::as_str)
                .filter(|from| !from.is_empty())
                .ok_or("initial-tab malformed source actor")?;
            if packet.get("error").is_some() {
                return Err(format!("initial-tab protocol error: {packet}"));
            }
            if from != "root" {
                if packet.get("type").and_then(Value::as_str).is_none() {
                    return Err("initial-tab unexpected non-root reply".to_owned());
                }
                continue;
            }
            if let Some(kind) = packet.get("type") {
                let kind = kind.as_str().ok_or("initial-tab malformed push type")?;
                if kind == "tabListChanged" {
                    report.notifications += 1;
                }
                continue;
            }
            let value = packet.get("tabs").ok_or("initial-tab reply missing tabs")?;
            let tabs: Vec<TabInfo> = serde_json::from_value(value.clone())
                .map_err(|e| format!("initial-tab malformed list: {e}"))?;
            let mut actors = HashSet::new();
            if tabs.iter().any(|tab| {
                tab.actor.as_ref().trim().is_empty()
                    || !actors.insert(tab.actor.as_ref().to_owned())
            }) {
                return Err("initial-tab invalid/duplicate descriptor actor".to_owned());
            }
            report.record(
                start,
                "list-reply",
                &json!({"ordinal":report.requests,"tabs":tabs}),
            );
            if !tabs.is_empty() {
                remaining(deadline).map_err(|e| e.to_string())?;
                report.tabs = tabs;
                return Ok(());
            }
            if report.requests == MAX_LIST_REQUESTS {
                return Err("initial-tab list request cap reached without a tab".to_owned());
            }
            // Only a valid empty reply completes the outstanding request and
            // permits a further observation. Events never accelerate slots.
            // Skip all slots elapsed during connect, greeting, or a slow reply.
            let now = Instant::now();
            while next_slot < 9 && start + interval * next_slot <= now {
                next_slot += 1;
            }
            if next_slot == 9 {
                return Err("initial-tab observation slots exhausted without a tab".to_owned());
            }
            let slot = next_slot;
            let due = start + interval * slot;
            report.record(
                start,
                "scheduled",
                &json!({"slot":slot,
                "due_ms":due.duration_since(start).as_millis()}),
            );
            // No timed receive here: restarting recv_from after a short read
            // timeout would discard its decoder's partial frame. Queued pushes
            // are decoded with the next reply under the original deadline.
            std::thread::sleep(due.saturating_duration_since(Instant::now()));
            remaining(deadline).map_err(|e| e.to_string())?;
            list_tabs(&mut writer, &mut report, start, slot)?;
            next_slot += 1;
        }
    })();
    report.bytes_read = reads.load(Ordering::Relaxed);
    report.bytes_written = written_bytes.load(Ordering::Relaxed);
    report.record(start, if result.is_ok() { "ready" } else { "failed" },
        &json!({"requests":report.requests,"notifications":report.notifications,"tab_count":report.tabs.len(),
            "bytes_read":report.bytes_read,"bytes_written":report.bytes_written,"error":result.as_ref().err(),
            "limit":"Root descriptor availability only; not actual-document or persistent target readiness."}));
    match result {
        Ok(()) => Ok(report),
        Err(reason) => Err(Failure {
            reason,
            report: Box::new(report),
        }),
    }
}
