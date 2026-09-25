//! Iteration 284 controls against actual existing daemon thread paths.
//!
//! The first two contracts failed in the archived, unchanged old product.
//! All fixtures retain actual worker handles and release owned blockers before
//! assertions or unwind. No native Firefox or process-global daemon state.

use super::*;

const CONTROL_BUDGET: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct Probes {
    reader_send: Mutex<Option<ReaderSendGate>>,
    reader_return: Mutex<Option<ReaderSendGate>>,
    reader_entered: Mutex<Option<mpsc::Sender<()>>>,
    client_writer: Mutex<Option<mpsc::Sender<ClientWriter>>>,
    recovery_writer: Mutex<Option<mpsc::Sender<()>>>,
    panic_worker: Mutex<Option<&'static str>>,
    dispatched: Mutex<Option<mpsc::Sender<Value>>>,
    dispatch_gate: Mutex<Option<ReaderSendGate>>,
    client_phase: Mutex<Option<mpsc::Sender<(&'static str, u16)>>>,
}

struct ReaderSendGate {
    entered: mpsc::Sender<()>,
    proceed: mpsc::Receiver<()>,
}

pub(super) struct ReaderReturn<'a>(pub(super) &'a Probes);
impl Drop for ReaderReturn<'_> {
    fn drop(&mut self) {
        if let Some(gate) = self.0.reader_return.lock().expect("return probe").take() {
            let _ = gate.entered.send(());
            let _ = gate.proceed.recv_timeout(CONTROL_BUDGET);
        }
    }
}

impl Probes {
    pub(super) fn reader_entered(&self) {
        if let Some(sender) = self
            .reader_entered
            .lock()
            .expect("reader entered probe")
            .take()
        {
            let _ = sender.send(());
        }
    }

    pub(super) fn maybe_panic(&self, name: &'static str) {
        let panic = {
            let mut selected = self.panic_worker.lock().expect("panic probe");
            if *selected == Some(name) {
                selected.take();
                true
            } else {
                false
            }
        };
        assert!(!panic, "controlled product worker panic: {name}");
    }

    pub(super) fn dispatched(&self, message: &Value) {
        let gate = self.dispatch_gate.lock().expect("dispatch gate").take();
        if let Some(gate) = gate {
            let _ = gate.entered.send(());
            let _ = gate.proceed.recv_timeout(CONTROL_BUDGET);
        }
        if let Some(sender) = &*self.dispatched.lock().expect("dispatch probe") {
            let _ = sender.send(message.clone());
        }
    }

    pub(super) fn client_phase(&self, phase: &'static str, peer_port: u16) {
        if let Some(sender) = &*self.client_phase.lock().expect("client phase probe") {
            let _ = sender.send((phase, peer_port));
        }
    }

    pub(super) fn client_writer_ready(&self, writer: &ClientWriter) {
        if let Some(sender) = self
            .client_writer
            .lock()
            .expect("client writer probe")
            .take()
        {
            let _ = sender.send(writer.clone());
        }
    }

    pub(super) fn before_recovery_writer(&self) {
        if let Some(sender) = self
            .recovery_writer
            .lock()
            .expect("recovery writer probe")
            .take()
        {
            let _ = sender.send(());
        }
    }
    pub(super) fn before_reader_send(&self) {
        let gate = self.reader_send.lock().expect("reader probe lock").take();
        if let Some(gate) = gate {
            let _ = gate.entered.send(());
            // This is a test barrier, not a daemon timeout or lifecycle hold
            // present in product builds. Test cleanup always releases it.
            let _ = gate.proceed.recv_timeout(CONTROL_BUDGET);
        }
    }
}

struct OwnedThread<T> {
    handle: Option<thread::JoinHandle<T>>,
    cleanup: Option<Box<dyn FnOnce()>>,
}

impl<T> OwnedThread<T> {
    fn finish(mut self) -> thread::Result<T> {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
        self.handle
            .take()
            .expect("retained daemon worker handle")
            .join()
    }
}

impl<T> Drop for OwnedThread<T> {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[test]
fn iter284_stop_wakes_reader_backpressure() {
    let (server, peer) = super::tests::loopback_pair();
    let shutdown_socket = server.try_clone().expect("owned reader cleanup clone");
    let state = super::tests::test_state();
    let event_rx = Arc::clone(&state.event_tx);
    state
        .event_tx
        .send(DaemonEvent::Packet(
            EventSource::Primary,
            json!({"test":"already-full"}),
        ))
        .expect("fill event queue");
    let (entered_tx, entered_rx) = mpsc::channel();
    let (proceed_tx, proceed_rx) = mpsc::channel();
    let (returned_tx, returned_rx) = mpsc::channel();
    *state
        .lifecycle_probes
        .reader_send
        .lock()
        .expect("reader probe lock") = Some(ReaderSendGate {
        entered: entered_tx,
        proceed: proceed_rx,
    });
    let state = Arc::new(state);
    let worker_state = Arc::clone(&state);
    let cleanup_state = Arc::clone(&state);
    let cleanup_proceed = proceed_tx.clone();
    let reader = OwnedThread {
        handle: Some(thread::spawn(move || {
            firefox_reader_loop(&worker_state, FramedReader::from_stream(server));
            let _ = returned_tx.send(());
        })),
        cleanup: Some(Box::new(move || {
            cleanup_state.stop(StopReason::AuthenticatedShutdown);
            let _ = cleanup_proceed.send(());
            let _ = shutdown_socket.shutdown(std::net::Shutdown::Both);
            // Emergency fixture cleanup remains independent of the verdict.
            // The archived old control dropped its std-mpsc receiver here.
            event_rx.close();
        })),
    };
    let mut peer_writer = FramedWriter::from_stream(peer);
    let sent = peer_writer.send(&json!({"from":"root", "test":"next-frame"}));
    let reached_send_boundary = entered_rx.recv_timeout(CONTROL_BUDGET).is_ok();
    // The production stop entry replaces the old flag-only fixture action;
    // the before/after return contract and observation budget are unchanged.
    state.stop(StopReason::AuthenticatedShutdown);
    let _ = proceed_tx.send(());
    let returned_before_cleanup = returned_rx.recv_timeout(CLIENT_DRAIN_BUDGET).is_ok();
    let joined = reader.finish();
    drop(peer_writer);
    assert!(sent.is_ok(), "owned peer must deliver the test frame");
    assert!(
        reached_send_boundary,
        "actual Firefox reader must reach the full-queue boundary"
    );
    assert!(
        joined.is_ok(),
        "actual Firefox reader must join after owned cleanup"
    );
    assert!(
        returned_before_cleanup,
        "stop must release the reader without dropping its event receiver"
    );
}

#[test]
fn iter284_stopped_rpc_claimant_cannot_claim() {
    let (server, _peer) = super::tests::loopback_pair();
    let writer = ClientWriter::new(server);
    let state = Arc::new(super::tests::test_state());
    let mut slot = state.rpc_writer.lock().expect("hold RPC slot until stop");
    *slot = Some((1, writer.clone(), Instant::now()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let claimant_state = Arc::clone(&state);
    let claimant = thread::spawn(move || {
        let _ = entered_tx.send(());
        claim_rpc_slot_queued(&claimant_state, 2, &writer, None, CLIENT_DRAIN_BUDGET)
    });
    let entered = entered_rx.recv_timeout(CONTROL_BUDGET).is_ok();
    // Stop wins before the slot becomes available. The old claimant ignores
    // this state and takes the slot as soon as the mutex is released.
    let stop_state = Arc::clone(&state);
    let stopper = thread::spawn(move || stop_state.stop(StopReason::AuthenticatedShutdown));
    let stop_published = state
        .cancellation
        .wait_until(Instant::now() + CONTROL_BUDGET);
    *slot = None;
    drop(slot);
    notify_rpc_slot_released(&state);
    stopper.join().expect("actual stop caller join");
    let result = claimant.join().expect("actual RPC claimant join");
    assert!(
        stop_published,
        "stop wins the common admission gate while RPC mutex remains occupied"
    );
    assert!(
        entered,
        "claimant must start before releasing the occupied RPC mutex"
    );
    assert!(
        !matches!(result, RpcClaim::Claimed),
        "a claimant must not acquire after stop linearizes"
    );
    assert!(
        state
            .rpc_writer
            .lock()
            .expect("RPC slot after join")
            .is_none()
    );
}

// Own all fixture sockets, cancellation registrations and actual product worker
// handles. Drop runs on assertions too; no timed observation is counted as join.
struct Runtime {
    state: Arc<SharedState>,
    workers: WorkerOwner,
    writer: FirefoxWriter,
    peer: TcpStream,
    reader_socket: Option<TcpStream>,
    _registrations: Vec<Registration>,
}

impl Runtime {
    fn new(state: SharedState) -> Self {
        let state = Arc::new(state);
        let (socket, peer) = super::tests::loopback_pair();
        peer.set_read_timeout(Some(CONTROL_BUDGET))
            .expect("peer timeout");
        let reader_socket = Some(socket.try_clone().expect("reader clone"));
        let socket_registration = state
            .cancellation
            .register_socket(&socket)
            .expect("socket wake");
        let writer = Arc::new(WriterSlot::new(FramedWriter::from_stream(socket)));
        let writer_registration = register_firefox_writer(&state.cancellation, &writer);
        let workers = WorkerOwner::new(Arc::clone(&state.cancellation));
        Self {
            state,
            workers,
            writer,
            peer,
            reader_socket,
            _registrations: vec![socket_registration, writer_registration],
        }
    }

    fn reader(&mut self) {
        let socket = self.reader_socket.take().expect("one primary reader");
        let state = Arc::clone(&self.state);
        self.workers
            .spawn("firefox-reader", WorkerPolicy::Required, move || {
                firefox_reader_loop(&state, FramedReader::from_stream(socket));
            })
            .expect("reader acquisition");
    }

    fn dispatcher(&mut self, initial: VecDeque<Value>) -> mpsc::Receiver<Value> {
        self.dispatcher_with_setup(initial, Arc::new(BoundedQueue::new(1)))
    }

    fn dispatcher_with_setup(
        &mut self,
        initial: VecDeque<Value>,
        setup: Arc<BoundedQueue<LazySubscription>>,
    ) -> mpsc::Receiver<Value> {
        let (tx, rx) = mpsc::channel();
        *self
            .state
            .lifecycle_probes
            .dispatched
            .lock()
            .expect("dispatch probe") = Some(tx);
        let state = Arc::clone(&self.state);
        let events = Arc::clone(&self.state.event_tx);
        let writer = Arc::clone(&self.writer);
        self.workers
            .spawn("event-dispatcher", WorkerPolicy::Required, move || {
                event_dispatcher_loop(&state, events, initial, None, None, setup, writer);
            })
            .expect("dispatcher acquisition");
        rx
    }

    fn handler(
        &mut self,
    ) -> (
        FramedReader,
        FramedWriter,
        mpsc::Receiver<(&'static str, u16)>,
    ) {
        let (socket, peer) = super::tests::loopback_pair();
        peer.set_read_timeout(Some(CONTROL_BUDGET))
            .expect("client timeout");
        let (phase_tx, phase_rx) = mpsc::channel();
        *self
            .state
            .lifecycle_probes
            .client_phase
            .lock()
            .expect("client probe") = Some(phase_tx);
        let state = Arc::clone(&self.state);
        let writer = Arc::clone(&self.writer);
        self.workers
            .spawn("client-handler", WorkerPolicy::Client, move || {
                let result = handle_client(&state, socket, &writer);
                assert!(
                    result.is_ok() || state.is_stopping(),
                    "unexpected handler failure: {result:?}"
                );
            })
            .expect("handler acquisition");
        (
            FramedReader::from_stream(peer.try_clone().expect("client read clone")),
            FramedWriter::from_stream(peer),
            phase_rx,
        )
    }

    fn stop(&mut self) -> Vec<super::super::lifecycle::JoinRecord> {
        self.state.stop(StopReason::AuthenticatedShutdown);
        self.workers
            .shutdown_and_join(StopReason::AuthenticatedShutdown)
            .to_vec()
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.state.stop(StopReason::OwnerDropped);
        self.workers.shutdown_and_join(StopReason::OwnerDropped);
    }
}

fn expect_phase(receiver: &mpsc::Receiver<(&'static str, u16)>, wanted: &'static str) {
    loop {
        let (phase, _) = receiver
            .recv_timeout(CONTROL_BUDGET)
            .expect("actual handler reached phase");
        if phase == wanted {
            return;
        }
    }
}

#[test]
fn iter284_auth_and_idle_handlers_are_joined() {
    for authenticate in [false, true] {
        let mut runtime = Runtime::new(super::tests::test_state());
        let (mut reader, mut writer, phases) = runtime.handler();
        expect_phase(&phases, "auth");
        if authenticate {
            writer
                .send(&json!({"auth":"test-token"}))
                .expect("authenticate");
            assert_eq!(
                reader.recv().expect("normal greeting")["applicationType"],
                "browser"
            );
            expect_phase(&phases, "idle");
        }
        let joined = runtime.stop();
        assert_eq!(joined.len(), 1);
        assert_eq!(
            joined[0].outcome,
            super::super::lifecycle::WorkerOutcome::Stopped
        );
        assert!(joined[0].supervision_returned);
        assert!(
            runtime
                .state
                .rpc_writer
                .lock()
                .expect("RPC after join")
                .is_none()
        );
        assert!(
            runtime
                .state
                .stream_subs
                .lock()
                .expect("subscribers after join")
                .is_empty()
        );
    }
}

#[test]
fn iter284_shutdown_handler_budget_includes_occupied_writer() {
    let mut runtime = Runtime::new(super::tests::test_state());
    let (tx, rx) = mpsc::channel();
    *runtime
        .state
        .lifecycle_probes
        .client_writer
        .lock()
        .expect("writer probe") = Some(tx);
    let (mut reader, mut peer_writer, phases) = runtime.handler();
    peer_writer
        .send(&json!({"auth":"test-token"}))
        .expect("auth");
    reader.recv().expect("greeting");
    let writer = rx
        .recv_timeout(CONTROL_BUDGET)
        .expect("actual handler writer");
    let lease = writer
        .hold_for_test(Instant::now() + CONTROL_BUDGET)
        .expect("occupy response writer");
    peer_writer
        .send(&json!({"to":"daemon","type":"shutdown"}))
        .expect("shutdown request");
    expect_phase(&phases, "shutdown");
    let stopped = runtime
        .state
        .cancellation
        .wait_until(Instant::now() + CLIENT_WRITE_DEADLINE + CONTROL_BUDGET);
    // Keep the writer occupied across the actual join. The real handler must
    // expire its existing budget and stop even though no lease can be obtained.
    let joined = if stopped {
        runtime.workers.join_acquired().to_vec()
    } else {
        runtime.stop()
    };
    drop(lease);
    assert!(
        stopped,
        "shutdown requester must not wait indefinitely for its response writer"
    );
    assert_eq!(
        runtime.state.cancellation.reason(),
        Some(StopReason::AuthenticatedShutdown)
    );
    assert_eq!(joined.len(), 1);
    assert!(joined[0].supervision_returned);
}

#[test]
fn iter284_recovery_holding_rpc_is_woken_before_rpc_notification() {
    let mut runtime = Runtime::new(super::tests::startup_state());
    dispatch_firefox_message(
        &runtime.state,
        &super::tests::startup_form("old", "about:blank"),
        None,
    );
    dispatch_firefox_message(&runtime.state, &super::tests::startup_destroy("old"), None);
    let (owner_socket, _owner_peer) = super::tests::loopback_pair();
    *runtime.state.rpc_writer.lock().expect("RPC owner") =
        Some((7, ClientWriter::new(owner_socket), Instant::now()));
    let writer = Arc::clone(&runtime.writer);
    let lease = writer
        .acquire(Instant::now() + CONTROL_BUDGET)
        .expect("occupied Firefox writer");
    let (entered_tx, entered_rx) = mpsc::channel();
    *runtime
        .state
        .lifecycle_probes
        .recovery_writer
        .lock()
        .expect("recovery probe") = Some(entered_tx);
    let (result_tx, result_rx) = mpsc::channel();
    let state = Arc::clone(&runtime.state);
    let worker_writer = Arc::clone(&writer);
    runtime
        .workers
        .spawn("client-handler", WorkerPolicy::Client, move || {
            let value = recover_startup_target(
                &state,
                &json!({"descriptor":"tab","remaining_ms":5000}),
                7,
                &worker_writer,
            );
            let _ = result_tx.send(value);
        })
        .expect("recovery worker");
    entered_rx
        .recv_timeout(CONTROL_BUDGET)
        .expect("sink installed while RPC mutex held");
    let joined = runtime.stop();
    let value = result_rx
        .recv_timeout(CONTROL_BUDGET)
        .expect("recovery body returned");
    assert_eq!(value["error"], "startup_recovery");
    assert_eq!(joined.len(), 1);
    assert!(
        runtime
            .state
            .startup_recovery_reply
            .lock()
            .expect("retained sink")
            .is_some()
    );
    dispatch_firefox_message(&runtime.state, &json!({"from":"watcher"}), None);
    assert!(
        runtime
            .state
            .startup_recovery_reply
            .lock()
            .expect("late reply remains owned")
            .is_some()
    );
    drop(lease);
}

#[test]
fn iter284_startup_replay_over_capacity_precedes_live_fifo() {
    let mut runtime = Runtime::new(super::tests::test_state());
    let initial: VecDeque<_> = (0..4097)
        .map(|index| json!({"from":"ordinary","sequence":index}))
        .collect();
    runtime
        .state
        .event_tx
        .send(DaemonEvent::Packet(
            EventSource::Primary,
            json!({"from":"ordinary","sequence":4097}),
        ))
        .expect("one live frame");
    let received = runtime.dispatcher(initial);
    let actual: Vec<_> = (0..4098)
        .map(|_| {
            received
                .recv_timeout(CONTROL_BUDGET)
                .expect("actual dispatch frame")["sequence"]
                .as_u64()
                .expect("sequence")
        })
        .collect();
    let joined = runtime.stop();
    assert_eq!(actual, (0..4098).collect::<Vec<_>>());
    assert_eq!(joined.len(), 1);
    assert!(!runtime.state.dispatcher.alive.load(Ordering::Relaxed));
}

#[test]
fn iter284_invalid_lazy_generation_cannot_republish_queued_target() {
    let mut runtime = Runtime::new(super::tests::test_state_with_queues(
        4,
        ff_rdp_core::release_queue(1).0,
    ));
    let generation = runtime.state.begin_lazy_generation();
    dispatch_firefox_message_from(
        &runtime.state,
        &super::tests::startup_form("lazy-old", "https://lazy.invalid/"),
        None,
        EventSource::Lazy(generation),
    );
    runtime
        .state
        .event_tx
        .send(DaemonEvent::Packet(
            EventSource::Lazy(generation),
            super::tests::startup_form("lazy-queued", "https://lazy.invalid/"),
        ))
        .expect("queued optional packet");
    let setup = Arc::new(BoundedQueue::new(1));
    let (_resource_tx, resource_rx) = mpsc::channel();
    assert!(
        setup
            .send(LazySubscription {
                generation,
                watcher: "stale-watcher".into(),
                bus: ResourceCommand::new("stale-watcher".into()),
                rx: resource_rx
            })
            .is_ok()
    );
    runtime.state.invalidate_lazy(generation);
    runtime
        .state
        .event_tx
        .send(DaemonEvent::Packet(
            EventSource::Primary,
            super::tests::startup_form("primary", "https://primary.invalid/"),
        ))
        .expect("main packet");
    let received = runtime.dispatcher_with_setup(VecDeque::new(), setup);
    let actual = received
        .recv_timeout(CONTROL_BUDGET)
        .expect("main dispatch survives optional loss");
    assert!(
        runtime
            .state
            .watcher_actor
            .lock()
            .expect("watcher after invalid setup")
            .is_empty()
    );
    let joined = runtime.stop();
    assert_eq!(actual["target"]["actor"], "primary");
    assert_eq!(
        runtime
            .state
            .top_level_target
            .lock()
            .expect("main target")
            .as_ref()
            .map(AsRef::as_ref),
        Some("primary")
    );
    assert!(
        runtime
            .state
            .frame_targets
            .lock()
            .expect("frames")
            .iter()
            .all(|packet| packet["target"]["actor"] != "lazy-queued")
    );
    assert_eq!(joined.len(), 1);
}

#[test]
fn iter284_empty_reader_dispatcher_and_release_queue_stop_join() {
    let (releases, receiver) = ff_rdp_core::release_queue(4);
    let mut runtime = Runtime::new(super::tests::test_state_with_queues(4, releases));
    runtime.reader();
    runtime.dispatcher(VecDeque::new());
    assert!(
        runtime
            .state
            .event_tx
            .wait_for_blocked(false, Instant::now() + CONTROL_BUDGET),
        "actual dispatcher waits for its empty queue"
    );
    let state = Arc::clone(&runtime.state);
    let writer = Arc::clone(&runtime.writer);
    runtime
        .workers
        .spawn("grip-release-drainer", WorkerPolicy::Required, move || {
            grip_release_drainer_loop(&state, receiver, writer);
        })
        .expect("drainer");
    let joined = runtime.stop();
    assert_eq!(joined.len(), 3);
    assert!(joined.iter().all(|record| record.supervision_returned
        && record.outcome == super::super::lifecycle::WorkerOutcome::Stopped));
    let mut peer = FramedReader::from_stream(runtime.peer.try_clone().expect("peer read"));
    assert!(
        peer.recv().is_err(),
        "private wake must never be a wire frame"
    );
}

#[test]
fn iter284_full_release_queue_and_occupied_firefox_writer_stop_join() {
    let (releases, receiver) = ff_rdp_core::release_queue(4);
    for index in 0..4 {
        releases
            .try_send(ff_rdp_core::ReleaseRequest {
                actor_id: format!("grip{index}").into(),
                method: "release",
            })
            .expect("fill release queue");
    }
    let mut runtime = Runtime::new(super::tests::test_state_with_queues(4, releases));
    let writer = Arc::clone(&runtime.writer);
    let lease = writer
        .acquire(Instant::now() + CONTROL_BUDGET)
        .expect("occupy Firefox writer");
    let state = Arc::clone(&runtime.state);
    let contender = Arc::clone(&writer);
    runtime
        .workers
        .spawn("grip-release-drainer", WorkerPolicy::Required, move || {
            grip_release_drainer_loop(&state, receiver, contender);
        })
        .expect("drainer");
    let blocked = writer.wait_for_blocked(Instant::now() + CONTROL_BUDGET);
    let joined = runtime.stop();
    assert!(
        blocked,
        "actual drainer must contend for the occupied writer"
    );
    // Join precedes releasing the writer, so the writer close must wake waiters.
    drop(lease);
    assert_eq!(joined.len(), 1);
    let mut peer = FramedReader::from_stream(runtime.peer.try_clone().expect("peer read"));
    assert!(peer.recv().is_err());
}

#[test]
fn iter284_required_reader_panic_cancels_and_joins_siblings() {
    let mut runtime = Runtime::new(super::tests::test_state());
    *runtime
        .state
        .lifecycle_probes
        .panic_worker
        .lock()
        .expect("panic probe") = Some("firefox-reader");
    runtime.dispatcher(VecDeque::new());
    runtime.reader();
    let stopped = runtime
        .state
        .cancellation
        .wait_until(Instant::now() + CONTROL_BUDGET);
    let joined = runtime.workers.join_acquired().to_vec();
    assert!(stopped);
    assert_eq!(
        runtime.state.cancellation.reason(),
        Some(StopReason::WorkerPanicked("firefox-reader"))
    );
    assert!(joined.iter().any(|record| record.name == "firefox-reader"
        && record.outcome == super::super::lifecycle::WorkerOutcome::Panicked));
    assert!(!runtime.workers.completed_successfully());
}

#[test]
fn iter284_partial_acquisition_and_unwind_join_actual_reader() {
    for unwind in [false, true] {
        let (joined_tx, joined_rx) = mpsc::channel();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut runtime = Runtime::new(super::tests::test_state());
            runtime.workers.observe_joins(joined_tx);
            runtime.reader();
            assert!(!unwind, "controlled owner unwind after reader acquisition");
            runtime.workers.fail_next_acquisition();
            assert!(
                runtime
                    .workers
                    .spawn("unacquired", WorkerPolicy::Required, || {})
                    .is_err()
            );
            // Scope drop follows the same owner cleanup path used by run_daemon's
            // fallible later acquisitions. An acquired reader is actually joined.
        }));
        assert_eq!(result.is_err(), unwind);
        let joined = joined_rx
            .recv_timeout(CONTROL_BUDGET)
            .expect("join observed before owner scope returned");
        assert_eq!(joined.name, "firefox-reader");
        assert!(joined.supervision_returned);
        assert!(
            joined_rx.try_recv().is_err(),
            "failed acquisition created no handle"
        );
    }
}

#[test]
fn iter284_optional_setup_failure_returns_without_stopping_primary() {
    let mut runtime = Runtime::new(super::tests::test_state());
    let listener = TcpListener::bind("127.0.0.1:0").expect("owned optional endpoint");
    let peer = listener.local_addr().expect("optional address");
    let cancellation = Arc::clone(&runtime.state.cancellation);
    let state = Arc::clone(&runtime.state);
    let setup = Arc::new(BoundedQueue::new(1));
    runtime
        .workers
        .spawn("watcher-establisher", WorkerPolicy::Optional, move || {
            background_establish_watcher_loop(&state, peer, CONTROL_BUDGET, &setup);
        })
        .expect("optional worker");
    let socket = accept_controlled(listener);
    let mut writer = FramedWriter::from_stream(socket);
    writer
        .send(&json!({"applicationType":"controlled-invalid-greeting"}))
        .expect("invalid greeting");
    let joined = runtime.workers.join_acquired().to_vec();
    assert_eq!(
        joined[0].outcome,
        super::super::lifecycle::WorkerOutcome::OptionalReturned
    );
    assert_eq!(cancellation.reason(), None);
    assert_eq!(
        runtime.state.lazy.lock().expect("lazy generation").active,
        None
    );
    // A normal primary event still routes after the optional worker returned.
    runtime
        .state
        .event_tx
        .send(DaemonEvent::Packet(
            EventSource::Primary,
            json!({"from":"ordinary","sequence":7}),
        ))
        .expect("main remains admitted");
    let received = runtime.dispatcher(VecDeque::new());
    assert_eq!(
        received
            .recv_timeout(CONTROL_BUDGET)
            .expect("main proxy dispatch")["sequence"],
        7
    );
    runtime.stop();
}

#[test]
fn iter284_optional_panic_is_fatal_and_invalidates_generation() {
    let mut runtime = Runtime::new(super::tests::test_state());
    *runtime
        .state
        .lifecycle_probes
        .panic_worker
        .lock()
        .expect("panic probe") = Some("watcher-establisher");
    let listener = TcpListener::bind("127.0.0.1:0").expect("owned optional endpoint");
    let peer = listener.local_addr().expect("optional address");
    let state = Arc::clone(&runtime.state);
    runtime
        .workers
        .spawn("watcher-establisher", WorkerPolicy::Optional, move || {
            background_establish_watcher_loop(
                &state,
                peer,
                CONTROL_BUDGET,
                &Arc::new(BoundedQueue::new(1)),
            );
        })
        .expect("optional worker");
    let joined = runtime.workers.join_acquired().to_vec();
    assert_eq!(
        joined[0].outcome,
        super::super::lifecycle::WorkerOutcome::Panicked
    );
    assert_eq!(
        runtime.state.cancellation.reason(),
        Some(StopReason::WorkerPanicked("watcher-establisher"))
    );
    assert_eq!(
        runtime
            .state
            .lazy
            .lock()
            .expect("invalidated optional generation")
            .active,
        None
    );
}

fn accept_controlled(listener: TcpListener) -> TcpStream {
    listener
        .set_nonblocking(true)
        .expect("nonblocking owned listener");
    let mut listener = mio::net::TcpListener::from_std(listener);
    let mut poll = mio::Poll::new().expect("owned peer poll");
    poll.registry()
        .register(&mut listener, mio::Token(0), mio::Interest::READABLE)
        .expect("listener readiness");
    let deadline = Instant::now() + CONTROL_BUDGET;
    let mut events = mio::Events::with_capacity(1);
    loop {
        match listener.accept() {
            Ok((socket, _)) => {
                let socket = TcpStream::from(socket);
                socket.set_nonblocking(false).expect("blocking peer");
                return socket;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("owned accept failed: {error}"),
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "optional worker must reach owned endpoint"
        );
        poll.poll(&mut events, Some(remaining))
            .expect("owned accept readiness");
    }
}

#[test]
fn iter284_continuous_initial_input_observes_stop_between_frames() {
    let mut runtime = Runtime::new(super::tests::test_state());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (proceed_tx, proceed_rx) = mpsc::channel();
    *runtime
        .state
        .lifecycle_probes
        .dispatch_gate
        .lock()
        .expect("dispatch barrier") = Some(ReaderSendGate {
        entered: entered_tx,
        proceed: proceed_rx,
    });
    let initial = (0..128)
        .map(|sequence| json!({"from":"ordinary","sequence":sequence}))
        .collect();
    let received = runtime.dispatcher(initial);
    let reached = entered_rx.recv_timeout(CONTROL_BUDGET).is_ok();
    runtime.state.stop(StopReason::AuthenticatedShutdown);
    let _ = proceed_tx.send(());
    let joined = runtime.workers.join_acquired().to_vec();
    assert!(reached);
    assert_eq!(
        received.try_iter().count(),
        1,
        "stop must win despite remaining continuously available catch-up frames"
    );
    assert_eq!(joined.len(), 1);
    assert_eq!(
        runtime
            .state
            .dispatcher
            .frames_finished
            .load(Ordering::Relaxed),
        1
    );
}

#[test]
fn iter284_handler_panic_runs_cleanup_and_is_fatal() {
    let mut runtime = Runtime::new(super::tests::test_state());
    *runtime
        .state
        .lifecycle_probes
        .panic_worker
        .lock()
        .expect("panic probe") = Some("client-handler");
    let (mut reader, mut writer, _) = runtime.handler();
    writer.send(&json!({"auth":"test-token"})).expect("auth");
    reader.recv().expect("greeting before controlled panic");
    let stopped = runtime
        .state
        .cancellation
        .wait_until(Instant::now() + CONTROL_BUDGET);
    let joined = if stopped {
        runtime.workers.join_acquired().to_vec()
    } else {
        runtime.stop()
    };
    assert!(stopped);
    assert_eq!(
        joined[0].outcome,
        super::super::lifecycle::WorkerOutcome::Panicked
    );
    assert!(
        runtime
            .state
            .rpc_writer
            .lock()
            .expect("RPC cleanup")
            .is_none()
    );
    assert!(
        runtime
            .state
            .stream_subs
            .lock()
            .expect("subscription cleanup")
            .is_empty()
    );
    assert!(!runtime.workers.completed_successfully());
}

#[test]
fn iter284_occupied_client_writer_releases_actual_dispatcher_and_handler() {
    let mut runtime = Runtime::new(super::tests::test_state());
    let (writer_tx, writer_rx) = mpsc::channel();
    *runtime
        .state
        .lifecycle_probes
        .client_writer
        .lock()
        .expect("writer probe") = Some(writer_tx);
    let (mut reader, mut peer_writer, phases) = runtime.handler();
    peer_writer
        .send(&json!({"auth":"test-token"}))
        .expect("auth");
    reader.recv().expect("greeting");
    expect_phase(&phases, "idle");
    let writer = writer_rx
        .recv_timeout(CONTROL_BUDGET)
        .expect("actual client writer");
    *runtime
        .state
        .rpc_writer
        .lock()
        .expect("install active owner") = Some((1, writer.clone(), Instant::now()));
    let lease = writer
        .hold_for_test(Instant::now() + CONTROL_BUDGET)
        .expect("occupied client writer");
    runtime
        .state
        .event_tx
        .send(DaemonEvent::Packet(
            EventSource::Primary,
            json!({"from":"ordinary","value":42}),
        ))
        .expect("reply queued");
    runtime.dispatcher(VecDeque::new());
    let blocked = writer.wait_for_blocked(Instant::now() + CONTROL_BUDGET);
    let joined = runtime.stop();
    drop(lease);
    assert!(
        blocked,
        "actual dispatcher must reach client writer contention"
    );
    assert_eq!(joined.len(), 2);
    assert!(
        runtime
            .state
            .rpc_writer
            .lock()
            .expect("actual handler cleanup")
            .is_none()
    );
    assert!(!runtime.state.dispatcher.alive.load(Ordering::Relaxed));
}

#[test]
fn iter284_normal_handler_requests_and_primary_replies_remain_ordered() {
    let mut runtime = Runtime::new(super::tests::test_state_with_queues(
        4,
        ff_rdp_core::release_queue(1).0,
    ));
    let (mut reader, mut writer, _) = runtime.handler();
    writer.send(&json!({"auth":"test-token"})).expect("auth");
    reader.recv().expect("greeting");
    runtime.reader();
    runtime.dispatcher(VecDeque::new());
    let mut firefox_reader =
        FramedReader::from_stream(runtime.peer.try_clone().expect("Firefox peer reader"));
    let mut firefox_writer =
        FramedWriter::from_stream(runtime.peer.try_clone().expect("Firefox peer writer"));
    for sequence in 0..4 {
        let request = json!({"to":"root","type":"ordinary","sequence":sequence});
        writer.send(&request).expect("ordinary request");
        assert_eq!(
            firefox_reader.recv().expect("actual forwarded request"),
            request
        );
        let response = json!({"from":"root","sequence":sequence});
        firefox_writer.send(&response).expect("ordinary response");
        assert_eq!(reader.recv().expect("actual forwarded response"), response);
    }
    let joined = runtime.stop();
    assert_eq!(joined.len(), 3);
    assert!(joined.iter().all(|record| record.supervision_returned));
}

#[test]
fn iter284_owner_cannot_finish_before_actual_reader_return_and_join() {
    let mut runtime = Runtime::new(super::tests::test_state());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (proceed_tx, proceed_rx) = mpsc::channel();
    *runtime
        .state
        .lifecycle_probes
        .reader_return
        .lock()
        .expect("return barrier") = Some(ReaderSendGate {
        entered: entered_tx,
        proceed: proceed_rx,
    });
    runtime.reader();
    // The first real frame proves the reader entered its body before stop.
    let mut peer = FramedWriter::from_stream(runtime.peer.try_clone().expect("peer"));
    peer.send(&json!({"from":"ordinary","sequence":0}))
        .expect("reader frame");
    assert!(
        runtime
            .state
            .event_tx
            .recv_until(Instant::now() + CONTROL_BUDGET)
            .is_some()
    );
    let state = Arc::clone(&runtime.state);
    let (done_tx, done_rx) = mpsc::channel();
    let cleanup = proceed_tx.clone();
    let owner = OwnedThread {
        handle: Some(thread::spawn(move || {
            let records = runtime.stop();
            let _ = done_tx.send(records);
        })),
        cleanup: Some(Box::new(move || {
            let _ = cleanup.send(());
        })),
    };
    let reached_epilogue = entered_rx.recv_timeout(CONTROL_BUDGET).is_ok();
    let completed_before_worker_return = done_rx.try_recv().is_ok();
    let _ = proceed_tx.send(());
    owner.finish().expect("actual owner thread join");
    let joined = done_rx
        .recv_timeout(CONTROL_BUDGET)
        .expect("owner result after actual reader join");
    assert!(reached_epilogue);
    assert!(
        !completed_before_worker_return,
        "owner completion cannot precede its reader's return"
    );
    assert_eq!(joined.len(), 1);
    assert!(joined[0].supervision_returned);
    assert!(state.is_stopping());
}

#[test]
fn iter284_optional_greeting_cancel_and_established_eof_are_joined() {
    for finish_setup in [false, true] {
        let mut runtime = Runtime::new(super::tests::test_state());
        let listener = TcpListener::bind("127.0.0.1:0").expect("owned watcher endpoint");
        let peer = listener.local_addr().expect("watcher address");
        let setup = Arc::new(BoundedQueue::new(1));
        let worker_setup = Arc::clone(&setup);
        let state = Arc::clone(&runtime.state);
        runtime
            .workers
            .spawn("watcher-establisher", WorkerPolicy::Optional, move || {
                background_establish_watcher_loop(&state, peer, CONTROL_BUDGET, &worker_setup);
            })
            .expect("watcher acquisition");
        let socket = accept_controlled(listener);
        socket
            .set_read_timeout(Some(CONTROL_BUDGET))
            .expect("peer read deadline");
        let mut reader = FramedReader::from_stream(socket.try_clone().expect("watcher peer read"));
        let mut writer = FramedWriter::from_stream(socket.try_clone().expect("watcher peer write"));
        if finish_setup {
            writer
                .send(&json!({"from":"root","applicationType":"browser"}))
                .expect("greeting");
            for (method, reply) in [
                (
                    "listTabs",
                    json!({"from":"root","tabs":[{"actor":"tab","browsingContextID":11}]}),
                ),
                ("getWatcher", json!({"from":"tab","actor":"lazy-watcher"})),
                ("watchTargets", json!({"from":"lazy-watcher"})),
                ("watchResources", json!({"from":"lazy-watcher"})),
            ] {
                let request = reader.recv().expect("actual watcher setup request");
                assert_eq!(request["type"], method);
                writer.send(&reply).expect("watcher setup reply");
            }
            socket
                .shutdown(std::net::Shutdown::Both)
                .expect("controlled optional EOF");
            let joined = runtime.workers.join_acquired().to_vec();
            assert_eq!(
                joined[0].outcome,
                super::super::lifecycle::WorkerOutcome::OptionalReturned
            );
            assert_eq!(runtime.state.cancellation.reason(), None);
            assert_eq!(
                runtime
                    .state
                    .lazy
                    .lock()
                    .expect("retired generation")
                    .active,
                None
            );
            assert!(
                setup.try_recv().is_some(),
                "actual subscription reached its handoff before EOF"
            );
        } else {
            let joined = runtime.stop();
            assert_eq!(
                joined[0].outcome,
                super::super::lifecycle::WorkerOutcome::Stopped
            );
            assert!(joined[0].supervision_returned);
        }
    }
}

#[test]
fn iter284_completed_client_is_joined_and_releases_its_rpc_owner() {
    let mut runtime = Runtime::new(super::tests::test_state());
    let (mut reader, mut writer, _) = runtime.handler();
    writer.send(&json!({"auth":"test-token"})).expect("auth");
    reader.recv().expect("greeting");
    writer
        .send(&json!({"to":"root","type":"ordinary"}))
        .expect("claim and forward");
    let mut firefox_peer = FramedReader::from_stream(runtime.peer.try_clone().expect("peer read"));
    assert_eq!(
        firefox_peer.recv().expect("forwarded request")["type"],
        "ordinary"
    );
    assert!(
        runtime
            .state
            .rpc_writer
            .lock()
            .expect("active RPC owner")
            .is_some()
    );
    drop(reader);
    drop(writer);
    let joined = runtime.workers.join_acquired().to_vec();
    assert_eq!(joined.len(), 1);
    assert_eq!(
        joined[0].outcome,
        super::super::lifecycle::WorkerOutcome::ClientReturned
    );
    assert!(joined[0].supervision_returned);
    assert_eq!(runtime.state.cancellation.reason(), None);
    assert!(
        runtime
            .state
            .rpc_writer
            .lock()
            .expect("completed handler RPC cleanup")
            .is_none()
    );
}

thread_local! {
    static RUN_CONTROL: std::cell::RefCell<Option<RunControl>> = const { std::cell::RefCell::new(None) };
}

pub(super) struct RunControl {
    ready: mpsc::Sender<RunReady>,
    reader_return: Option<ReaderSendGate>,
    reader_entered_tx: Option<mpsc::Sender<()>>,
    reader_entered_rx: mpsc::Receiver<()>,
    joins: mpsc::Sender<super::super::lifecycle::JoinRecord>,
    cleanup_state: Arc<Mutex<Option<Arc<SharedState>>>>,
    fail_dispatcher: bool,
}

struct RunReady {
    proxy_port: u16,
    token: String,
}

pub(super) fn take_run_control() -> Option<RunControl> {
    RUN_CONTROL.with(|slot| slot.borrow_mut().take())
}

impl RunControl {
    pub(super) fn attach(
        &mut self,
        state: &Arc<SharedState>,
        owner: &mut WorkerOwner,
        info: &DaemonInfo,
    ) {
        *state
            .lifecycle_probes
            .reader_return
            .lock()
            .expect("run reader gate") = self.reader_return.take();
        *state
            .lifecycle_probes
            .reader_entered
            .lock()
            .expect("run reader entry") = self.reader_entered_tx.take();
        *self.cleanup_state.lock().expect("run cleanup state") = Some(Arc::clone(state));
        owner.observe_joins(self.joins.clone());
        let _ = self.ready.send(RunReady {
            proxy_port: info.proxy_port,
            token: info.auth_token.clone(),
        });
    }

    pub(super) fn before_dispatcher_acquisition(&self, owner: &mut WorkerOwner) -> Result<()> {
        // Only the isolated fixture installs this control. It synchronizes a
        // real acquired reader body; ordinary product scheduling is untouched.
        self.reader_entered_rx
            .recv_timeout(CONTROL_BUDGET)
            .context("controlled reader body entry")?;
        if self.fail_dispatcher {
            owner.fail_next_acquisition();
        }
        Ok(())
    }
}

fn run_daemon_entrypoint_fixture(fail_dispatcher: bool) -> Value {
    let listener = TcpListener::bind("127.0.0.1:0").expect("owned full-daemon Firefox endpoint");
    let port = listener.local_addr().expect("Firefox endpoint").port();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (proceed_tx, proceed_rx) = mpsc::channel();
    let (body_tx, body_rx) = mpsc::channel();
    let (joined_tx, joined_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let cleanup_state: Arc<Mutex<Option<Arc<SharedState>>>> = Arc::new(Mutex::new(None));
    let cleanup_socket: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));
    let control = RunControl {
        ready: ready_tx,
        reader_return: Some(ReaderSendGate {
            entered: entered_tx,
            proceed: proceed_rx,
        }),
        reader_entered_tx: Some(body_tx),
        reader_entered_rx: body_rx,
        joins: joined_tx,
        cleanup_state: Arc::clone(&cleanup_state),
        fail_dispatcher,
    };
    let release = proceed_tx.clone();
    let interrupt = Arc::clone(&cleanup_socket);
    let daemon = OwnedThread {
        handle: Some(thread::spawn(move || {
            RUN_CONTROL.with(|slot| {
                assert!(slot.borrow().is_none());
                *slot.borrow_mut() = Some(control);
            });
            let result = run_daemon("127.0.0.1", port, 60);
            let _ = result_tx.send(result.map_err(|error| format!("{error:#}")));
        })),
        cleanup: Some(Box::new(move || {
            // Release the fixture's epilogue gate before any cleanup joins.
            let _ = release.send(());
            if let Some(socket) = &*interrupt.lock().expect("cleanup socket") {
                let _ = socket.shutdown(std::net::Shutdown::Both);
            }
            if let Some(state) = &*cleanup_state.lock().expect("cleanup state") {
                state.stop(StopReason::OwnerDropped);
            }
        })),
    };
    let socket = accept_controlled(listener);
    socket
        .set_read_timeout(Some(CONTROL_BUDGET))
        .expect("Firefox read deadline");
    *cleanup_socket.lock().expect("owned socket cleanup") =
        Some(socket.try_clone().expect("cleanup clone"));
    let mut reader = FramedReader::from_stream(socket.try_clone().expect("Firefox peer reader"));
    let mut writer = FramedWriter::from_stream(socket.try_clone().expect("Firefox peer writer"));
    writer
        .send(&json!({"from":"root","applicationType":"browser"}))
        .expect("Firefox greeting");
    for (method, reply) in [
        (
            "listTabs",
            json!({"from":"root","tabs":[{"actor":"tab","browsingContextID":11}]}),
        ),
        ("getWatcher", json!({"from":"tab","actor":"watcher"})),
        ("watchTargets", json!({"from":"watcher"})),
        ("watchResources", json!({"from":"watcher"})),
    ] {
        let request = reader.recv().expect("actual initial protocol request");
        assert_eq!(request["type"], method);
        writer.send(&reply).expect("initial protocol reply");
    }
    let ready = ready_rx
        .recv_timeout(CONTROL_BUDGET)
        .expect("actual published daemon state");
    if !fail_dispatcher {
        let socket =
            TcpStream::connect(("127.0.0.1", ready.proxy_port)).expect("actual daemon client");
        socket
            .set_read_timeout(Some(CONTROL_BUDGET))
            .expect("client deadline");
        let mut reader = FramedReader::from_stream(socket.try_clone().expect("client reader"));
        let mut writer = FramedWriter::from_stream(socket);
        writer
            .send(&json!({"auth":ready.token}))
            .expect("daemon auth");
        assert_eq!(
            reader.recv().expect("daemon greeting")["applicationType"],
            "browser"
        );
        writer
            .send(&json!({"to":"daemon","type":"shutdown"}))
            .expect("authenticated shutdown");
        // Observe the actual bounded response before the connection is retired.
        let response = reader
            .recv()
            .expect("shutdown response before cancellation");
        assert_eq!(response["from"], "daemon");
    }
    let reached = entered_rx.recv_timeout(CONTROL_BUDGET).is_ok();
    let returned_while_gated = result_rx.try_recv().is_ok();
    let registry_present_while_gated = registry::read_registry(port)
        .expect("registry observation")
        .is_some();
    let _ = proceed_tx.send(());
    // Do not run emergency cleanup until the normal/error path completed.
    let result = result_rx
        .recv_timeout(CONTROL_BUDGET)
        .expect("actual daemon entrypoint result");
    daemon.finish().expect("actual daemon entry thread join");
    let records: Vec<_> = joined_rx.try_iter().collect();
    let registry_absent_after_return = registry::read_registry(port)
        .expect("registry after return")
        .is_none();
    assert!(
        reached,
        "real acquired reader must reach the gated epilogue"
    );
    assert!(
        !returned_while_gated,
        "run_daemon returned before its reader worker"
    );
    assert!(
        registry_present_while_gated,
        "registry must outlive acquired worker cleanup"
    );
    assert!(registry_absent_after_return);
    let mut names: Vec<_> = records.iter().map(|record| record.name).collect();
    names.sort_unstable();
    let expected = if fail_dispatcher {
        vec!["firefox-reader", "grip-release-drainer"]
    } else {
        vec![
            "cli-client",
            "event-dispatcher",
            "firefox-reader",
            "grip-release-drainer",
        ]
    };
    assert_eq!(
        names, expected,
        "every actually acquired handle must have one actual join"
    );
    assert!(records.iter().all(|record| record.supervision_returned
        && record.outcome == super::super::lifecycle::WorkerOutcome::Stopped));
    assert_eq!(
        result.is_err(),
        fail_dispatcher,
        "entrypoint return contract: {result:?}"
    );
    if let Err(error) = &result {
        assert!(
            error.contains("spawning event dispatcher thread"),
            "{error}"
        );
    }
    json!({
        "mode": if fail_dispatcher { "partial-acquisition" } else { "authenticated-shutdown" },
        "reader_gate_observed": reached,
        "returned_while_gated": returned_while_gated,
        "registry_present_while_gated": registry_present_while_gated,
        "registry_absent_after_return": registry_absent_after_return,
        "entry_thread_joined": true,
        "result": result,
        "actual_worker_joins": records.iter().map(|record| json!({"name":record.name,"outcome":format!("{:?}",record.outcome),"supervision_returned":record.supervision_returned})).collect::<Vec<_>>()
    })
}

fn run_daemon_isolated_control(mode: &str, test_name: &str, fail_dispatcher: bool) {
    if std::env::var("FF_RDP_284_TEST_CHILD").ok().as_deref() == Some(mode) {
        let receipt = run_daemon_entrypoint_fixture(fail_dispatcher);
        let home = std::env::var_os("FF_RDP_HOME").expect("isolated child home");
        std::fs::write(
            std::path::PathBuf::from(home).join("join-proof.json"),
            serde_json::to_vec_pretty(&receipt).expect("proof encoding"),
        )
        .expect("retain child proof");
        return;
    }
    let private_home = tempfile::Builder::new()
        .prefix("ff-rdp-284-run-control-")
        .tempdir()
        .expect("isolated child home");
    let output = std::process::Command::new(std::env::current_exe().expect("current unit binary"))
        .args([test_name, "--exact", "--nocapture", "--test-threads=1"])
        .env("FF_RDP_284_TEST_CHILD", mode)
        .env("FF_RDP_HOME", private_home.path())
        .output()
        .expect("actual isolated test child wait");
    let proof = std::fs::read(private_home.path().join("join-proof.json"));
    // Keep every child failure/output even when the outer assertion fails.
    if let Some(home) = std::env::var_os("FF_RDP_HOME") {
        let home = std::path::PathBuf::from(home);
        let _ = std::fs::create_dir_all(&home);
        std::fs::write(home.join(format!("{mode}-child.stdout")), &output.stdout)
            .expect("child stdout receipt");
        std::fs::write(home.join(format!("{mode}-child.stderr")), &output.stderr)
            .expect("child stderr receipt");
        if let Ok(bytes) = &proof {
            std::fs::write(home.join(format!("{mode}-join-proof.json")), bytes)
                .expect("child join receipt");
        }
    }
    assert!(
        output.status.success(),
        "isolated child failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let proof: Value =
        serde_json::from_slice(&proof.expect("actual worker join proof")).expect("join proof JSON");
    assert_eq!(proof["entry_thread_joined"], true);
    assert_eq!(proof["returned_while_gated"], false);
    println!("iter284 isolated {mode}: {proof}");
}

#[test]
fn iter284_run_daemon_shutdown_joins_before_return() {
    run_daemon_isolated_control(
        "shutdown",
        "daemon::server::lifecycle_controls::iter284_run_daemon_shutdown_joins_before_return",
        false,
    );
}

#[test]
fn iter284_run_daemon_partial_acquisition_joins_before_error() {
    run_daemon_isolated_control(
        "partial",
        "daemon::server::lifecycle_controls::iter284_run_daemon_partial_acquisition_joins_before_error",
        true,
    );
}

fn dispatched_target(
    runtime: &Runtime,
    observed: &mpsc::Receiver<Value>,
    source: EventSource,
    actor: &str,
    top: bool,
    url: &str,
) {
    let mut packet = super::tests::startup_form(actor, url);
    packet["target"]["isTopLevelTarget"] = json!(top);
    runtime
        .state
        .event_tx
        .send(DaemonEvent::Packet(source, packet))
        .expect("target queued to actual dispatcher");
    assert_eq!(
        observed
            .recv_timeout(CONTROL_BUDGET)
            .expect("actual target dispatch completed")["target"]["actor"],
        actor
    );
}

fn current_target_actors(state: &SharedState) -> Vec<String> {
    frame_targets_snapshot(state)
        .iter()
        .filter_map(|packet| packet_target_actor(packet).map(str::to_owned))
        .collect()
}

#[test]
fn iter284_primary_snapshot_survives_optional_publication_and_loss() {
    let mut runtime = Runtime::new(super::tests::test_state_with_queues(
        4,
        ff_rdp_core::release_queue(1).0,
    ));
    let generation = runtime.state.begin_lazy_generation();
    let observed = runtime.dispatcher(VecDeque::new());
    dispatched_target(
        &runtime,
        &observed,
        EventSource::Primary,
        "primary-top",
        true,
        "https://same.example/",
    );
    dispatched_target(
        &runtime,
        &observed,
        EventSource::Primary,
        "primary-child",
        false,
        "https://same.example/frame",
    );
    runtime
        .state
        .buffer
        .lock()
        .expect("seed retained history")
        .insert_raw("console-message", json!({"message":"primary-history"}));
    let nav_before = runtime.state.nav_generation.load(Ordering::Relaxed);
    dispatched_target(
        &runtime,
        &observed,
        EventSource::Lazy(generation),
        "optional-top",
        true,
        "https://same.example/",
    );
    dispatched_target(
        &runtime,
        &observed,
        EventSource::Lazy(generation),
        "optional-child",
        false,
        "https://same.example/frame",
    );
    let during_optional = current_target_actors(&runtime.state);
    runtime.state.invalidate_lazy(generation);
    let after_loss = current_target_actors(&runtime.state);
    let top_after = runtime
        .state
        .top_level_target
        .lock()
        .expect("selected top")
        .clone();
    let ownership = runtime
        .state
        .lazy
        .lock()
        .expect("source ownership")
        .target_sources
        .clone();
    let history = runtime
        .state
        .buffer
        .lock()
        .expect("retained history")
        .sizes()
        .get("console-message")
        .copied();
    let nav_after = runtime.state.nav_generation.load(Ordering::Relaxed);
    let joined = runtime.stop();
    assert_eq!(joined.len(), 1);
    assert_eq!(
        during_optional,
        ["primary-top", "primary-child"],
        "a different connection's actors must not replace primary state"
    );
    assert_eq!(
        after_loss,
        ["primary-top", "primary-child"],
        "optional loss must retain the original primary snapshot"
    );
    assert_eq!(top_after.as_ref().map(AsRef::as_ref), Some("primary-top"));
    assert_eq!(ownership.len(), 2);
    assert!(
        ownership
            .values()
            .all(|source| *source == EventSource::Primary)
    );
    assert_eq!(
        history,
        Some(1),
        "source identity alone is not navigation or a reason to purge history"
    );
    assert_eq!(nav_before, nav_after);
}

#[test]
fn iter284_primary_replacement_prunes_implicit_source_ownership() {
    let mut runtime = Runtime::new(super::tests::test_state_with_queues(
        4,
        ff_rdp_core::release_queue(1).0,
    ));
    let observed = runtime.dispatcher(VecDeque::new());
    dispatched_target(
        &runtime,
        &observed,
        EventSource::Primary,
        "old-top",
        true,
        "https://old.example/",
    );
    dispatched_target(
        &runtime,
        &observed,
        EventSource::Primary,
        "old-child",
        false,
        "https://old.example/frame",
    );
    dispatched_target(
        &runtime,
        &observed,
        EventSource::Primary,
        "old-child",
        false,
        "https://old.example/frame#updated",
    );
    let before_replacement = current_target_actors(&runtime.state);
    {
        let mut buffer = runtime.state.buffer.lock().expect("navigation resources");
        buffer.insert_raw("console-message", json!({"message":"old-document"}));
        buffer.insert_raw("network-event", json!({"url":"https://new.example/"}));
    }
    // Firefox need not announce individual destruction of outgoing children.
    dispatched_target(
        &runtime,
        &observed,
        EventSource::Primary,
        "new-top",
        true,
        "https://new.example/",
    );
    let snapshot = current_target_actors(&runtime.state);
    let ownership = runtime
        .state
        .lazy
        .lock()
        .expect("source ownership")
        .target_sources
        .clone();
    let sizes = runtime
        .state
        .buffer
        .lock()
        .expect("navigation resource result")
        .sizes();
    let joined = runtime.stop();
    assert_eq!(joined.len(), 1);
    assert_eq!(
        before_replacement,
        ["old-top", "old-child"],
        "same actor replaces in place"
    );
    assert_eq!(snapshot, ["new-top"]);
    assert_eq!(
        ownership.len(),
        1,
        "implicitly retired actors must not accumulate in ownership metadata"
    );
    assert_eq!(ownership.get("new-top"), Some(&EventSource::Primary));
    assert!(!ownership.contains_key("old-top"));
    assert!(!ownership.contains_key("old-child"));
    assert!(
        !sizes.contains_key("console-message"),
        "genuine same-source navigation still purges stale console resources"
    );
    assert_eq!(
        sizes.get("network-event"),
        Some(&1),
        "in-flight network resources survive navigation"
    );
}

#[test]
fn iter284_optional_switch_retires_actors_and_fronts_before_loss() {
    let mut runtime = Runtime::new(super::tests::test_state_with_queues(
        4,
        ff_rdp_core::release_queue(1).0,
    ));
    let generation = runtime.state.begin_lazy_generation();
    let observed = runtime.dispatcher(VecDeque::new());
    dispatched_target(
        &runtime,
        &observed,
        EventSource::Primary,
        "primary-top",
        true,
        "https://primary.example/",
    );
    dispatched_target(
        &runtime,
        &observed,
        EventSource::Primary,
        "primary-child",
        false,
        "https://primary.example/frame",
    );
    let registry = Arc::clone(&runtime.state.actor_registry);
    registry.register(
        "primary-front".into(),
        FrontKind::Console,
        Some("primary-top".into()),
    );
    dispatched_target(
        &runtime,
        &observed,
        EventSource::Lazy(generation),
        "optional-old",
        true,
        "https://old.example/",
    );
    dispatched_target(
        &runtime,
        &observed,
        EventSource::Lazy(generation),
        "optional-child",
        false,
        "https://old.example/frame",
    );
    registry.register(
        "optional-old-front".into(),
        FrontKind::Console,
        Some("optional-old".into()),
    );
    registry.register(
        "optional-child-front".into(),
        FrontKind::Console,
        Some("optional-child".into()),
    );
    let retired = [
        "optional-old",
        "optional-child",
        "optional-old-front",
        "optional-child-front",
    ];
    // Ordinary same-actor reannouncement must retain both targets and fronts.
    dispatched_target(
        &runtime,
        &observed,
        EventSource::Lazy(generation),
        "optional-old",
        true,
        "https://old.example/#same",
    );
    let alive_after_reannouncement = retired
        .iter()
        .all(|actor| registry.assert_alive(&(*actor).into()).is_ok());
    let child_still_owned = runtime
        .state
        .lazy
        .lock()
        .expect("same-source ownership")
        .target_sources
        .contains_key("optional-child");
    // No destroyed forms are delivered for the outgoing optional document.
    dispatched_target(
        &runtime,
        &observed,
        EventSource::Lazy(generation),
        "optional-new",
        true,
        "https://new.example/",
    );
    registry.register(
        "optional-new-front".into(),
        FrontKind::Console,
        Some("optional-new".into()),
    );
    let dead_at_replacement = retired
        .iter()
        .all(|actor| registry.assert_alive(&(*actor).into()).is_err());
    let current_alive_before_loss = registry.assert_alive(&"optional-new".into()).is_ok()
        && registry.assert_alive(&"optional-new-front".into()).is_ok();
    let retired_ownership_absent = {
        let lazy = runtime.state.lazy.lock().expect("replacement ownership");
        !lazy.target_sources.contains_key("optional-old")
            && !lazy.target_sources.contains_key("optional-child")
    };
    runtime.state.invalidate_lazy(generation);
    let old_dead_after_loss = retired
        .iter()
        .all(|actor| registry.assert_alive(&(*actor).into()).is_err());
    let current_dead_after_loss = registry.assert_alive(&"optional-new".into()).is_err()
        && registry.assert_alive(&"optional-new-front".into()).is_err();
    let primary_alive = ["primary-top", "primary-child", "primary-front"]
        .iter()
        .all(|actor| registry.assert_alive(&(*actor).into()).is_ok());
    let snapshot_after_loss = current_target_actors(&runtime.state);
    let remaining_ownership = runtime
        .state
        .lazy
        .lock()
        .expect("ownership after optional loss")
        .target_sources
        .clone();
    let joined = runtime.stop();
    assert_eq!(joined.len(), 1);
    assert!(
        alive_after_reannouncement && child_still_owned,
        "same-actor reannouncement must not retire targets or dependents"
    );
    assert!(
        dead_at_replacement,
        "implicit optional retirement must invalidate old actors and fronts before discarding ownership"
    );
    assert!(
        current_alive_before_loss,
        "the replacement actor and its front remain alive until loss"
    );
    assert!(
        retired_ownership_absent,
        "do not retain an unbounded historical ownership ledger"
    );
    assert!(old_dead_after_loss);
    assert!(
        current_dead_after_loss,
        "normal optional loss retires its current actor and dependent front"
    );
    assert!(
        primary_alive,
        "optional retirement must not invalidate unrelated primary actors/fronts"
    );
    assert_eq!(snapshot_after_loss, ["primary-top", "primary-child"]);
    assert_eq!(remaining_ownership.len(), 2);
    assert!(
        remaining_ownership
            .values()
            .all(|source| *source == EventSource::Primary)
    );
}
