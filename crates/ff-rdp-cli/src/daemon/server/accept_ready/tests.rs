use super::*;
use crate::daemon::lifecycle::StopReason;
use std::sync::mpsc;
use std::thread;

const CONTROL_BUDGET: Duration = Duration::from_secs(3);

fn listener() -> TcpListener {
    let listener = TcpListener::bind("127.0.0.1:0").expect("owned listener");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    listener
}

#[test]
fn queued_connections_rearm_registered_listener() {
    let listener = listener();
    let address = listener.local_addr().expect("listener address");
    let cancellation = Arc::new(Cancellation::default());
    let mut ready = ReadyListener::new(&listener, cancellation).expect("registered listener");
    for _ in 0..2 {
        assert_eq!(
            ready.accept().expect_err("empty queue").kind(),
            io::ErrorKind::WouldBlock
        );
        let client = TcpStream::connect_timeout(&address, CONTROL_BUDGET).expect("queued client");
        // Assert the actual readiness disposition, not a flaky sub-100ms time.
        assert_eq!(ready.wait().expect("poll"), Wake::Readable);
        let (accepted, peer) = ready
            .accept()
            .expect("accept through registered Mio listener");
        assert_eq!(peer, client.local_addr().expect("client address"));
        assert_eq!(accepted.peer_addr().expect("accepted peer"), peer);
        drop(accepted);
        drop(client);
    }
    assert_eq!(
        ready.accept().expect_err("fully drained").kind(),
        io::ErrorKind::WouldBlock
    );
}

// The barrier is AFTER the production cancellation precheck and BEFORE poll.
// A queued listener event or cancellation must survive that registration race.
// The worker and barrier are bounded; always release and actually join before
// asserting any parent-side observation (including failed setup).
fn controlled_poll(cancel: bool) {
    let listener = listener();
    let address = listener.local_addr().expect("listener address");
    let cancellation = Arc::new(Cancellation::default());
    let mut ready =
        ReadyListener::new(&listener, Arc::clone(&cancellation)).expect("registered listener");
    assert_eq!(
        ready.accept().expect_err("initial empty queue").kind(),
        io::ErrorKind::WouldBlock
    );
    let (entered_tx, entered_rx) = mpsc::channel();
    let (proceed_tx, proceed_rx) = mpsc::channel();
    let (polled_tx, polled_rx) = mpsc::channel();
    ready.probe = Some(PollProbe {
        entered: entered_tx,
        proceed: proceed_rx,
        polled: polled_tx,
    });
    let worker = thread::spawn(move || {
        let outcome = ready.wait()?;
        let accepted = if outcome == Wake::Readable {
            Some(ready.accept()?.1)
        } else {
            None
        };
        Ok::<_, io::Error>((outcome, accepted))
    });
    let entered = entered_rx.recv_timeout(CONTROL_BUDGET);
    let client = if !cancel && entered.is_ok() {
        Some(TcpStream::connect_timeout(&address, CONTROL_BUDGET))
    } else {
        None
    };
    if cancel || entered.is_err() || client.as_ref().is_some_and(Result::is_err) {
        cancellation.request_stop(StopReason::AuthenticatedShutdown);
    }
    let released = proceed_tx.send(());
    let joined = worker.join();
    // No future worker remains; cancellation also releases any retained wakes.
    cancellation.request_stop(StopReason::OwnerDropped);
    let polled = polled_rx.try_recv();
    assert!(
        entered.is_ok(),
        "worker reached actual pre-poll barrier: {entered:?}"
    );
    assert!(released.is_ok(), "released owned barrier: {released:?}");
    let (outcome, accepted) = joined
        .expect("actual worker join")
        .expect("actual worker body");
    let tokens = polled.expect("actual Poll returned event tokens");
    if cancel {
        assert_eq!(outcome, Wake::Cancelled);
        assert!(
            tokens.contains(&CANCEL.0),
            "actual cancellation readiness: {tokens:?}"
        );
        assert_eq!(accepted, None);
    } else {
        let client = client
            .expect("connection attempted")
            .expect("owned connection");
        assert_eq!(outcome, Wake::Readable);
        assert!(
            tokens.contains(&LISTENER.0),
            "actual listener readiness: {tokens:?}"
        );
        assert_eq!(accepted, Some(client.local_addr().expect("client address")));
    }
}

#[test]
fn connection_after_would_block_has_readiness_event() {
    controlled_poll(false);
}

#[test]
fn cancellation_between_check_and_poll_has_wake_event() {
    controlled_poll(true);
}

#[test]
fn cancellation_before_registration_skips_poll() {
    let listener = listener();
    let cancellation = Arc::new(Cancellation::default());
    cancellation.request_stop(StopReason::Signal);
    let mut ready =
        ReadyListener::new(&listener, cancellation).expect("already cancelled registration");
    let (entered, entered_rx) = mpsc::channel();
    let (_proceed, proceed_rx) = mpsc::channel();
    let (polled, _polled_rx) = mpsc::channel();
    ready.probe = Some(PollProbe {
        entered,
        proceed: proceed_rx,
        polled,
    });
    assert_eq!(
        ready.wait().expect("cancelled without polling"),
        Wake::Cancelled
    );
    assert!(ready.probe.is_some(), "poll probe was never consumed");
    assert!(matches!(
        entered_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn empty_listener_returns_housekeeping_without_connection() {
    let listener = listener();
    let mut ready = ReadyListener::new(&listener, Arc::new(Cancellation::default()))
        .expect("registered listener");
    assert_eq!(
        ready.wait().expect("bounded housekeeping poll"),
        Wake::Housekeeping
    );
    assert_eq!(
        ready
            .accept()
            .expect_err("timeout did not invent a client")
            .kind(),
        io::ErrorKind::WouldBlock
    );
}
