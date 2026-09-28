//! Finite, browser-free iteration259 controls through actual daemon workers.
//! Inline packets model an ordinary root reply, not recorded Firefox fixtures.

use std::io::Write as _;
use std::net::{Shutdown, TcpListener};
use std::panic::{AssertUnwindSafe, catch_unwind};

use anyhow::{Context, Result, ensure};
use ff_rdp_core::RootActor;

use super::super::lifecycle::{JoinRecord, WorkerOutcome};
use super::*;

const ACTIVE_BUDGET: Duration = Duration::from_secs(20);
const CASE_BUDGET: Duration = Duration::from_secs(30);
const IO_BUDGET: Duration = Duration::from_secs(3);
const A_MARKER: &str = "ITER259-ANSWER-OWNED-BY-A";
const B_MARKER: &str = "ITER259-ANSWER-OWNED-BY-B";

struct Worker {
    name: &'static str,
    result: mpsc::Receiver<Result<()>>,
    joined: bool,
}

struct Harness {
    case: &'static str,
    started: Instant,
    deadline: Instant,
    state: Arc<SharedState>,
    sockets: Vec<TcpStream>,
    registrations: Vec<Registration>,
    supervisor: WorkerOwner,
    joins: mpsc::Receiver<JoinRecord>,
    workers: Vec<Worker>,
    failures: Vec<String>,
    cleaned: bool,
}

impl Harness {
    fn new(case: &'static str, state: SharedState) -> Self {
        let started = Instant::now();
        let mut supervisor = WorkerOwner::new(Arc::clone(&state.cancellation));
        let (join_tx, joins) = mpsc::channel();
        supervisor.observe_joins(join_tx);
        Self {
            case,
            started,
            deadline: started + ACTIVE_BUDGET,
            state: Arc::new(state),
            sockets: Vec::new(),
            registrations: Vec::new(),
            supervisor,
            joins,
            workers: Vec::new(),
            failures: Vec::new(),
            cleaned: false,
        }
    }

    fn phase(&self, name: &str, detail: &Value) {
        let _ = writeln!(
            std::io::stderr().lock(),
            "ITER259_PHASE {}",
            json!({"case":self.case,"phase":name,"elapsed_ms":self.started.elapsed().as_millis(),"detail":detail})
        );
    }

    fn own_socket(&mut self, socket: &TcpStream) -> Result<()> {
        self.sockets.push(
            socket
                .try_clone()
                .context("retain owned socket interrupt")?,
        );
        Ok(())
    }

    fn spawn(
        &mut self,
        name: &'static str,
        policy: WorkerPolicy,
        work: impl FnOnce() -> Result<()> + Send + 'static,
    ) -> Result<()> {
        let (result_tx, result) = mpsc::channel();
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        self.supervisor.spawn(name, policy, move || {
            let _ = entered_tx.send(());
            let _ = result_tx.send(work());
        })?;
        self.workers.push(Worker {
            name,
            result,
            joined: false,
        });
        self.phase("worker_acquired", &json!({"worker":name}));
        entered_rx
            .recv_timeout(self.deadline.saturating_duration_since(Instant::now()))
            .context("worker body did not enter before deadline")?;
        self.phase("worker_body_entered", &json!({"worker":name}));
        Ok(())
    }

    fn join(&mut self, name: &'static str) -> Result<()> {
        let index = self
            .workers
            .iter()
            .position(|worker| worker.name == name)
            .context("named worker was not acquired")?;
        while !self.workers[index].joined {
            self.supervisor.reap_finished();
            self.observe_joins("worker_joined");
            if self.workers[index].joined {
                break;
            }
            ensure!(
                Instant::now() < self.deadline,
                "active deadline waiting for {name}"
            );
            thread::sleep(Duration::from_millis(1));
        }
        ensure!(
            self.failures.is_empty(),
            "worker failures: {:?}",
            self.failures
        );
        Ok(())
    }

    fn observe_joins(&mut self, phase: &str) {
        while let Ok(record) = self.joins.try_recv() {
            self.phase(
                phase,
                &json!({"worker":record.name,
                "outcome":format!("{:?}", record.outcome),
                "supervision_returned":record.supervision_returned}),
            );
            let Some(worker) = self
                .workers
                .iter_mut()
                .find(|worker| worker.name == record.name)
            else {
                self.failures
                    .push(format!("unrecognized join: {}", record.name));
                continue;
            };
            if worker.joined {
                self.failures
                    .push(format!("duplicate join: {}", record.name));
            }
            worker.joined = true;
            if !record.supervision_returned
                || matches!(
                    record.outcome,
                    WorkerOutcome::Panicked | WorkerOutcome::UnexpectedReturn
                )
            {
                self.failures
                    .push(format!("{}: {:?}", record.name, record.outcome));
            }
            match worker.result.try_recv() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => self.failures.push(format!("{}: {error:#}", record.name)),
                Err(error) => self
                    .failures
                    .push(format!("{} body completion missing: {error}", record.name)),
            }
        }
    }

    /// Interrupt all owned I/O before any join, then collect every real handle.
    /// The schedule contains at most two Firefox responses and an eight-slot
    /// cancellable queue. Production stop closes the queue and wakes its reader
    /// and dispatcher; B starts only after A's actual supervised handler join.
    fn cleanup(&mut self) -> Vec<String> {
        if self.cleaned {
            return self.failures.clone();
        }
        self.state.stop(StopReason::AuthenticatedShutdown);
        for socket in &self.sockets {
            let _ = socket.shutdown(Shutdown::Both);
        }
        // Never discard/detach a handle on timeout or assertion failure.
        // Root's separate watchdog rejects a hung join as unknown completion.
        self.supervisor
            .shutdown_and_join(StopReason::AuthenticatedShutdown);
        self.observe_joins("cleanup_joined");
        for worker in &self.workers {
            if !worker.joined {
                self.failures
                    .push(format!("{} actual join missing", worker.name));
            }
        }
        self.cleaned = true;
        self.phase(
            "cleanup_complete",
            &json!({"workers":self.workers.len(),"failures":self.failures}),
        );
        self.failures.clone()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        // Errors are recorded by cleanup; Drop must not panic while unwinding.
        let _ = self.cleanup();
    }
}

// Same portable readiness mechanism as lifecycle_controls::accept_controlled,
// with the caller's existing absolute I/O/active deadline instead of a new wait.
fn accept_ready(listener: TcpListener, deadline: Instant) -> Result<TcpStream> {
    let mut listener = mio::net::TcpListener::from_std(listener);
    let mut poll = mio::Poll::new().context("owned listener poll")?;
    poll.registry()
        .register(&mut listener, mio::Token(0), mio::Interest::READABLE)
        .context("owned listener readiness registration")?;
    let mut events = mio::Events::with_capacity(1);
    loop {
        ensure!(
            Instant::now() < deadline,
            "owned listener accept deadline elapsed"
        );
        match listener.accept() {
            Ok((socket, _)) => {
                let socket = TcpStream::from(socket);
                socket.set_nonblocking(false)?;
                return Ok(socket);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error).context("owned listener accept"),
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(
            !remaining.is_zero(),
            "owned listener readiness deadline elapsed"
        );
        match poll.poll(&mut events, Some(remaining)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error).context("owned listener readiness wait"),
        }
    }
}

fn socket_pair(active_deadline: Instant) -> Result<(TcpStream, TcpStream)> {
    let deadline = active_deadline.min(Instant::now() + IO_BUDGET);
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let client = TcpStream::connect_timeout(
        &listener.local_addr()?,
        deadline.saturating_duration_since(Instant::now()),
    )?;
    let server =
        accept_ready(listener, deadline).context("accept established owned loopback connection")?;
    for socket in [&client, &server] {
        socket.set_nonblocking(false)?;
        socket.set_nodelay(true)?;
        socket.set_read_timeout(Some(IO_BUDGET))?;
        socket.set_write_timeout(Some(IO_BUDGET))?;
    }
    Ok((client, server))
}

fn authenticated_client(
    harness: &mut Harness,
    name: &'static str,
    firefox_writer: &FirefoxWriter,
) -> Result<RdpTransport> {
    let deadline = harness.deadline.min(Instant::now() + IO_BUDGET);
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let mut client = RdpTransport::connect_raw(
        "127.0.0.1",
        address.port(),
        deadline.saturating_duration_since(Instant::now()),
    )?;
    let server =
        accept_ready(listener, deadline).context("accept established client connection")?;
    harness.own_socket(&server)?;
    let state = Arc::clone(&harness.state);
    let writer = Arc::clone(firefox_writer);
    harness.spawn(name, WorkerPolicy::Client, move || {
        handle_client(&state, server, &writer)
    })?;
    client.send(&json!({"auth":"test-token"}))?;
    let greeting = client.recv()?;
    ensure!(
        greeting["applicationType"] == "browser",
        "actual handler greeting missing"
    );
    ensure!(
        greeting["protocol_version"] == DAEMON_PROTOCOL_VERSION,
        "wrong daemon version"
    );
    client.enable_proxy_v3();
    harness.phase("authenticated", &json!({"handler":name}));
    Ok(client)
}

fn read_request(harness: &Harness, peer: &mut FramedReader, owner: &str) -> Result<()> {
    ensure!(
        Instant::now() < harness.deadline,
        "active deadline before {owner} request"
    );
    let request = peer
        .recv()
        .with_context(|| format!("actual {owner} Firefox request"))?;
    ensure!(
        request == json!({"to":"root","type":"listTabs"}),
        "unexpected {owner} request: {request}"
    );
    harness.phase(
        "firefox_request_observed",
        &json!({"owner":owner,"request":request}),
    );
    Ok(())
}

fn send_reply(harness: &Harness, peer: &mut FramedWriter, marker: &str) -> Result<()> {
    let reply = json!({"from":"root","tabs":[{"actor":"test-tab","title":marker,"url":"about:blank"}],"selected":0});
    peer.send(&reply)?;
    harness.phase("peer_reply_written", &json!({"marker":marker}));
    Ok(())
}

fn run_schedule(harness: &mut Harness, abandoned: bool) -> Result<String> {
    let (daemon, peer) = socket_pair(harness.deadline)?;
    harness.own_socket(&daemon)?;
    harness.own_socket(&peer)?;
    let daemon_reader = FramedReader::from_stream(daemon.try_clone()?);
    harness
        .registrations
        .push(harness.state.cancellation.register_socket(&daemon)?);
    let origin = harness
        .state
        .origin(EventSource::Primary)
        .context("primary origin")?;
    *lock_or_recover!(origin.socket) = Some(daemon.try_clone()?);
    let firefox_writer = Arc::new(WriterSlot::new(FramedWriter::from_stream(daemon)));
    *lock_or_recover!(origin.writer) = Some(Arc::clone(&firefox_writer));
    harness.registrations.push(register_firefox_writer(
        &harness.state.cancellation,
        &firefox_writer,
    ));
    let mut peer_reader = FramedReader::from_stream(peer.try_clone()?);
    let mut peer_writer = FramedWriter::from_stream(peer);
    let state = Arc::clone(&harness.state);
    harness.spawn("firefox-reader", WorkerPolicy::Required, move || {
        firefox_reader_loop(&state, daemon_reader);
        Ok(())
    })?;
    let state = Arc::clone(&harness.state);
    let writer = Arc::clone(&firefox_writer);
    let event_rx = Arc::clone(&harness.state.event_tx);
    let setup_rx = Arc::new(BoundedQueue::new(1));
    harness.spawn("dispatcher", WorkerPolicy::Required, move || {
        event_dispatcher_loop(
            &state,
            event_rx,
            VecDeque::new(),
            None,
            None,
            setup_rx,
            writer,
        );
        Ok(())
    })?;

    let mut a = authenticated_client(harness, "handler-a", &firefox_writer)?;
    a.send_ordinary(&json!({"to":"root","type":"listTabs"}))?;
    read_request(harness, &mut peer_reader, "A")?;
    let a_owner = lock_or_recover!(harness.state.rpc_writer)
        .as_ref()
        .map(|(id, ..)| *id)
        .context("A has no actual RPC slot")?;
    harness.phase("owner_observed", &json!({"owner":"A","client_id":a_owner}));
    if !abandoned {
        send_reply(harness, &mut peer_writer, A_MARKER)?;
        let reply = ff_rdp_core::transport::recv_reply_from(&mut a, "root")?;
        ensure!(
            reply["tabs"][0]["title"] == A_MARKER,
            "A did not consume its own response"
        );
        harness.phase("a_reply_consumed", &json!({"marker":A_MARKER}));
    }
    // A successor is already authenticated, but its request starts only after
    // A's actual departure/join. Thus cancellation cannot masquerade as auth failure.
    let prepared_b = if abandoned {
        let mut b = authenticated_client(harness, "handler-b", &firefox_writer)?;
        let (proceed_tx, proceed_rx) = mpsc::sync_channel(1);
        let (returned_tx, returned_rx) = mpsc::sync_channel(1);
        let deadline = harness.deadline;
        harness.spawn("caller-b", WorkerPolicy::Client, move || {
            proceed_rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .context("B request gate not released")?;
            let result = b.with_read_deadline(deadline, RootActor::list_tabs);
            returned_tx
                .send(result.is_err())
                .context("B result observer missing")?;
            Ok(())
        })?;
        Some((proceed_tx, returned_rx))
    } else {
        None
    };
    // No client-side clones exist: dropping A is its voluntary departure.
    drop(a);
    harness.phase(
        "a_client_dropped",
        &json!({"ordinary_reply_sent":!abandoned}),
    );
    harness.join("handler-a")?;
    ensure!(
        lock_or_recover!(harness.state.rpc_writer).is_none(),
        "A slot remains after actual handler join"
    );
    harness.phase("a_return_and_slot_release", &json!({"client_id":a_owner}));

    if abandoned {
        let snapshot = harness.state.ownership_health();
        ensure!(
            harness.state.cancellation.reason().is_none()
                && snapshot["retiring"] == false
                && snapshot["terminal_reason"].is_null()
                && snapshot["departures_with_outstanding"] == 1
                && snapshot["pending_ordinary"] == 1
                && snapshot["orphan_pending"] == 1
                && snapshot["active_pending"] == 0
                && snapshot["quarantined_actor_count"] == 1,
            "departure must quarantine root without forgiving debt: {snapshot}"
        );
        harness.phase("root_quarantined_before_b_send", &snapshot);
        let (proceed_tx, returned_rx) = prepared_b.context("authenticated B worker missing")?;
        proceed_tx
            .send(())
            .context("B request gate receiver missing")?;
        ensure!(
            returned_rx.recv_timeout(harness.deadline.saturating_duration_since(Instant::now()))?,
            "B unexpectedly received an answer"
        );
        let snapshot = harness.state.ownership_health();
        ensure!(
            harness.state.cancellation.reason() == Some(StopReason::ReplyOwnership)
                && snapshot["terminal_reason"] == "orphan_actor_reused"
                && snapshot["pending_ordinary"] == 1
                && snapshot["orphan_pending"] == 1,
            "same-actor B was not refused before reserving/sending: {snapshot}"
        );
        // The actual Firefox socket was interrupted. A timeout is insufficient:
        // it would not establish completion of the no-successor-send observation.
        match peer_reader.recv() {
            Err(error) if is_client_hangup(&error) => {}
            other => anyhow::bail!("expected closed Firefox stream with no B frame, got {other:?}"),
        }
        harness.phase(
            "b_request_not_forwarded",
            &json!({"closed_origin":true,"b_returned_error":true}),
        );
        harness.join("caller-b")?;
        harness.join("handler-b")?;
        let late = json!({"from":"root","tabs":[{"title":A_MARKER}]});
        ensure!(
            reply_ownership::route(&harness.state, EventSource::Primary, &late),
            "retired ledger did not consume late A reply"
        );
        ensure!(
            harness.state.ownership_health()["discarded_replies"] == 1,
            "late packet not accounted"
        );
        harness.phase(
            "late_a_ledger_disposition",
            &json!({"injected_after_wire_closed":true,"discarded":true}),
        );
        return Ok("retired-without-successor-send".into());
    }
    let mut b = authenticated_client(harness, "handler-b", &firefox_writer)?;
    let deadline = harness.deadline;
    let (accepted_tx, accepted_rx) = mpsc::sync_channel(1);
    harness.spawn("caller-b", WorkerPolicy::Client, move || {
        let result = b
            .with_read_deadline(deadline, RootActor::list_tabs)
            .map_err(|error| error.to_string())
            .and_then(|tabs| {
                if tabs.len() == 1 {
                    Ok(tabs[0].title.clone())
                } else {
                    Err(format!(
                        "B parsed {} tabs, expected exactly one",
                        tabs.len()
                    ))
                }
            });
        accepted_tx
            .send(result)
            .map_err(|_| anyhow::anyhow!("caller-B result observer gone"))?;
        Ok(())
    })?;
    read_request(harness, &mut peer_reader, "B")?;
    let b_owner = lock_or_recover!(harness.state.rpc_writer)
        .as_ref()
        .map(|(id, ..)| *id)
        .context("B has no actual RPC slot")?;
    ensure!(b_owner != a_owner, "B reused A client identity");
    harness.phase("owner_observed", &json!({"owner":"B","client_id":b_owner}));
    if abandoned {
        send_reply(harness, &mut peer_writer, A_MARKER)?;
    }
    send_reply(harness, &mut peer_writer, B_MARKER)?;
    let returned = accepted_rx
        .recv_timeout(harness.deadline.saturating_duration_since(Instant::now()))
        .context("B caller did not return before active deadline")?;
    harness.phase("b_caller_returned", &json!({"result":returned}));
    harness.join("caller-b")?;
    harness.join("handler-b")?;
    returned.map_err(anyhow::Error::msg)
}

fn exercise(case: &'static str, abandoned: bool) -> (Result<String>, Vec<String>, Duration) {
    let state = super::tests::test_state_with_queues(8, ff_rdp_core::release_queue(1).0);
    let mut harness = Harness::new(case, state);
    let outcome = catch_unwind(AssertUnwindSafe(|| run_schedule(&mut harness, abandoned)));
    let mut cleanup_failures = harness.cleanup();
    if abandoned && cleanup_failures.is_empty() {
        let persist = (|| -> Result<()> {
            let directory = tempfile::tempdir()?;
            let mut info = DaemonInfo {
                session_sequence: None,
                session_id: None,
                pid: std::process::id(),
                proxy_port: 1,
                firefox_host: "127.0.0.1".into(),
                firefox_port: 6000,
                started_at: "controlled".into(),
                auth_token: "controlled".into(),
                start_token: None,
            };
            registry::reply_ownership::publish_in(directory.path(), &mut info)?;
            let mut finalizer = reply_ownership::Finalizer::in_directory(
                Arc::clone(&harness.state),
                info,
                directory.path().to_owned(),
            );
            finalizer.finish()?;
            let receipt = registry::reply_ownership::read_receipt_in(directory.path(), 6000)?
                .context("missing persisted receipt")?;
            ensure!(
                receipt["reply_ownership"]["terminal_reason"] == "orphan_actor_reused",
                "wrong persisted terminal reason"
            );
            harness.phase("terminal_reason_persisted_after_joins", &receipt);
            Ok(())
        })();
        if let Err(error) = persist {
            cleanup_failures.push(format!("terminal receipt: {error:#}"));
        }
    }
    let elapsed = harness.started.elapsed();
    let result = match outcome {
        Ok(result) => result,
        Err(_) => Err(anyhow::anyhow!(
            "schedule panicked; all acquired workers joined before assertion"
        )),
    };
    (result, cleanup_failures, elapsed)
}

#[test]
fn primary_retirement_publishes_reason_before_controlled_reader_wake() -> Result<()> {
    let mut harness = Harness::new("retirement-reader-wake", super::tests::test_state());
    let (daemon, peer) = socket_pair(harness.deadline)?;
    harness.own_socket(&daemon)?;
    harness.own_socket(&peer)?;
    let origin = harness
        .state
        .origin(EventSource::Primary)
        .context("primary")?;
    *lock_or_recover!(origin.socket) = Some(daemon.try_clone()?);
    let reader = FramedReader::from_stream(daemon.try_clone()?);
    let writer = Arc::new(WriterSlot::new(FramedWriter::from_stream(daemon)));
    *lock_or_recover!(origin.writer) = Some(Arc::clone(&writer));
    let (wake_tx, wake_rx) = mpsc::channel();
    let cancellation = Arc::clone(&harness.state.cancellation);
    let wake_origin = Arc::clone(&origin);
    harness
        .registrations
        .push(
            harness
                .state
                .cancellation
                .register(WakePhase::SocketAndWriter, move || {
                    let reason = cancellation.reason();
                    let ledger = lock_or_recover!(wake_origin.ledger).snapshot();
                    let _ = wake_tx.send((reason, ledger));
                }),
        );
    let socket_registration = harness.state.cancellation.register_socket(
        lock_or_recover!(origin.socket)
            .as_ref()
            .context("interrupt socket")?,
    )?;
    harness.registrations.push(socket_registration);
    harness.registrations.push(register_firefox_writer(
        &harness.state.cancellation,
        &writer,
    ));
    let (returned_tx, returned_rx) = mpsc::channel();
    let state = Arc::clone(&harness.state);
    harness.spawn("firefox-reader", WorkerPolicy::Required, move || {
        firefox_reader_loop(&state, reader);
        let _ = returned_tx.send(());
        Ok(())
    })?;
    let mut peer = FramedWriter::from_stream(peer);
    peer.send(&json!({"from":"root","type":"test-reader-ready"}))?;
    let frame = harness.state.event_tx.recv_until(harness.deadline);
    ensure!(
        matches!(frame,
        Some(DaemonEvent::Packet(EventSource::Primary, ref message))
        if *message == json!({"from":"root","type":"test-reader-ready"})),
        "actual production reader did not consume the readiness frame"
    );

    // The old implementation interrupted the socket, then acquired this
    // mutex, then published ReplyOwnership. Holding it forces the actual
    // reader to finish its EOF path before that old publication can occur.
    // The corrected primary path uses registered wakes and never needs this
    // mutex. No EOF allowance, timer retry, or synthetic reader substitutes.
    let held_writer = lock_or_recover!(origin.writer);
    let state = Arc::clone(&harness.state);
    harness.spawn("origin-retirement", WorkerPolicy::Client, move || {
        state.retire_origin(EventSource::Primary, "controlled_owner_departure");
        Ok(())
    })?;
    let returned =
        returned_rx.recv_timeout(harness.deadline.saturating_duration_since(Instant::now()));
    let wake = wake_rx.try_recv();
    let reason = harness.state.cancellation.reason();
    drop(held_writer); // Release the deliberate blocker before any join/assertion.
    harness.join("firefox-reader")?;
    harness.join("origin-retirement")?;
    let active_elapsed = harness.started.elapsed();
    let cleanup = harness.cleanup();
    returned.context("actual reader did not return after retirement wake")?;
    let (wake_reason, ledger) = wake.context("registered interrupt phase did not run")?;
    harness.phase(
        "controlled_reader_returned",
        &json!({"reason":format!("{reason:?}"),
            "wake_reason":format!("{wake_reason:?}"),"ledger":ledger,
            "active_elapsed_ms":active_elapsed.as_millis()}),
    );
    ensure!(cleanup.is_empty(), "cleanup failures: {cleanup:?}");
    ensure!(active_elapsed < ACTIVE_BUDGET, "active deadline exceeded");
    ensure!(
        harness.started.elapsed() < CASE_BUDGET,
        "case exceeded 30 seconds"
    );
    ensure!(
        reason == Some(StopReason::ReplyOwnership),
        "reader won reason: {reason:?}"
    );
    ensure!(
        wake_reason == reason,
        "reason was not published before interrupt phase"
    );
    ensure!(
        ledger["terminal_reason"] == "controlled_owner_departure",
        "ledger not latched before wake"
    );
    Ok(())
}

#[test]
fn missing_primary_origin_refuses_recovery_without_writing_or_stopping_under_rpc() -> Result<()> {
    use std::io::Read as _;
    let started = Instant::now();
    let deadline = started + ACTIVE_BUDGET;
    let state = super::tests::test_state();
    let (daemon, mut peer) = socket_pair(deadline)?;
    let writer = WriterSlot::new(FramedWriter::from_stream(daemon));
    lock_or_recover!(state.reply_origins).remove(&EventSource::Primary);
    let owner = lock_or_recover!(state.rpc_writer);
    let result = send_startup_recovery_owned(&state, &writer, "watcher", deadline);
    ensure!(
        matches!(result, Err(ProtocolError::InvalidPacket(ref reason)) if reason == "primary_reply_origin_missing"),
        "missing origin did not return the named refusal: {result:?}"
    );
    ensure!(
        state.cancellation.reason().is_none(),
        "helper stopped under RPC lock"
    );
    let released = writer.acquire(deadline);
    ensure!(
        released.is_ok(),
        "recovery refusal retained its writer lease"
    );
    drop(released);
    drop(owner);
    peer.set_nonblocking(true)?;
    let mut byte = [0];
    ensure!(
        matches!(peer.read(&mut byte), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "refused recovery wrote to Firefox"
    );
    ensure!(
        started.elapsed() < ACTIVE_BUDGET,
        "active deadline exceeded"
    );
    Ok(())
}

#[test]
fn unfinished_owner_retires_before_successor_send_and_persists_reason() {
    let (result, cleanup, elapsed) = exercise("abandoned-A", true);
    assert!(cleanup.is_empty(), "cleanup failures: {cleanup:?}");
    assert!(
        elapsed < CASE_BUDGET,
        "case exceeded 30 seconds: {elapsed:?}"
    );
    assert_eq!(
        result.expect("attributed retirement schedule"),
        "retired-without-successor-send"
    );
}

#[test]
fn ordinary_sequential_owners_each_receive_their_own_reply() {
    let (result, cleanup, elapsed) = exercise("sequential-control", false);
    assert!(cleanup.is_empty(), "cleanup failures: {cleanup:?}");
    assert!(
        elapsed < CASE_BUDGET,
        "case exceeded 30 seconds: {elapsed:?}"
    );
    assert_eq!(result.expect("actual B caller result"), B_MARKER);
}

#[test]
fn terminal_release_keeps_exact_origin_and_rejects_old_generation() -> Result<()> {
    use std::io::Read as _;
    let state = super::tests::test_state();
    let deadline = Instant::now() + ACTIVE_BUDGET;
    let (primary, mut primary_peer) = socket_pair(deadline)?;
    let primary_origin = state.origin(EventSource::Primary).context("primary")?;
    *lock_or_recover!(primary_origin.socket) = Some(primary.try_clone()?);
    *lock_or_recover!(primary_origin.writer) = Some(Arc::new(WriterSlot::new(
        FramedWriter::from_stream(primary),
    )));
    let generation = state.begin_lazy_generation();
    let source = EventSource::Lazy(generation);
    let (lazy, lazy_peer) = socket_pair(deadline)?;
    let origin = Arc::new(reply_ownership::Origin::default());
    *lock_or_recover!(origin.socket) = Some(lazy.try_clone()?);
    *lock_or_recover!(origin.writer) =
        Some(Arc::new(WriterSlot::new(FramedWriter::from_stream(lazy))));
    lock_or_recover!(state.reply_origins).insert(source, Arc::clone(&origin));
    let mut peer = FramedReader::from_stream(lazy_peer);
    state.release_on_origin(&ff_rdp_core::ReleaseRequest {
        actor_id: "same-actor".into(),
        method: "release",
        origin: Some(source.key()),
    });
    ensure!(
        peer.recv()? == json!({"to":"same-actor","type":"release"}),
        "release missing on original lazy peer"
    );
    primary_peer.set_nonblocking(true)?;
    let mut byte = [0];
    ensure!(
        matches!(primary_peer.read(&mut byte),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock),
        "lazy release reached primary peer"
    );
    ensure!(
        reply_ownership::route(
            &state,
            source,
            &json!({"from":"same-actor","error":"noSuchActor"})
        ),
        "release error escaped sink"
    );
    ensure!(
        lock_or_recover!(origin.ledger).snapshot()["discarded_replies"] == 1,
        "release error not consumed"
    );
    state.invalidate_lazy(generation);
    let new_generation = state.begin_lazy_generation();
    ensure!(new_generation != generation, "generation reused");
    let (replacement, mut replacement_peer) = socket_pair(deadline)?;
    let replacement_origin = Arc::new(reply_ownership::Origin::default());
    *lock_or_recover!(replacement_origin.socket) = Some(replacement.try_clone()?);
    *lock_or_recover!(replacement_origin.writer) = Some(Arc::new(WriterSlot::new(
        FramedWriter::from_stream(replacement),
    )));
    lock_or_recover!(state.reply_origins)
        .insert(EventSource::Lazy(new_generation), replacement_origin);

    state.release_on_origin(&ff_rdp_core::ReleaseRequest {
        actor_id: "same-actor".into(),
        method: "release",
        origin: Some(source.key()),
    });
    state.release_on_origin(&ff_rdp_core::ReleaseRequest {
        actor_id: "same-actor".into(),
        method: "release",
        origin: None,
    });
    ensure!(
        matches!(primary_peer.read(&mut byte),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock),
        "old/absent origin fell back to primary"
    );
    replacement_peer.set_nonblocking(true)?;
    ensure!(
        matches!(replacement_peer.read(&mut byte),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock),
        "old/absent origin reached replacement generation"
    );
    ensure!(!state.is_stopping(), "optional retirement stopped primary");
    ensure!(Instant::now() < deadline, "active bound");
    Ok(())
}

#[test]
fn recovery_sink_requires_primary_origin() -> Result<()> {
    let state = super::tests::test_state();
    let generation = state.begin_lazy_generation();
    let mailbox = Arc::new(BoundedQueue::new(1));
    *lock_or_recover!(state.startup_recovery_reply) =
        Some(("same-watcher".into(), Arc::clone(&mailbox)));
    let origin = Arc::new(reply_ownership::Origin::default());
    lock_or_recover!(origin.ledger)
        .accounting
        .terminal("same-watcher")
        .unwrap();
    lock_or_recover!(state.reply_origins).insert(EventSource::Lazy(generation), origin);
    dispatch_firefox_message_from(
        &state,
        &json!({"from":"same-watcher","marker":"lazy"}),
        None,
        EventSource::Lazy(generation),
    );
    dispatch_firefox_message_from(
        &state,
        &json!({"from":"same-watcher","marker":"primary"}),
        None,
        EventSource::Primary,
    );
    let value = mailbox
        .recv_until(Instant::now() + IO_BUDGET)
        .context("primary recovery response missing")?;
    ensure!(
        value["marker"] == "primary",
        "lazy response completed primary recovery"
    );
    Ok(())
}

#[test]
fn proxy_requires_explicit_valid_reply_contract() {
    assert!(reply_ownership::unwrap(json!({"to":"root","type":"listTabs"})).is_err());
    assert!(reply_ownership::unwrap(json!({"to":"daemon","type":"forward","reply_contract":"ordinary","packet":{"to":"daemon","type":"shutdown"}})).is_err());
    assert!(reply_ownership::unwrap(json!({"to":"daemon","type":"forward","reply_contract":"ordinary","extra":true,"packet":{"to":"root","type":"listTabs"}})).is_err());
    assert!(reply_ownership::unwrap(json!({"to":"daemon","type":"forward","reply_contract":"ordinary","packet":{"to":"root","type":"listTabs"}})).is_ok());
    assert!(reply_ownership::unwrap(json!({"to":"daemon","type":"status"})).is_ok());
}

#[test]
fn attributed_reply_keeps_original_writer_during_empty_handover() -> Result<()> {
    use std::io::Read as _;
    let mut harness = Harness::new("dispatch-before-departure", super::tests::test_state());
    let (a_socket, a_peer) = socket_pair(harness.deadline)?;
    let (b_socket, mut b_peer) = socket_pair(harness.deadline)?;
    for socket in [&a_socket, &a_peer, &b_socket, &b_peer] {
        harness.own_socket(socket)?;
    }
    let a_writer = ClientWriter::with_deadline(a_socket, IO_BUDGET);
    let b_writer = ClientWriter::with_deadline(b_socket, IO_BUDGET);
    let origin = harness
        .state
        .origin(EventSource::Primary)
        .context("primary origin")?;
    lock_or_recover!(origin.ledger)
        .register(
            7,
            &a_writer,
            &json!({"to":"root"}),
            ff_rdp_core::ReplyContract::Ordinary,
        )
        .map_err(anyhow::Error::msg)?;
    *lock_or_recover!(harness.state.rpc_writer) = Some((7, a_writer.clone(), Instant::now()));
    let lease = a_writer
        .hold_for_test(harness.deadline)
        .map_err(|error| anyhow::anyhow!("hold A writer: {error:?}"))?;
    let state = Arc::clone(&harness.state);
    harness.spawn("dispatcher-race", WorkerPolicy::Client, move || {
        ensure!(
            reply_ownership::route(
                &state,
                EventSource::Primary,
                &json!({"from":"root","marker":"A"})
            ),
            "ordinary response not handled"
        );
        Ok(())
    })?;
    ensure!(
        a_writer.wait_for_blocked(harness.deadline),
        "dispatcher did not select A writer"
    );
    ensure!(
        lock_or_recover!(origin.ledger).accounting.pending() == 0,
        "response not attributed before write"
    );
    drop(ClientCleanupGuard {
        state: &harness.state,
        client_id: 7,
    });
    ensure!(
        !harness.state.is_stopping(),
        "empty handover retired origin"
    );
    *lock_or_recover!(harness.state.rpc_writer) = Some((8, b_writer, Instant::now()));
    drop(lease);
    harness.join("dispatcher-race")?;
    let mut reader = FramedReader::from_stream(a_peer);
    ensure!(
        reader.recv()?["marker"] == "A",
        "recorded recipient did not receive A"
    );
    b_peer.set_nonblocking(true)?;
    ensure!(
        matches!(b_peer.read(&mut [0]),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock),
        "reply moved to B writer"
    );
    harness.phase(
        "original_writer_retained",
        &json!({"pending":0,"new_slot_owner":8,"recipient":7}),
    );
    let cleanup = harness.cleanup();
    ensure!(cleanup.is_empty(), "cleanup: {cleanup:?}");
    ensure!(harness.started.elapsed() < CASE_BUDGET, "case budget");
    Ok(())
}

#[test]
fn pending_resource_gc_uses_subscription_origin_on_actual_peer() -> Result<()> {
    use std::io::Read as _;
    let mut harness = Harness::new("gc-origin", super::tests::test_state());
    let (primary, mut primary_peer) = socket_pair(harness.deadline)?;
    let primary_origin = harness
        .state
        .origin(EventSource::Primary)
        .context("primary origin")?;
    *lock_or_recover!(primary_origin.socket) = Some(primary.try_clone()?);
    *lock_or_recover!(primary_origin.writer) = Some(Arc::new(WriterSlot::new(
        FramedWriter::from_stream(primary),
    )));
    let generation = harness.state.begin_lazy_generation();
    let source = EventSource::Lazy(generation);
    let (socket, peer) = socket_pair(harness.deadline)?;
    harness.own_socket(&socket)?;
    harness.own_socket(&peer)?;
    let (observed_tx, observed_rx) = mpsc::sync_channel(1);
    harness.spawn("gc-peer", WorkerPolicy::Client, move || {
        let mut reader = FramedReader::from_stream(peer.try_clone()?);
        let mut writer = FramedWriter::from_stream(peer);
        let subscribe = reader.recv()?;
        ensure!(
            subscribe["to"] == "same-watcher" && subscribe["type"] == "watchResources",
            "wrong setup packet"
        );
        writer.send(&json!({"from":"same-watcher"}))?;
        let gc = reader.recv()?;
        observed_tx.send(gc).context("GC observer missing")?;
        Ok(())
    })?;
    let mut transport = RdpTransport::from_stream(socket)?;
    transport.track_reply_ownership();
    let mut bus = ResourceCommand::new("same-watcher".into());
    let (_, receiver) = bus.subscribe(&mut transport, &[ResourceType::NetworkEvent])?;
    drop(receiver);
    let origin = Arc::new(reply_ownership::Origin::default());
    *lock_or_recover!(origin.socket) = Some(transport.try_clone_stream()?);
    lock_or_recover!(origin.ledger).accounting = transport.take_reply_accounting();
    let (_, writer) = transport.split();
    *lock_or_recover!(origin.writer) = Some(Arc::new(WriterSlot::new(writer)));
    lock_or_recover!(harness.state.reply_origins).insert(source, Arc::clone(&origin));
    bus.dispatch_event(&json!({"type":"resources-available-array","array":[["network-event",[{
        "actor":"network1","method":"GET","url":"https://fixture.invalid/","isXHR":false,
        "cause":{"type":"document"},"startedDateTime":"2026-01-01T00:00:00Z","timeStamp":1000.0,"resourceId":1
    }]]]}));
    bus.gc_with_send(|packet| harness.state.send_owned(source, packet, false));
    let packet = observed_rx
        .recv_timeout(harness.deadline.saturating_duration_since(Instant::now()))
        .context("actual GC packet missing")?;
    ensure!(
        packet
            == json!({"to":"same-watcher","type":"unwatchResources","resourceTypes":["network-event"]}),
        "wrong original-peer GC packet: {packet}"
    );
    primary_peer.set_nonblocking(true)?;
    ensure!(
        matches!(primary_peer.read(&mut [0]),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock),
        "lazy GC reached primary"
    );
    ensure!(
        lock_or_recover!(origin.ledger).snapshot()["oneway_hazard_actors"] == 1,
        "GC error hazard missing"
    );
    harness.join("gc-peer")?;
    harness.phase(
        "gc_original_peer_observed",
        &json!({"generation":generation,"packet":packet,"primary_empty":true}),
    );
    let cleanup = harness.cleanup();
    ensure!(cleanup.is_empty(), "cleanup: {cleanup:?}");
    ensure!(harness.started.elapsed() < CASE_BUDGET, "case budget");
    Ok(())
}

#[path = "retiring_document_control.rs"]
mod retiring_document_control;

#[test]
fn departed_owner_counters_exclude_earlier_orphans_and_wrong_client_cleanup() -> Result<()> {
    let (socket, _peer) = socket_pair(Instant::now() + IO_BUDGET)?;
    let writer = ClientWriter::with_deadline(socket, IO_BUDGET);
    let mut ledger = reply_ownership::Ledger::default();
    ledger
        .register(
            1,
            &writer,
            &json!({"to":"old-a"}),
            ff_rdp_core::ReplyContract::Ordinary,
        )
        .map_err(anyhow::Error::msg)?;
    ensure!(
        ledger.depart(99).is_none(),
        "unrelated cleanup retired origin"
    );
    ensure!(
        ledger.snapshot()["active_pending"] == 1,
        "unrelated cleanup orphaned A"
    );
    ensure!(
        ledger.depart(1).is_none(),
        "A departure retired distinct actors"
    );
    ledger
        .register(
            2,
            &writer,
            &json!({"to":"healthy-b"}),
            ff_rdp_core::ReplyContract::Ordinary,
        )
        .map_err(anyhow::Error::msg)?;
    ensure!(
        ledger.accounting.receive(&json!({"from":"healthy-b"}))
            == Ok(ff_rdp_core::ReplyDisposition::Reply),
        "B completion"
    );
    ensure!(
        ledger.depart(2).is_none(),
        "healthy B departure retired old debt"
    );
    ensure!(
        ledger.snapshot()["departures_with_outstanding"] == 1,
        "old debt counted as B's departure"
    );
    ledger
        .register(
            3,
            &writer,
            &json!({"to":"old-c"}),
            ff_rdp_core::ReplyContract::AsyncEvaluation,
        )
        .map_err(anyhow::Error::msg)?;
    ensure!(
        ledger.depart(3).is_none(),
        "C departure retired disjoint actors"
    );
    let status = ledger.snapshot();
    ensure!(
        status["pending"] == 2
            && status["active_pending"] == 0
            && status["orphan_pending_ordinary"] == 1
            && status["orphan_pending_async"] == 1
            && status["quarantined_actor_count"] == 2
            && status["departures_with_outstanding"] == 2,
        "multiple-owner status lost obligations: {status}"
    );
    ledger
        .register(
            4,
            &writer,
            &json!({"to":"current-d"}),
            ff_rdp_core::ReplyContract::Ordinary,
        )
        .map_err(anyhow::Error::msg)?;
    ensure!(ledger.depart(3).is_none(), "repeated C cleanup retired D");
    ensure!(
        ledger.snapshot()["active_pending"] == 1
            && ledger.snapshot()["departures_with_outstanding"] == 2,
        "stale departure changed D ownership"
    );
    // Bypassing the actual departure boundary is still an origin-wide conflict.
    ensure!(
        ledger.register(
            5,
            &writer,
            &json!({"to":"other-e"}),
            ff_rdp_core::ReplyContract::Ordinary
        ) == Err("owner_changed_with_outstanding"),
        "active owner conflict was admitted"
    );
    ensure!(
        ledger.snapshot()["pending"] == 3,
        "conflicting send reserved or erased debt"
    );
    Ok(())
}
