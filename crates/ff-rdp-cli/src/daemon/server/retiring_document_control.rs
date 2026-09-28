//! Caller prevention through the production origin ledger and supervised workers.
use super::*;
use crate::commands::connect_tab::resolve_target_snapshot;
use crate::daemon::client::TargetEndpoint;

fn snapshot_endpoint(
    harness: &mut Harness,
    writer: &FirefoxWriter,
    name: &'static str,
) -> Result<TargetEndpoint> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let endpoint = TargetEndpoint::new(listener.local_addr()?.port(), "test-token");
    let state = Arc::clone(&harness.state);
    let writer = Arc::clone(writer);
    let deadline = harness.deadline.min(Instant::now() + IO_BUDGET);
    harness.spawn(name, WorkerPolicy::Client, move || {
        let stream = accept_ready(listener, deadline)?;
        handle_client(&state, stream, &writer)
    })?;
    Ok(endpoint)
}

fn schedule(harness: &mut Harness) -> Result<()> {
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

    *lock_or_recover!(harness.state.primary_target) = Some(PrimaryTarget {
        startup_recovery: StartupRecovery::Finished,
        descriptor: "tab".into(),
        watcher: "watcher".into(),
        initial_browsing_context: Some(11),
        current: Some(json!({"actor":"old","consoleActor":"old/console",
            "innerWindowId":7,"browsingContextID":11,"url":"https://old/"})),
    });
    let mut a = authenticated_client(harness, "handler-a", &firefox_writer)?;
    // Own the actual RPC slot before the actual dispatcher delivers the start.
    a.send_ordinary(&json!({"to":"root","type":"listTabs"}))?;
    read_request(harness, &mut peer_reader, "A-initial")?;
    send_reply(harness, &mut peer_writer, A_MARKER)?;
    let first = ff_rdp_core::transport::recv_reply_from(&mut a, "root")?;
    ensure!(first["tabs"][0]["title"] == A_MARKER, "initial A reply");
    peer_writer.send(&json!({"from":"watcher","type":"resources-available-array",
        "array":[["document-event",[{"name":"will-navigate","isFrameSwitching":false,
            "innerWindowId":7,"browsingContextID":11,"newURI":"https://new/"}]]]}))?;
    a.with_read_deadline(harness.deadline, |transport| {
        ff_rdp_core::transport::recv_event_from(transport, "old", |event| {
            event["type"] == "willNavigate"
        })
    })?;
    ensure!(
        a.take_navigation_started().as_deref() == Some("https://new/"),
        "actual translated start absent"
    );

    let endpoint = snapshot_endpoint(harness, &firefox_writer, "snapshot-old")?;
    let old = resolve_target_snapshot(
        &mut a,
        Some(&endpoint),
        &"tab".into(),
        harness.deadline.min(Instant::now() + IO_BUDGET),
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?;
    harness.join("snapshot-old")?;
    ensure!(
        old.is_none(),
        "outgoing live form must not start metadata work"
    );
    ensure!(
        harness.state.ownership_health()["pending_ordinary"] == 0,
        "old form registered ordinary debt"
    );

    // Real reader/dispatcher publication precedes the received target event.
    peer_writer.send(
        &json!({"from":"watcher","type":"target-available-form","target":{
        "actor":"new","consoleActor":"new/console","innerWindowId":8,"browsingContextID":12,
        "targetType":"frame","isTopLevelTarget":true,"isPopup":false,"url":"https://new/"}}),
    )?;
    a.with_read_deadline(harness.deadline, |transport| {
        ff_rdp_core::transport::recv_event_from(transport, "watcher", |event| {
            event["type"] == "target-available-form"
        })
    })?;
    let mut metadata_reader = FramedReader::from_stream(peer_reader.try_clone_stream()?);
    let mut metadata_writer = FramedWriter::from_stream(peer_reader.try_clone_stream()?);
    harness.spawn("metadata-peer", WorkerPolicy::Client, move || {
        let first = metadata_reader.recv()?;
        ensure!(
            first == json!({"to":"new","type":"listFrames"}),
            "FIRST post-start Firefox request must be replacement metadata, got {first}"
        );
        metadata_writer
            .send(&json!({"from":"new","frames":[{"isTopLevel":true,"url":"https://new/"}]}))?;
        Ok(())
    })?;
    let endpoint = snapshot_endpoint(harness, &firefox_writer, "snapshot-new")?;
    let target = resolve_target_snapshot(
        &mut a,
        Some(&endpoint),
        &"tab".into(),
        harness.deadline.min(Instant::now() + IO_BUDGET),
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?
    .context("replacement not acquired")?;
    ensure!(
        target.actor.as_ref() == "new" && target.url.as_deref() == Some("https://new/"),
        "wrong replacement metadata"
    );
    harness.join("snapshot-new")?;
    harness.join("metadata-peer")?;
    drop(a);
    harness.join("handler-a")?;
    ensure!(
        lock_or_recover!(harness.state.rpc_writer).is_none(),
        "A did not release slot on actual return"
    );
    ensure!(
        harness.state.cancellation.reason().is_none(),
        "healthy handover cancelled origin"
    );
    let health = harness.state.ownership_health();
    ensure!(
        health["pending_ordinary"] == 0
            && health["pending_async"] == 0
            && health["departures_with_outstanding"] == 0,
        "post-A ledger: {health}"
    );

    let mut b = authenticated_client(harness, "handler-b", &firefox_writer)?;
    let deadline = harness.deadline;
    let (returned_tx, returned_rx) = mpsc::sync_channel(1);
    harness.spawn("caller-b", WorkerPolicy::Client, move || {
        let result = b
            .with_read_deadline(deadline, RootActor::list_tabs)
            .map_err(anyhow::Error::from)
            .and_then(|tabs| {
                ensure!(tabs.len() == 1, "B parsed wrong tab count");
                Ok(tabs[0].title.clone())
            });
        returned_tx
            .send(result)
            .map_err(|_| anyhow::anyhow!("B observer gone"))?;
        Ok(())
    })?;
    read_request(harness, &mut peer_reader, "B")?;
    send_reply(harness, &mut peer_writer, B_MARKER)?;
    let answer =
        returned_rx.recv_timeout(harness.deadline.saturating_duration_since(Instant::now()))??;
    harness.join("caller-b")?;
    harness.join("handler-b")?;
    ensure!(
        answer == B_MARKER,
        "actual RootActor B parsed another owner's answer"
    );
    let health = harness.state.ownership_health();
    ensure!(
        health["pending"] == 0
            && health["departures_with_outstanding"] == 0
            && health["terminal_reason"].is_null()
            && harness.state.cancellation.reason().is_none(),
        "post-B ledger: {health}"
    );
    harness.phase("healthy_replacement_and_successor", &health);
    Ok(())
}

#[test]
fn observed_navigation_skips_old_request_and_keeps_successor_healthy() {
    let state = super::super::tests::test_state_with_queues(8, ff_rdp_core::release_queue(1).0);
    let mut harness = Harness::new("retiring-document-successor", state);
    let outcome = catch_unwind(AssertUnwindSafe(|| schedule(&mut harness)));
    let active_elapsed = harness.started.elapsed();
    let cleanup = harness.cleanup();
    let elapsed = harness.started.elapsed();
    assert!(
        cleanup.is_empty(),
        "actual joined cleanup failures: {cleanup:?}"
    );
    assert!(
        active_elapsed < ACTIVE_BUDGET && elapsed < CASE_BUDGET,
        "existing fixture budgets exceeded"
    );
    match outcome {
        Ok(result) => result.expect("actual healthy successor schedule"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

// Regression derived from the independently executed in-flight composition proof.
// This does not attribute either historical closing failure.
fn orphan_successor_schedule(harness: &mut Harness) -> Result<()> {
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
    *lock_or_recover!(harness.state.primary_target) = Some(PrimaryTarget {
        startup_recovery: StartupRecovery::Finished,
        descriptor: "tab".into(),
        watcher: "watcher".into(),
        initial_browsing_context: Some(11),
        current: Some(json!({"actor":"old","consoleActor":"old/console",
            "innerWindowId":7,"browsingContextID":11,"url":"https://old/"})),
    });

    let mut a = authenticated_client(harness, "handler-a", &firefox_writer)?;
    a.send_ordinary(&json!({"to":"root","type":"listTabs"}))?;
    read_request(harness, &mut peer_reader, "A-initial")?;
    send_reply(harness, &mut peer_writer, A_MARKER)?;
    let first = ff_rdp_core::transport::recv_reply_from(&mut a, "root")?;
    ensure!(first["tabs"][0]["title"] == A_MARKER, "initial A reply");
    let a_owner = lock_or_recover!(harness.state.rpc_writer)
        .as_ref()
        .map(|(id, ..)| *id)
        .context("A did not acquire the real slot")?;
    ensure!(
        harness.state.ownership_health()["pending"] == 0,
        "setup debt"
    );
    ensure!(
        !a.is_retiring_document(7),
        "old document was already retiring"
    );
    a.set_target_guard(Some(99));

    // This peer cannot deliver the lifecycle event until the OLD ordinary
    // metadata request has actually crossed the production handler/ledger.
    // It deliberately never sends an ordinary reply from actor `old`.
    let (old_observed_tx, old_observed_rx) = mpsc::sync_channel(1);
    let mut old_reader = FramedReader::from_stream(peer_reader.try_clone_stream()?);
    let mut old_writer = FramedWriter::from_stream(peer_reader.try_clone_stream()?);
    let state = Arc::clone(&harness.state);
    harness.spawn("old-metadata-peer", WorkerPolicy::Client, move || {
        let request = old_reader.recv()?;
        ensure!(
            request == json!({"to":"old","type":"listFrames"}),
            "FIRST metadata request must already be old/listFrames, got {request}"
        );
        let health = state.ownership_health();
        ensure!(
            health["pending"] == 1
                && health["pending_ordinary"] == 1
                && health["pending_async"] == 0
                && health["retiring"] == false,
            "old request did not create the sole ordinary obligation: {health}"
        );
        old_observed_tx
            .send((request, health))
            .context("old observation receiver")?;
        old_writer.send(&json!({"from":"watcher","type":"resources-available-array",
            "array":[["document-event",[{"name":"will-navigate","isFrameSwitching":false,
                "innerWindowId":7,"browsingContextID":11,"newURI":"https://replacement/"}]]]}))?;
        Ok(())
    })?;
    let endpoint = snapshot_endpoint(harness, &firefox_writer, "snapshot-old")?;
    let old = resolve_target_snapshot(
        &mut a,
        Some(&endpoint),
        &"tab".into(),
        harness.deadline.min(Instant::now() + IO_BUDGET),
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?;
    let (request, observed_health) =
        old_observed_rx.recv_timeout(harness.deadline.saturating_duration_since(Instant::now()))?;
    harness.join("snapshot-old")?;
    harness.join("old-metadata-peer")?;
    ensure!(
        old.is_none(),
        "interrupted metadata must return None from the real caller"
    );
    ensure!(
        a.target_guard() == Some(99),
        "interrupted metadata lost outer guard"
    );
    ensure!(
        a.is_retiring_document(7),
        "actual translated willNavigate was not received"
    );
    ensure!(
        a.take_navigation_started().as_deref() == Some("https://replacement/"),
        "navigation latch"
    );
    let health = harness.state.ownership_health();
    ensure!(
        health["pending"] == 1
            && health["pending_ordinary"] == 1
            && health["pending_async"] == 0
            && health["departures_with_outstanding"] == 0
            && health["retiring"] == false
            && health["terminal_reason"].is_null()
            && harness.state.cancellation.reason().is_none(),
        "local interruption changed the real outstanding obligation: {health}"
    );
    ensure!(
        lock_or_recover!(harness.state.rpc_writer)
            .as_ref()
            .map(|(id, ..)| *id)
            == Some(a_owner),
        "A lost its slot before departure"
    );
    harness.phase(
        "old_request_before_navigation_then_local_none",
        &json!({
            "request":request,"peer_observed_ledger":observed_health,"caller_returned_none":true,
            "restored_guard":99,"client_id":a_owner,"ledger_after_local_return":health,
        }),
    );

    peer_writer.send(&json!({"from":"watcher","type":"target-available-form","target":{
        "actor":"new","consoleActor":"new/console","innerWindowId":8,"browsingContextID":12,
        "targetType":"frame","isTopLevelTarget":true,"isPopup":false,"url":"https://availability-only/"}}))?;
    a.with_read_deadline(harness.deadline, |transport| {
        ff_rdp_core::transport::recv_event_from(transport, "watcher", |event| {
            event["type"] == "target-available-form" && event["target"]["actor"] == "new"
        })
    })?;
    let mut new_reader = FramedReader::from_stream(peer_reader.try_clone_stream()?);
    let mut new_writer = FramedWriter::from_stream(peer_reader.try_clone_stream()?);
    let state = Arc::clone(&harness.state);
    let (new_observed_tx, new_observed_rx) = mpsc::sync_channel(1);
    harness.spawn("new-metadata-peer", WorkerPolicy::Client, move || {
        let request = new_reader.recv()?;
        ensure!(
            request == json!({"to":"new","type":"listFrames"}),
            "NEXT actual request must be replacement metadata, got {request}"
        );
        let health = state.ownership_health();
        ensure!(
            health["pending_ordinary"] == 2 && health["pending"] == 2,
            "new request did not coexist with old debt: {health}"
        );
        new_observed_tx
            .send((request, health))
            .context("new observation receiver")?;
        new_writer.send(&json!({"from":"new","frames":[{"isTopLevel":true,
            "url":"https://iter259.invalid/inflight-replacement"}]}))?;
        Ok(())
    })?;
    let endpoint = snapshot_endpoint(harness, &firefox_writer, "snapshot-new")?;
    let target = resolve_target_snapshot(
        &mut a,
        Some(&endpoint),
        &"tab".into(),
        harness.deadline.min(Instant::now() + IO_BUDGET),
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?
    .context("replacement not returned")?;
    let (request, observed_health) =
        new_observed_rx.recv_timeout(harness.deadline.saturating_duration_since(Instant::now()))?;
    harness.join("snapshot-new")?;
    harness.join("new-metadata-peer")?;
    ensure!(
        target.actor.as_ref() == "new"
            && target.inner_window_id == Some(8)
            && target.url.as_deref() == Some("https://iter259.invalid/inflight-replacement"),
        "real caller returned availability/old metadata: {target:?}"
    );
    ensure!(
        a.target_guard() == Some(99),
        "replacement metadata lost outer guard"
    );
    let health = harness.state.ownership_health();
    ensure!(
        health["pending"] == 1
            && health["pending_ordinary"] == 1
            && health["pending_async"] == 0
            && health["retiring"] == false
            && health["departures_with_outstanding"] == 0
            && health["terminal_reason"].is_null(),
        "successful replacement did not leave exactly old debt: {health}"
    );
    ensure!(
        lock_or_recover!(harness.state.rpc_writer)
            .as_ref()
            .map(|(id, ..)| *id)
            == Some(a_owner),
        "A slot changed before actual client drop"
    );
    harness.phase(
        "replacement_returned_with_old_debt",
        &json!({"request":request,
        "peer_observed_ledger":observed_health,"returned_actor":target.actor,
        "returned_url":target.url,"client_id":a_owner,"ledger":health}),
    );

    drop(a);
    harness.phase(
        "a_client_dropped",
        &json!({"old_reply_ever_sent":false,"client_id":a_owner}),
    );
    harness.join("handler-a")?;
    ensure!(
        lock_or_recover!(harness.state.rpc_writer).is_none(),
        "A slot remains after actual handler return"
    );
    let health = harness.state.ownership_health();
    ensure!(
        harness.state.cancellation.reason().is_none()
            && health["retiring"] == false
            && health["terminal_reason"].is_null()
            && health["pending"] == 1
            && health["pending_ordinary"] == 1
            && health["orphan_pending"] == 1
            && health["orphan_pending_ordinary"] == 1
            && health["active_pending"] == 0
            && health["quarantined_actor_count"] == 1
            && health["pending_async"] == 0
            && health["departures_with_outstanding"] == 1,
        "A departure must retain old debt without retiring distinct actors: {health}"
    );
    harness.phase(
        "a_handler_return_slot_release_and_quarantine",
        &json!({"client_id":a_owner,"ledger":health}),
    );
    // Acquire B only after actual A return. This also leaves no gated B worker
    // behind if the pre-repair source fails the quarantine assertion above.
    let mut b = authenticated_client(harness, "handler-b", &firefox_writer)?;
    let (returned_tx, returned_rx) = mpsc::sync_channel(1);
    let (finish_tx, finish_rx) = mpsc::sync_channel(1);
    let (events_tx, events_rx) = mpsc::channel();
    b.set_event_sink(Some(events_tx));
    let deadline = harness.deadline;
    harness.spawn("caller-b", WorkerPolicy::Client, move || {
        let result = b
            .with_read_deadline(deadline, RootActor::list_tabs)
            .map_err(|error| error.to_string())
            .and_then(|tabs| {
                if tabs.len() == 1 {
                    Ok(tabs[0].title.clone())
                } else {
                    Err(format!("B parsed {} tabs", tabs.len()))
                }
            });
        returned_tx
            .send((result, events_rx.try_iter().collect::<Vec<_>>()))
            .context("B result observer missing")?;
        // Keep the current owner present while the observer checks its debt.
        finish_rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .context("B finish gate not released")?;
        Ok(())
    })?;
    read_request(harness, &mut peer_reader, "B-distinct-root")?;
    let b_owner = lock_or_recover!(harness.state.rpc_writer)
        .as_ref()
        .map(|(id, ..)| *id)
        .context("B did not acquire the slot")?;
    ensure!(b_owner != a_owner, "successor reused A client identity");
    let health = harness.state.ownership_health();
    ensure!(
        health["pending"] == 2
            && health["orphan_pending"] == 1
            && health["active_pending"] == 1
            && health["departures_with_outstanding"] == 1,
        "B inherited or erased old debt: {health}"
    );
    harness.phase("distinct_successor_reserved_with_orphan_debt", &health);
    // Ordered on the actual Firefox stream: OLD completion first, B's own reply
    // second. Inspect RootActor's side channel too: sibling replies must not be
    // silently delivered as an event while RootActor waits for its own answer.
    peer_writer.send(&json!({"from":"old","frames":[{"isTopLevel":true,
        "url":"https://iter259.invalid/DELAYED-OLD"}]}))?;
    send_reply(harness, &mut peer_writer, B_MARKER)?;
    let (returned, events) =
        returned_rx.recv_timeout(harness.deadline.saturating_duration_since(Instant::now()))?;
    let health = harness.state.ownership_health();
    // Capture status while B owns the slot, then release and actually join B
    // before assertions. A routing-mutation failure must still have real returns.
    finish_tx.send(()).context("B finish receiver missing")?;
    harness.join("caller-b")?;
    harness.join("handler-b")?;
    ensure!(
        returned.as_deref() == Ok(B_MARKER),
        "B did not parse its own answer: {returned:?}"
    );
    ensure!(
        events.iter().all(|event| event["from"] != "old"),
        "delayed old reply escaped to B's side channel: {events:?}"
    );
    ensure!(
        health["pending"] == 0
            && health["active_pending"] == 0
            && health["orphan_pending"] == 0
            && health["quarantined_actor_count"] == 0
            && health["discarded_replies"] == 1
            && health["departures_with_outstanding"] == 1
            && health["retiring"] == false
            && harness.state.cancellation.reason().is_none(),
        "late old reply was not exclusively accounted and discarded: {health}"
    );
    harness.phase(
        "actual_b_answer_after_delayed_old_reply_discard",
        &json!({"result":returned,"events":events,"ledger":health}),
    );
    ensure!(
        lock_or_recover!(harness.state.rpc_writer).is_none(),
        "B slot remains after actual return"
    );
    ensure!(
        harness.state.ownership_health()["departures_with_outstanding"] == 1,
        "healthy B departure counted earlier orphan debt again"
    );
    Ok(())
}

#[test]
fn interrupted_metadata_quarantines_old_actor_and_delivers_distinct_successor() {
    let state = super::super::tests::test_state_with_queues(8, ff_rdp_core::release_queue(1).0);
    let mut harness = Harness::new("orphan-metadata-successor", state);
    let outcome = catch_unwind(AssertUnwindSafe(|| orphan_successor_schedule(&mut harness)));
    let active_elapsed = harness.started.elapsed();
    let body = match &outcome {
        Ok(Ok(())) => json!({"outcome":"returned_ok"}),
        Ok(Err(error)) => json!({"outcome":"returned_error","error":format!("{error:#}")}),
        Err(panic) => json!({"outcome":"panicked","message":panic.downcast_ref::<String>()
            .map(String::as_str).or_else(|| panic.downcast_ref::<&str>().copied())}),
    };
    harness.phase("proof_body_outcome", &body);
    let cleanup = harness.cleanup();
    let elapsed = harness.started.elapsed();
    assert!(
        cleanup.is_empty(),
        "actual joined cleanup failures: {cleanup:?}"
    );
    assert!(
        active_elapsed < ACTIVE_BUDGET && elapsed < CASE_BUDGET,
        "unchanged fixture bounds exceeded"
    );
    match outcome {
        Ok(result) => result.expect("quarantined in-flight metadata successor regression"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
    assert!(
        harness.workers.len() == 9 && harness.workers.iter().all(|worker| worker.joined),
        "every acquired worker must have an actual supervised join"
    );
}
