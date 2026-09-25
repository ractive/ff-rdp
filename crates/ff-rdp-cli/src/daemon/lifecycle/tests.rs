use super::*;
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

const CONTROL_BUDGET: Duration = Duration::from_secs(5);

/// Release owned blockers and retain the actual join even when an assertion fails.
struct OwnedThread<T> {
    handle: Option<JoinHandle<T>>,
    cleanup: Option<Box<dyn FnOnce()>>,
}

impl<T> OwnedThread<T> {
    fn finish(mut self) -> thread::Result<T> {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
        self.handle.take().expect("retained thread handle").join()
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

fn owned_thread<T: Send + 'static>(
    cleanup: impl FnOnce() + 'static,
    body: impl FnOnce() -> T + Send + 'static,
) -> OwnedThread<T> {
    OwnedThread {
        handle: Some(thread::spawn(body)),
        cleanup: Some(Box::new(cleanup)),
    }
}

#[test]
fn iter284_primitive_stop_reason_admission_and_wake_order() {
    let cancellation = Arc::new(Cancellation::default());
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut registrations = Vec::new();
    // Deliberately register in the opposite order from the required wake order.
    for phase in [WakePhase::Rpc, WakePhase::Queue, WakePhase::SocketAndWriter] {
        let seen = Arc::clone(&order);
        registrations.push(cancellation.register(phase, move || {
            seen.lock().expect("wake order").push(phase);
        }));
    }
    let mut admitted = 0;
    cancellation
        .while_running(|| admitted += 1)
        .expect("initial admission");
    assert!(cancellation.request_stop(StopReason::Signal));
    assert!(!cancellation.request_stop(StopReason::Idle));
    assert_eq!(cancellation.reason(), Some(StopReason::Signal));
    assert_eq!(
        cancellation.while_running(|| admitted += 1),
        Err(StopReason::Signal)
    );
    assert_eq!(admitted, 1);
    assert_eq!(
        *order.lock().expect("wake order"),
        [WakePhase::SocketAndWriter, WakePhase::Queue, WakePhase::Rpc]
    );
    let late = Arc::clone(&order);
    registrations.push(cancellation.register(WakePhase::Queue, move || {
        late.lock().expect("late wake").push(WakePhase::Queue);
    }));
    assert_eq!(order.lock().expect("wake order").len(), 4);
    drop(registrations);
}

#[test]
fn iter284_primitive_registered_socket_interrupts_actual_read() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("owned listener");
    let client =
        TcpStream::connect(listener.local_addr().expect("listener address")).expect("owned client");
    let (server, _) = listener.accept().expect("owned server");
    let cancellation = Arc::new(Cancellation::default());
    let _registration = cancellation
        .register_socket(&server)
        .expect("shutdown clone");
    let cleanup = server.try_clone().expect("cleanup clone");
    let worker = owned_thread(
        move || {
            let _ = cleanup.shutdown(Shutdown::Both);
        },
        move || {
            use std::io::Read;
            let mut socket = server;
            socket.read(&mut [0; 1])
        },
    );
    cancellation.request_stop(StopReason::AuthenticatedShutdown);
    let result = worker.finish().expect("actual read worker join");
    assert!(matches!(result, Ok(0) | Err(_)));
    drop(client);
}

fn wait_for_queue_waiter<T>(queue: &BoundedQueue<T>, sender: bool) -> bool {
    let deadline = Instant::now() + CONTROL_BUDGET;
    let mut guard = queue.state.lock().expect("queue state");
    loop {
        if if sender {
            guard.send_waiters > 0
        } else {
            guard.recv_waiters > 0
        } {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        guard = queue
            .blocked
            .wait_timeout(guard, remaining)
            .expect("wait for actual queue wait")
            .0;
    }
}

#[test]
fn iter284_primitive_full_and_empty_queue_cancel_actual_waiters() {
    let full = Arc::new(BoundedQueue::new(1));
    full.send(1).expect("fill queue");
    let body_queue = Arc::clone(&full);
    let cleanup_queue = Arc::clone(&full);
    let sender = owned_thread(move || cleanup_queue.close(), move || body_queue.send(2));
    let observed_sender = wait_for_queue_waiter(&full, true);
    full.close();
    let send_result = sender.finish().expect("actual sender join");
    assert!(observed_sender, "producer must reach full-queue wait");
    assert_eq!(send_result, Err(2));

    let empty = Arc::new(BoundedQueue::<u8>::new(1));
    let body_queue = Arc::clone(&empty);
    let cleanup_queue = Arc::clone(&empty);
    let receiver = owned_thread(move || cleanup_queue.close(), move || body_queue.recv());
    let observed_receiver = wait_for_queue_waiter(&empty, false);
    empty.close();
    let recv_result = receiver.finish().expect("actual receiver join");
    assert!(observed_receiver, "consumer must reach empty-queue wait");
    assert_eq!(recv_result, None);
}

#[test]
fn iter284_primitive_queue_preserves_normal_fifo() {
    let queue = Arc::new(BoundedQueue::new(2));
    let body_queue = Arc::clone(&queue);
    let cleanup_queue = Arc::clone(&queue);
    let producer = owned_thread(
        move || cleanup_queue.close(),
        move || {
            for value in 0..32 {
                body_queue.send(value).expect("normal enqueue");
            }
        },
    );
    let received = (0..32)
        .map(|_| queue.recv().expect("normal receive"))
        .collect::<Vec<_>>();
    producer.finish().expect("actual producer join");
    assert_eq!(received, (0..32).collect::<Vec<_>>());
}

#[test]
fn iter284_primitive_occupied_writer_cancellation_and_expired_budget() {
    let slot = Arc::new(WriterSlot::new(7));
    let held = slot
        .acquire(Instant::now() + CONTROL_BUDGET)
        .expect("initial lease");
    assert!(matches!(
        slot.acquire(Instant::now()),
        Err(LeaseError::Deadline)
    ));
    let body_slot = Arc::clone(&slot);
    let cleanup_slot = Arc::clone(&slot);
    let waiter = owned_thread(
        move || cleanup_slot.close(),
        move || {
            body_slot
                .acquire(Instant::now() + CONTROL_BUDGET)
                .map(|lease| *lease)
        },
    );
    let deadline = Instant::now() + CONTROL_BUDGET;
    let mut guard = slot.state.lock().expect("slot state");
    while guard.waiters == 0 && Instant::now() < deadline {
        guard = slot
            .blocked
            .wait_timeout(guard, deadline.saturating_duration_since(Instant::now()))
            .expect("wait for occupied writer")
            .0;
    }
    let observed = guard.waiters > 0;
    drop(guard);
    slot.close();
    let result = waiter.finish().expect("actual writer waiter join");
    // The holder still owns its lease: cancellation did not depend on its return.
    assert_eq!(*held, 7);
    assert!(observed, "contender must reach writer ownership wait");
    assert_eq!(result, Err(LeaseError::Cancelled));
    drop(held);
}

#[test]
fn iter284_primitive_owner_joins_cancelled_body_and_preserves_panic() {
    let cancellation = Arc::new(Cancellation::default());
    let (release_tx, release_rx) = mpsc::channel();
    let (entered_tx, entered_rx) = mpsc::channel();
    let _wake = cancellation.register(WakePhase::Queue, move || {
        let _ = release_tx.send(());
    });
    let mut owner = WorkerOwner::new(Arc::clone(&cancellation));
    owner
        .spawn("gated-worker", WorkerPolicy::Required, move || {
            let _ = entered_tx.send(());
            let _ = release_rx.recv();
        })
        .expect("owned worker spawn");
    let entered = entered_rx.recv_timeout(CONTROL_BUDGET).is_ok();
    let joined = owner.shutdown_and_join(StopReason::StartupFailure);
    assert!(entered);
    assert_eq!(
        joined,
        [JoinRecord {
            name: "gated-worker",
            outcome: WorkerOutcome::Stopped,
            supervision_returned: true
        }]
    );
    owner.reap_finished();

    let cancellation = Arc::new(Cancellation::default());
    let mut owner = WorkerOwner::new(Arc::clone(&cancellation));
    owner
        .spawn("panicking-worker", WorkerPolicy::Required, || {
            panic!("controlled worker panic")
        })
        .expect("owned panic worker spawn");
    owner.join_one(0);
    assert_eq!(owner.joined[0].outcome, WorkerOutcome::Panicked);
    assert!(owner.joined[0].supervision_returned);
    assert_eq!(
        cancellation.reason(),
        Some(StopReason::WorkerPanicked("panicking-worker"))
    );
}

#[test]
fn iter284_primitive_optional_and_client_return_do_not_stop_service() {
    for (policy, expected) in [
        (WorkerPolicy::Optional, WorkerOutcome::OptionalReturned),
        (WorkerPolicy::Client, WorkerOutcome::ClientReturned),
    ] {
        let cancellation = Arc::new(Cancellation::default());
        let mut owner = WorkerOwner::new(Arc::clone(&cancellation));
        owner
            .spawn("normal-return", policy, || {})
            .expect("owned normal worker spawn");
        owner.join_one(0);
        assert_eq!(owner.joined[0].outcome, expected);
        assert_eq!(cancellation.reason(), None);
        owner.shutdown_and_join(StopReason::RecoveryFailure);
    }
}

#[test]
fn iter284_connector_actual_poll_observes_preposted_cancel_and_joins() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("owned real connector endpoint");
    let peer = listener.local_addr().expect("connector peer");
    let cancellation = Arc::new(Cancellation::default());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (proceed_tx, proceed_rx) = mpsc::channel();
    let (polled_tx, polled_rx) = mpsc::channel();
    *cancellation.connect_probe.lock().expect("connector probe") = Some(ConnectPollProbe {
        entered: entered_tx,
        proceed: proceed_rx,
        polled: polled_tx,
    });
    let worker_cancellation = Arc::clone(&cancellation);
    let cleanup_cancellation = Arc::clone(&cancellation);
    let cleanup_proceed = proceed_tx.clone();
    let worker = owned_thread(
        move || {
            cleanup_cancellation.request_stop(StopReason::OwnerDropped);
            let _ = cleanup_proceed.send(());
        },
        move || {
            connect_cancellable(
                peer,
                &worker_cancellation,
                Instant::now() + Duration::from_secs(10),
            )
        },
    );
    let entered = entered_rx.recv_timeout(CONTROL_BUDGET).is_ok();
    cancellation.request_stop(StopReason::AuthenticatedShutdown);
    let _ = proceed_tx.send(());
    let result = worker.finish().expect("actual connector thread join");
    let tokens = polled_rx
        .recv_timeout(CONTROL_BUDGET)
        .expect("actual Poll observation");
    assert!(entered, "actual connector must reach its poll boundary");
    assert!(
        tokens.contains(&1),
        "actual registered Waker token must be returned: {tokens:?}"
    );
    assert!(matches!(result, Err(error) if error.kind() == io::ErrorKind::Interrupted));
    // Socket readiness may also have arrived: stop still wins before adoption.
    // This does not assert kernel-pending TCP or an already-blocked poll PC.
    drop(listener);
}
