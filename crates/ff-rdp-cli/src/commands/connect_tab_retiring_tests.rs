//! Owned deterministic peers: first emitted request is the exclusion oracle.
use super::*;
use anyhow::{Context, Result, ensure};
use ff_rdp_core::transport::{encode_frame, recv_from};
use serde_json::Value;
use std::io::{BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::JoinHandle;

const IO: Duration = Duration::from_secs(2);

fn accept(listener: &TcpListener) -> Result<TcpStream> {
    let mut socket = mio::net::TcpListener::from_std(listener.try_clone()?);
    let mut poll = mio::Poll::new()?;
    poll.registry()
        .register(&mut socket, mio::Token(0), mio::Interest::READABLE)?;
    let mut events = mio::Events::with_capacity(1);
    let deadline = Instant::now() + IO;
    loop {
        ensure!(Instant::now() < deadline, "fixture accept deadline");
        match socket.accept() {
            Ok((stream, _)) => {
                let stream = TcpStream::from(stream);
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(IO))?;
                stream.set_write_timeout(Some(IO))?;
                return Ok(stream);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error.into()),
        }
        poll.poll(
            &mut events,
            Some(deadline.saturating_duration_since(Instant::now())),
        )?;
    }
}

fn pair(proxy: bool) -> Result<(RdpTransport, TcpStream)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let mut transport = RdpTransport::connect_raw("127.0.0.1", listener.local_addr()?.port(), IO)?;
    let peer = accept(&listener)?;
    if proxy {
        transport.enable_proxy_v3();
    }
    Ok((transport, peer))
}
fn send(peer: &mut TcpStream, value: &Value) -> Result<()> {
    peer.write_all(encode_frame(&value.to_string()).as_bytes())?;
    Ok(())
}
fn form(actor: &str, document: u64) -> Value {
    json!({"actor":actor,"consoleActor":format!("{actor}/console"),"innerWindowId":document,"url":"https://old/"})
}
fn side(forms: Vec<Value>) -> Result<(TargetEndpoint, JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let endpoint = TargetEndpoint::new(listener.local_addr()?.port(), "token");
    let worker = std::thread::spawn(move || {
        for target in forms {
            let mut peer = accept(&listener)?;
            let mut reader = BufReader::new(peer.try_clone()?);
            ensure!(recv_from(&mut reader)?["auth"] == "token", "side auth");
            send(
                &mut peer,
                &json!({"protocol_version":crate::daemon::server::DAEMON_PROTOCOL_VERSION}),
            )?;
            let request = recv_from(&mut reader)?;
            ensure!(
                request["type"] == "resolve-tab-target",
                "side request: {request}"
            );
            send(
                &mut peer,
                &json!({"from":"daemon","type":"resolve-tab-target","state":"live","target":target}),
            )?;
        }
        Ok(())
    });
    Ok((endpoint, worker))
}
fn joins(workers: Vec<(&'static str, JoinHandle<Result<()>>)>) -> Result<()> {
    let outcomes: Vec<_> = workers
        .into_iter()
        .map(|(name, worker)| (name, worker.join()))
        .collect();
    let mut failures = Vec::new();
    for (name, outcome) in outcomes {
        let disposition = match &outcome {
            Ok(Ok(())) => "returned_ok",
            Ok(Err(_)) => "returned_error",
            Err(_) => "panicked",
        };
        // stderr-ok: cfg(test) evidence of actual owned worker joins for qualification.
        eprintln!(
            "ITER259_CALLER_JOIN {}",
            json!({"worker":name,"actual_join":true,"outcome":disposition})
        );
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(error)) => failures.push(format!("{error:#}")),
            Err(_) => failures.push("peer panicked".to_owned()),
        }
    }
    ensure!(
        failures.is_empty(),
        "actual joined peer failures: {failures:?}"
    );
    Ok(())
}
fn packet(request: Value, proxy: bool) -> Result<Value> {
    if proxy {
        ensure!(
            request["type"] == "forward" && request["reply_contract"] == "ordinary",
            "explicit ordinary envelope: {request}"
        );
        Ok(request["packet"].clone())
    } else {
        Ok(request)
    }
}

#[test]
fn observed_start_skips_old_snapshot_before_first_new_metadata_request() -> Result<()> {
    let (mut transport, mut peer) = pair(true)?;
    send(
        &mut peer,
        &json!({"from":"old","type":"willNavigate","innerWindowId":7,"url":"https://new/"}),
    )?;
    transport.recv()?;
    ensure!(
        transport.take_navigation_started().as_deref() == Some("https://new/"),
        "navigation latch"
    );
    transport.set_target_guard(Some(77));
    let (endpoint, snapshot_worker) = side(vec![form("old", 7)])?;
    let metadata_worker = std::thread::spawn(move || {
        let mut reader = BufReader::new(peer.try_clone()?);
        let first = packet(recv_from(&mut reader)?, true)?;
        ensure!(
            first == json!({"to":"new","type":"listFrames"}),
            "FIRST metadata request must target replacement, got {first}"
        );
        send(
            &mut peer,
            &json!({"from":"new","frames":[{"isTopLevel":true,"url":"https://new/"}]}),
        )
    });
    let mut workers = vec![
        ("old-snapshot-peer", snapshot_worker),
        ("metadata-peer", metadata_worker),
    ];
    let result = (|| -> Result<()> {
        let deadline = Instant::now() + IO;
        ensure!(
            resolve_target_snapshot(&mut transport, Some(&endpoint), &"tab".into(), deadline)
                .map_err(|error| anyhow::anyhow!("{error}"))?
                .is_none(),
            "outgoing snapshot remained eligible"
        );
        ensure!(
            transport.target_guard() == Some(77),
            "excluded snapshot changed outer guard"
        );
        // Acquire a replacement peer only after the old observation returned.
        // A mutation that emits the old request therefore has no unused accept
        // worker waiting for a second query during cleanup.
        let (replacement_endpoint, replacement_worker) = side(vec![form("new", 8)])?;
        workers.push(("replacement-snapshot-peer", replacement_worker));
        let new = resolve_target_snapshot(
            &mut transport,
            Some(&replacement_endpoint),
            &"tab".into(),
            deadline,
        )
        .map_err(|error| anyhow::anyhow!("{error}"))?
        .context("replacement missing")?;
        ensure!(
            new.actor.as_ref() == "new"
                && new.inner_window_id == Some(8)
                && new.url.as_deref() == Some("https://new/"),
            "replacement metadata incorrect"
        );
        ensure!(
            transport.target_guard() == Some(77),
            "metadata guard not restored"
        );
        Ok(())
    })();
    drop(transport);
    let joined = joins(workers);
    joined?;
    result
}

#[test]
fn unannounced_same_document_and_unrelated_scopes_still_refresh_url() -> Result<()> {
    for (proxy, event) in [
        (
            false,
            json!({"type":"willNavigate","innerWindowId":7,"url":"https://new/"}),
        ),
        (
            true,
            json!({"type":"tabNavigated","state":"start","innerWindowId":7,"url":"https://new/"}),
        ),
        (true, json!({"type":"willNavigate","url":"https://new/"})),
        (
            true,
            json!({"type":"willNavigate","innerWindowId":99,"url":"https://new/"}),
        ),
        (
            true,
            json!({"type":"frameUpdate","from":"same","frames":[]}),
        ),
    ] {
        let (mut transport, mut peer) = pair(proxy)?;
        send(&mut peer, &event)?;
        transport.recv()?;
        let _ = transport.take_navigation_started();
        let (endpoint, snapshot_worker) = side(vec![form("same", 7)])?;
        let metadata_worker = std::thread::spawn(move || {
            let mut reader = BufReader::new(peer.try_clone()?);
            let first = packet(recv_from(&mut reader)?, proxy)?;
            ensure!(
                first == json!({"to":"same","type":"listFrames"}),
                "scope suppressed/changed required metadata: {first}"
            );
            send(
                &mut peer,
                &json!({"from":"same","frames":[{"isTopLevel":true,"url":"https://old/#fragment"}]}),
            )
        });
        let result = resolve_target_snapshot(
            &mut transport,
            Some(&endpoint),
            &"tab".into(),
            Instant::now() + IO,
        );
        drop(transport);
        joins(vec![
            ("snapshot-peer", snapshot_worker),
            ("metadata-peer", metadata_worker),
        ])?;
        let target = result
            .map_err(|error| anyhow::anyhow!("{error}"))?
            .context("same document wrongly excluded")?;
        ensure!(
            target.inner_window_id == Some(7)
                && target.url.as_deref() == Some("https://old/#fragment"),
            "same-document URL not refreshed: {event}"
        );
    }
    Ok(())
}

#[test]
fn cancelled_snapshot_preserves_metadata_and_does_not_extend_expired_deadline() -> Result<()> {
    let (mut transport, mut peer) = pair(true)?;
    send(
        &mut peer,
        &json!({"from":"old","type":"willNavigate","innerWindowId":7,"url":"https://new/"}),
    )?;
    transport.recv()?;
    let _ = transport.take_navigation_started();
    let (endpoint, snapshot_worker) = side(vec![form("old", 7)])?;
    let peer_worker = std::thread::spawn(move || {
        let first = recv_from(&mut BufReader::new(peer.try_clone()?))?;
        ensure!(
            first == json!({"to":"daemon","type":"cancelled-check-complete"}),
            "cancelled/no-replacement sent a forbidden metadata request: {first}"
        );
        Ok(())
    });
    let mut ctx = ConnectedTab::for_test(transport, "old/console".into());
    ctx.target_endpoint = Some(endpoint);
    ctx.set_target_metadata_for_test(Some(7), Some("https://old/".to_owned()));
    let before = ctx.target().clone();
    let result = (|| -> Result<()> {
        ctx.refresh_target();
        let deadline = Instant::now();
        let began = Instant::now();
        let expired = ctx.refresh_target_until(deadline);
        ensure!(
            matches!(expired, Err(AppError::Timeout(_))),
            "expired deadline was widened: {expired:?}"
        );
        ensure!(
            began.elapsed() < IO,
            "expired request consumed another I/O budget"
        );
        ensure!(
            ctx.target().actor == before.actor
                && ctx.target().url == before.url
                && ctx.target().inner_window_id == before.inner_window_id,
            "cancelled acquisition invalidated cached target"
        );
        ctx.transport_mut()
            .send(&json!({"to":"daemon","type":"cancelled-check-complete"}))?;
        Ok(())
    })();
    drop(ctx);
    let joined = joins(vec![
        ("snapshot-peer", snapshot_worker),
        ("cancelled-peer", peer_worker),
    ]);
    joined?;
    result
}
