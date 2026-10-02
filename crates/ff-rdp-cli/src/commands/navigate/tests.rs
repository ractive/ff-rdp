#[cfg(test)]
mod tests {
    use super::*;

    fn default_wait_opts<'a>() -> WaitAfterNav<'a> {
        WaitAfterNav {
            wait_text: None,
            wait_selector: None,
            wait_timeout: 5000,
            no_wait: false,
            wait_for: &[],
            wait_level: WaitLevel::Complete,
            wait_strategy: WaitStrategy::Events,
        }
    }

    // -----------------------------------------------------------------------
    // iter-169 Theme A/B
    // -----------------------------------------------------------------------

    /// AC: "no grace window in `navigate.rs` is longer than the 2000 ms
    /// iter-166 set — the fix must be in delivery, not in waiting."
    ///
    /// iter-169 measured that the residual `no_status_reported` failures were
    /// caused by a blocking round-trip discarding the status update, not by
    /// the wait being too short (30 cold-start runs: the one failure sat out
    /// the full 2 034 ms). Widening the window can therefore only make the
    /// command slower, never more correct — so pin the ceiling here.
    #[test]
    fn unit_169_grace_budget_is_capped() {
        for reason in [
            None,
            Some(StatusUnknown::NotObserved),
            Some(StatusUnknown::NoDocumentRequest),
            Some(StatusUnknown::NoStatusReported),
        ] {
            let budget = status_grace_budget_ms(reason);
            assert!(
                budget <= MAX_STATUS_GRACE_MS,
                "grace budget for {reason:?} is {budget}ms, above the {MAX_STATUS_GRACE_MS}ms \
                 ceiling iter-166 set — fix the delivery, not the wait"
            );
        }
        // The three distinct budgets, spelled out so a silent reshuffle of the
        // match arms fails here rather than in a live sweep.
        assert_eq!(status_grace_budget_ms(Some(StatusUnknown::NotObserved)), 0);
        assert_eq!(
            status_grace_budget_ms(Some(StatusUnknown::NoDocumentRequest)),
            300
        );
        assert_eq!(
            status_grace_budget_ms(Some(StatusUnknown::NoStatusReported)),
            MAX_STATUS_GRACE_MS
        );
    }

    /// Theme B: the `--no-wait` envelope fragment names the reason it has no
    /// status instead of leaving both keys off (which is what made a `reload`
    /// indistinguishable from `navigate`'s meaningful `null`).
    #[test]
    fn unit_169_not_observed_status_carries_both_keys() {
        let map = not_observed_status();
        assert_eq!(map.get("status"), Some(&Value::Null));
        assert_eq!(
            map.get("status_reason").and_then(Value::as_str),
            Some("not_observed")
        );
    }

    /// The wire strings are part of the CLI's contract — a rename would
    /// silently break every caller matching on them.
    #[test]
    fn unit_169_status_reason_wire_strings_are_stable() {
        assert_eq!(StatusUnknown::NotObserved.as_str(), "not_observed");
        assert_eq!(
            StatusUnknown::NoDocumentRequest.as_str(),
            "no_document_request"
        );
        assert_eq!(
            StatusUnknown::NoStatusReported.as_str(),
            "no_status_reported"
        );
    }

    // -----------------------------------------------------------------------
    // iter-92 Theme B: unit_navigate_rejects_stale_ready_state
    //
    // Verifies the freshness helper: a `readyState == complete` reading whose
    // navigationStart predates the pre-epoch (from before the navigate dispatch)
    // must be treated as stale so the poll keeps waiting.
    // -----------------------------------------------------------------------

    /// `unit_navigate_rejects_stale_ready_state`:
    ///
    /// Feed the `is_readystate_fresh` helper a `navigationStart` that is equal
    /// to or older than the pre-epoch and assert it returns `false`.  Then feed
    /// a fresh `navigationStart` and assert `true`.
    #[test]
    fn unit_navigate_rejects_stale_ready_state() {
        let pre_epoch = 1_000_000.0_f64;

        // Stale: navigationStart == pre_epoch (same load, not a new nav).
        assert!(
            !is_readystate_fresh(pre_epoch, pre_epoch),
            "navigationStart equal to pre_epoch must be stale"
        );

        // Stale: navigationStart < pre_epoch.
        assert!(
            !is_readystate_fresh(pre_epoch - 100.0, pre_epoch),
            "navigationStart before pre_epoch must be stale"
        );

        // Fresh: navigationStart clearly after pre_epoch.
        assert!(
            is_readystate_fresh(pre_epoch + 1.0, pre_epoch),
            "navigationStart 1 ms after pre_epoch must be fresh"
        );
        assert!(
            is_readystate_fresh(pre_epoch + 5000.0, pre_epoch),
            "navigationStart 5 s after pre_epoch must be fresh"
        );
    }

    #[test]
    fn wait_after_nav_no_condition_returns_none() {
        let opts = default_wait_opts();
        assert!(!opts.has_condition());
    }

    #[test]
    fn wait_after_nav_text_has_condition() {
        let opts = WaitAfterNav {
            wait_text: Some("Hello"),
            ..default_wait_opts()
        };
        assert!(opts.has_condition());
    }

    #[test]
    fn wait_after_nav_selector_has_condition() {
        let opts = WaitAfterNav {
            wait_selector: Some("button.submit"),
            ..default_wait_opts()
        };
        assert!(opts.has_condition());
    }

    #[test]
    fn describe_wait_condition_selector() {
        let opts = WaitAfterNav {
            wait_selector: Some("div#main"),
            wait_timeout: 3000,
            ..default_wait_opts()
        };
        assert_eq!(describe_wait_condition(&opts), r#"selector="div#main""#);
    }

    #[test]
    fn describe_wait_condition_text() {
        let opts = WaitAfterNav {
            wait_text: Some("Loaded"),
            wait_timeout: 3000,
            ..default_wait_opts()
        };
        assert_eq!(describe_wait_condition(&opts), r#"text="Loaded""#);
    }

    #[test]
    fn no_wait_field_skips_commit_wait() {
        let opts = WaitAfterNav {
            no_wait: true,
            ..default_wait_opts()
        };
        assert!(opts.no_wait);
        assert!(!opts.has_condition());
    }

    #[test]
    fn wait_for_empty_slice_is_none() {
        let opts = default_wait_opts();
        assert!(opts.wait_for.is_empty());
    }

    // -----------------------------------------------------------------------
    // Theme F / B: neterror detection + typed NavCause mapping
    // -----------------------------------------------------------------------

    #[test]
    fn classify_neterror_dns_not_found() {
        let url = "about:neterror?e=dnsNotFound&u=https%3A//bad.invalid/";
        let e_param = classify_neterror(url).unwrap();
        assert_eq!(e_param, "dnsNotFound");
        assert_eq!(NavCause::from_e_param(e_param), NavCause::DnsFail);
    }

    #[test]
    fn classify_neterror_connection_failure() {
        let url = "about:neterror?e=connectionFailure&u=foo";
        let e_param = classify_neterror(url).unwrap();
        assert_eq!(NavCause::from_e_param(e_param), NavCause::ConnReset);
    }

    #[test]
    fn classify_neterror_unknown_code_passthrough() {
        let url = "about:neterror?e=someNewFirefoxCode&u=foo";
        let e_param = classify_neterror(url).unwrap();
        assert!(matches!(
            NavCause::from_e_param(e_param),
            NavCause::Unknown(_)
        ));
    }

    #[test]
    fn classify_neterror_returns_none_for_non_neterror() {
        assert!(classify_neterror("https://example.com").is_none());
        assert!(classify_neterror("about:blank").is_none());
    }

    #[test]
    fn is_neterror_url_detects_about_neterror() {
        assert!(is_neterror_url("about:neterror?e=dnsNotFound"));
        assert!(!is_neterror_url("https://example.com"));
        assert!(!is_neterror_url("about:blank"));
    }

    // -----------------------------------------------------------------------
    // Theme G: cross-origin URL matching
    // -----------------------------------------------------------------------

    #[test]
    fn urls_match_scheme_host_path_identical() {
        assert!(urls_match_scheme_host_path(
            "https://example.com/path",
            "https://example.com/path"
        ));
    }

    #[test]
    fn urls_match_scheme_host_path_strips_query() {
        assert!(urls_match_scheme_host_path(
            "https://example.com/path?q=1",
            "https://example.com/path?q=2"
        ));
        assert!(urls_match_scheme_host_path(
            "https://example.com/path?q=1",
            "https://example.com/path"
        ));
    }

    #[test]
    fn urls_match_scheme_host_path_strips_hash() {
        assert!(urls_match_scheme_host_path(
            "https://example.com/path#a",
            "https://example.com/path#b"
        ));
    }

    #[test]
    fn urls_match_scheme_host_path_strips_trailing_slash() {
        assert!(urls_match_scheme_host_path(
            "https://example.com/path/",
            "https://example.com/path"
        ));
    }

    #[test]
    fn urls_do_not_match_different_paths_scheme_host_path() {
        assert!(!urls_match_scheme_host_path(
            "https://example.com/a",
            "https://example.com/b"
        ));
        assert!(!urls_match_scheme_host_path(
            "https://example.com/",
            "https://other.com/"
        ));
    }

    /// iter-83 Theme C: assert the default `WaitStrategy` is `Both` so the
    /// CLI's documented default (events first, readystate fallback) is exercised
    /// when callers omit `--wait-strategy`.
    #[test]
    fn wait_strategy_default_is_both() {
        assert_eq!(WaitStrategy::default(), WaitStrategy::Both);
    }

    /// iter-85 Theme C: the `Both` budget-split formula must always reserve at
    /// least 1 000 ms for the readystate fallback — even at the default 10 s
    /// timeout — so the fallback has a meaningful window instead of 0 ms
    /// (the bug that caused example.com to always time out). Revised for the
    /// Ubuntu CI regression: the reserve is also capped at half the total, so
    /// the events wait keeps a real window at small `--timeout` values instead
    /// of collapsing to 1 ms — see `split_wait_budget_exact_values` below.
    #[test]
    fn navigate_both_strategy_reserves_readystate_budget() {
        let timeout_ms: u64 = 10_000; // default cli.timeout
        let (reserved_ms, events_budget) = split_wait_budget(timeout_ms);
        // Reserved slice must be at least 1 s.
        assert!(
            reserved_ms >= 1000,
            "readystate reserve must be ≥ 1000 ms; got {reserved_ms}"
        );
        // Events budget must get at least half the total timeout.
        assert!(
            events_budget >= timeout_ms / 2,
            "events budget must be ≥ half the timeout; got events_budget={events_budget}, \
             timeout={timeout_ms}"
        );
        // The two slices must not exceed the total budget.
        assert!(
            events_budget + reserved_ms <= timeout_ms,
            "events_budget ({events_budget}) + reserved_ms ({reserved_ms}) \
             exceeds timeout ({timeout_ms})"
        );
    }

    /// Regression test for the Ubuntu CI failure: at `--timeout 1000` (used by
    /// the e2e tests `navigate_outputs_json_envelope` and
    /// `navigate_with_jq_extracts_url`), the old formula reserved the *entire*
    /// 1000 ms for the readystate fallback, leaving `events_budget == 1` and
    /// causing the events wait to time out instantly ("timed out after 0ms
    /// (phase: recv)"). Pin the exact split across the timeout range,
    /// including tiny inputs that must not panic.
    #[test]
    fn split_wait_budget_exact_values() {
        assert_eq!(split_wait_budget(1000), (500, 500));
        assert_eq!(split_wait_budget(10_000), (3000, 7000)); // unchanged at the default
        assert_eq!(split_wait_budget(2000), (1000, 1000));
        assert_eq!(split_wait_budget(10), (5, 5));
        assert_eq!(split_wait_budget(1), (0, 1));
        assert_eq!(split_wait_budget(0), (0, 0));
    }

    /// iter-83 Theme C: parsing the navigate command without `--wait-strategy`
    /// must resolve to `WaitStrategy::Both`.
    #[test]
    fn navigate_clap_default_wait_strategy_is_both() {
        use clap::Parser as _;
        let cli =
            crate::cli::args::Cli::try_parse_from(["ff-rdp", "navigate", "https://example.com/"])
                .expect("clap parse navigate");
        match cli.command {
            crate::cli::args::Command::Navigate(args) => {
                let wait_strategy = args.wait_strategy;
                assert_eq!(
                    wait_strategy,
                    WaitStrategy::Both,
                    "clap default for --wait-strategy must be Both (iter-83 Theme C)"
                );
            }
            _ => panic!("expected Navigate command variant"),
        }
    }

    // -----------------------------------------------------------------------
    // wait_for_doc_complete — deadline ordering regression test (iter-61w)
    //
    // Verifies that the deadline check fires at the top of the outer loop,
    // so that events flooding the channel do not delay timeout detection beyond
    // `timeout_ms + poll_interval` (100 ms).
    // -----------------------------------------------------------------------

    #[test]
    fn deadline_fires_within_timeout_plus_one_poll_interval() {
        use std::io::Write;
        use std::net::TcpListener;
        use std::time::Instant;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        const TIMEOUT_MS: u64 = 50;
        const POLL_MS: u64 = 100;
        // Maximum allowed elapsed: 50ms timeout + 100ms poll + 200ms margin.
        const MAX_ELAPSED_MS: u64 = TIMEOUT_MS + POLL_MS + 200;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // Spawn a server that only sends the greeting and then idles, so
        // every transport recv times out.  The dom-loading flood that
        // exercises the deadline logic is pre-loaded into the mpsc channel
        // below — the old (post-drain) deadline check could be starved by it.
        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();

            // Send greeting.
            let greeting = serde_json::json!({
                "from": "root",
                "applicationType": "browser",
                "traits": {}
            });
            let _ = writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes());

            // Keep the stream open; send nothing further so the transport times
            // out on every recv call and the deadline logic is exercised.
            // (We don't send dom-complete, so the timeout must fire.)
            std::thread::sleep(Duration::from_secs(1));
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        // Build a ResourceCommand and an mpsc channel whose receiver we pass to
        // wait_for_doc_complete.  We pre-load the channel with many dom-loading
        // events so the inner drain loop has work to do on each iteration.
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<Resource>>();
        let dom_loading = std::sync::Arc::new(Resource::DocumentEvent(serde_json::json!({
            "name": "dom-loading",
            "url": "https://example.com/",
        })));
        // Send enough events to fill several drain batches.
        for _ in 0..1000 {
            tx.send(std::sync::Arc::clone(&dom_loading)).unwrap();
        }

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let bus_arc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));

        let started = Instant::now();
        let result = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            TIMEOUT_MS,
            WaitLevel::Complete,
            started,
            None,
            "https://example.com/",
            // These mock-transport tests exercise `navigate`'s route, which
            // subscribes to `NetworkEvent` (iter-166).
            true,
        );
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

        server_handle.join().unwrap();

        assert!(
            matches!(result, Err(AppError::Timeout(_))),
            "expected Timeout, got: {result:?}"
        );
        assert!(
            elapsed_ms <= MAX_ELAPSED_MS,
            "deadline overrun: elapsed {elapsed_ms}ms > allowed {MAX_ELAPSED_MS}ms"
        );
    }

    // -----------------------------------------------------------------------
    // navigate_bus_lock_released_during_wait (iter-71b AC)
    //
    // Verify that `wait_for_doc_complete` does NOT hold the bus lock across
    // `transport.recv()`.  We do this by attempting to acquire the lock from
    // a second thread while the function is blocked in recv — if the lock were
    // held the second thread would also block, causing the test to time out.
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // navigate_subscribes_before_navigateto (iter-79 Theme A AC)
    //
    // The navigate prelude must issue, in this exact order:
    //   1. watchTargets("frame")           — engages the frame-target stream
    //   2. watchResources(["document-event"]) — engages the resource stream
    //   3. navigateTo                       — triggers the navigation
    //
    // Without (1) Firefox suppresses document-event resources entirely (per
    // the watcher contract), so wait_for_doc_complete never observes the
    // events on a real page and the CLI times out.  This test pins the
    // prelude to that order by capturing outbound packets on a mock server.
    // -----------------------------------------------------------------------

    #[test]
    fn navigate_subscribes_before_navigateto() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame, recv_from};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // Mock Firefox: greeting, then accept three packets.  Reply to the
        // first two (watchTargets, watchResources) so actor_request returns;
        // the third (navigateTo) is fire-and-forget.
        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = std::io::BufReader::new(stream);

            let greeting = serde_json::json!({
                "from": "root",
                "applicationType": "browser",
                "traits": {}
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();

            let p1 = recv_from(&mut reader).unwrap();
            // Reply to watchTargets.
            let reply1 = serde_json::json!({
                "from": p1["to"].as_str().unwrap_or("conn0/watcher1"),
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&reply1).unwrap()).as_bytes())
                .unwrap();

            let p2 = recv_from(&mut reader).unwrap();
            // Reply to watchResources.
            let reply2 = serde_json::json!({
                "from": p2["to"].as_str().unwrap_or("conn0/watcher1"),
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&reply2).unwrap()).as_bytes())
                .unwrap();

            let p3 = recv_from(&mut reader).unwrap();
            (p1, p2, p3)
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let target_actor = ff_rdp_core::ActorId::from("conn0/target1");

        // Drive the prelude exactly as run_core() does: watchTargets, then
        // ResourceCommand::subscribe (which sends watchResources), then a raw
        // navigateTo send.
        WatcherActor::watch_targets(&mut transport, &watcher_actor, "frame").unwrap();

        let mut bus = ResourceCommand::new(watcher_actor.clone());
        let (_sub_id, _rx) = bus
            .subscribe(&mut transport, &[ResourceType::DocumentEvent])
            .unwrap();

        transport
            .send(&json!({
                "to": target_actor.as_ref(),
                "type": "navigateTo",
                "url": "https://example.com/",
            }))
            .unwrap();

        let (p1, p2, p3) = server_handle.join().unwrap();

        assert_eq!(
            p1["type"].as_str(),
            Some("watchTargets"),
            "first packet must be watchTargets, got: {p1}"
        );
        assert_eq!(
            p1["targetType"].as_str(),
            Some("frame"),
            "watchTargets must target 'frame', got: {p1}"
        );
        assert_eq!(
            p2["type"].as_str(),
            Some("watchResources"),
            "second packet must be watchResources, got: {p2}"
        );
        let res_types = p2["resourceTypes"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        assert!(
            res_types.contains(&"document-event"),
            "watchResources must include 'document-event', got: {p2}"
        );
        assert_eq!(
            p3["type"].as_str(),
            Some("navigateTo"),
            "third packet must be navigateTo, got: {p3}"
        );
        assert_eq!(
            p3["url"].as_str(),
            Some("https://example.com/"),
            "navigateTo URL must match request, got: {p3}"
        );
    }

    #[test]
    fn navigate_bus_lock_released_during_wait() {
        use std::io::Write as _;
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicBool, Ordering};

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // Server: send greeting, then sleep 500ms before sending anything else
        // so the transport blocks in recv for that window.
        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let greeting =
                serde_json::json!({"from": "root", "applicationType": "browser", "traits": {}});
            let _ = writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes());
            std::thread::sleep(Duration::from_millis(500));
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        // short timeout so wait_for_doc_complete times out quickly
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<Resource>>();
        drop(tx); // empty channel — wait will timeout

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let bus_arc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));
        let bus_arc_clone = Arc::clone(&bus_arc);

        // Probe: attempt to acquire the lock from a second thread while
        // wait_for_doc_complete is running. We record whether the lock was
        // acquired within a 300 ms window.
        let lock_acquired = Arc::new(AtomicBool::new(false));
        let lock_acquired_clone = Arc::clone(&lock_acquired);

        let probe_handle = std::thread::spawn(move || {
            // Give wait_for_doc_complete time to start its first recv call.
            std::thread::sleep(Duration::from_millis(60));
            // Try to lock with a generous timeout: if the lock is held across
            // recv() this will block for ~100ms (the poll_interval) or more.
            // We just try to acquire it; success means it was released.
            if bus_arc_clone.try_lock().is_ok() {
                lock_acquired_clone.store(true, Ordering::Relaxed);
            }
        });

        // Run wait_for_doc_complete with a 200ms timeout so the test finishes quickly.
        let _ = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            200,
            WaitLevel::Complete,
            Instant::now(),
            None,
            "https://example.com/",
            // These mock-transport tests exercise `navigate`'s route, which
            // subscribes to `NetworkEvent` (iter-166).
            true,
        );

        probe_handle.join().unwrap();
        server_handle.join().unwrap();

        assert!(
            lock_acquired.load(Ordering::Relaxed),
            "navigate_bus_lock_released_during_wait: second thread could not acquire \
             the bus lock while wait_for_doc_complete was running — lock held too long"
        );
    }

    /// Answer one `getTarget` round-trip on `writer`/`reader` with a `frame`
    /// response carrying `console_actor`.
    ///
    /// The `wait_for_doc_complete` probe refresh (iter-124) issues this call
    /// as soon as `dom-loading` commits (or lazily before its first probe
    /// attempt), so any mock server driving a `probe: Some(..)` case must
    /// answer it before the eval round-trips it gates.
    fn answer_get_target(
        reader: &mut std::io::BufReader<std::net::TcpStream>,
        writer: &mut std::net::TcpStream,
        tab_actor: &str,
        console_actor: &str,
    ) {
        use std::io::Write as _;

        use ff_rdp_core::transport::{encode_frame, recv_from};

        let _req = recv_from(reader).unwrap();
        let response = serde_json::json!({
            "from": tab_actor,
            "frame": { "actor": "conn0/target1", "consoleActor": console_actor },
        });
        writer
            .write_all(encode_frame(&serde_json::to_string(&response).unwrap()).as_bytes())
            .unwrap();
    }

    /// Answer one `getTarget` round-trip on `writer`/`reader` with an
    /// actor-level error reply (`noSuchActor`), matching the wire shape
    /// `recv_reply_from` parses into `ProtocolError::ActorError`.
    ///
    /// Used by the iter-124 review-fix unit tests to prove a transient (or
    /// persistent) `getTarget` failure does not permanently strand the probe
    /// on its stale console actor.
    fn answer_get_target_error(
        reader: &mut std::io::BufReader<std::net::TcpStream>,
        writer: &mut std::net::TcpStream,
        tab_actor: &str,
    ) {
        use std::io::Write as _;

        use ff_rdp_core::transport::{encode_frame, recv_from};

        let _req = recv_from(reader).unwrap();
        let response = serde_json::json!({
            "from": tab_actor,
            "error": "noSuchActor",
            "message": "No such actor for ID: conn0/tabDescriptor1",
        });
        writer
            .write_all(encode_frame(&serde_json::to_string(&response).unwrap()).as_bytes())
            .unwrap();
    }

    /// Answer one `evaluateJSAsync` round-trip on `writer`/`reader` with an
    /// immediate actor-level error reply (`noSuchActor`), simulating an eval
    /// sent to a stale (already-invalidated) console actor. Returns the
    /// `text` the client asked to evaluate so the caller can assert on it.
    ///
    /// Used by the iter-124 review-fix tests: when a probe-timer tick's
    /// `getTarget` refresh fails, `wait_for_doc_complete` still attempts
    /// `probe_readystate_complete` with the (still-stale) actor before
    /// looping — this answers that attempt so the mock server's expected
    /// request sequence matches the real code path exactly.
    fn answer_one_eval_error(
        reader: &mut std::io::BufReader<std::net::TcpStream>,
        writer: &mut std::net::TcpStream,
        console_actor: &str,
    ) -> String {
        use std::io::Write as _;

        use ff_rdp_core::transport::{encode_frame, recv_from};

        let req = recv_from(reader).unwrap();
        let text = req["text"].as_str().unwrap_or_default().to_owned();
        let error = serde_json::json!({
            "from": console_actor,
            "error": "noSuchActor",
            "message": format!("No such actor for ID: {console_actor}"),
        });
        writer
            .write_all(encode_frame(&serde_json::to_string(&error).unwrap()).as_bytes())
            .unwrap();
        text
    }

    /// Answer one `evaluateJSAsync` round-trip on `writer`/`reader`, replying
    /// with the immediate `resultID` ack followed by an `evaluationResult`
    /// carrying `result_value`.  Returns the `text` the client asked to evaluate
    /// so the caller can assert on it.
    ///
    /// Shared by the iter-122 Theme A/B unit tests below.
    fn answer_one_eval(
        reader: &mut std::io::BufReader<std::net::TcpStream>,
        writer: &mut std::net::TcpStream,
        console_actor: &str,
        result_value: &serde_json::Value,
    ) -> String {
        use std::io::Write as _;

        use ff_rdp_core::transport::{encode_frame, recv_from};

        let req = recv_from(reader).unwrap();
        let text = req["text"].as_str().unwrap_or_default().to_owned();
        // Immediate ack (a reply — no `type` field) carrying the resultID.
        let ack = serde_json::json!({ "from": console_actor, "resultID": "r1" });
        writer
            .write_all(encode_frame(&serde_json::to_string(&ack).unwrap()).as_bytes())
            .unwrap();
        // The evaluationResult push event.
        let eval_result = serde_json::json!({
            "from": console_actor,
            "type": "evaluationResult",
            "resultID": "r1",
            "result": result_value,
        });
        writer
            .write_all(encode_frame(&serde_json::to_string(&eval_result).unwrap()).as_bytes())
            .unwrap();
        text
    }

    /// iter-122 Theme A: `unit_navigate_readystate_probe_short_circuits`
    ///
    /// When the events stream never fires `dom-complete` (the FF152 symptom) but
    /// the interleaved probe observes `document.readyState === 'complete'`,
    /// `wait_for_doc_complete` must return promptly — well inside the events
    /// budget — with `ready_state: "complete"` and `committed_url` resolved from
    /// `location.href` (Theme B), never an empty string.
    #[test]
    fn unit_navigate_readystate_probe_short_circuits() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let console_actor = "conn0/console1";

        let tab_actor = "conn0/tabDescriptor1";

        // Mock Firefox: greeting, then answer the probe's lazy console-actor
        // refresh (getTarget — no dom-loading event arrives to trigger the
        // earlier refresh, iter-124) followed by one atomic readiness sample.
        // It never sends any document-event, so only the probe can resolve.
        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = std::io::BufReader::new(stream);

            let greeting = serde_json::json!({
                "from": "root", "applicationType": "browser", "traits": {}
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();

            answer_get_target(&mut reader, &mut writer, tab_actor, console_actor);

            answer_one_eval(
                &mut reader,
                &mut writer,
                console_actor,
                &scripted_readiness(200.0, "https://example.com/", "complete"),
            )
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        // Empty channel — no document-event ever arrives, so the probe is the
        // only way out other than the timeout.
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<Resource>>();
        drop(tx);

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let bus_arc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));
        let tab = ff_rdp_core::ActorId::from(tab_actor);

        let nav_start = Instant::now();
        let mut probe = ReadyStateProbe {
            // Deliberately stale — the pre-navigation actor — to prove the
            // refresh (via `tab_actor`) is what makes the probe usable.
            console_actor: Some(ff_rdp_core::ActorId::from("conn0/stale-console")),
            tab_actor: &tab,
            pre_epoch: Some(1.0),
            // Probe almost immediately so the test does not wait 300 ms.
            first_probe_at: nav_start,
            probe_interval: Duration::from_millis(50),
            poll_enabled: true,
            pre_href: String::new(),
            trust_event_url: true,
        };

        // Generous events budget: the probe must return long before this.
        let result = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            5_000,
            WaitLevel::Complete,
            nav_start,
            Some(&mut probe),
            "https://example.com/",
            // These mock-transport tests exercise `navigate`'s route, which
            // subscribes to `NetworkEvent` (iter-166).
            true,
        );

        let ready_text = server_handle.join().unwrap();

        let ci = result.expect("probe should short-circuit to a CommitInfo");
        assert_eq!(ci.ready_state, "complete");
        assert_eq!(
            ci.committed_url, "https://example.com/",
            "committed_url must come from location.href, not be empty/about:blank"
        );
        assert!(
            ready_text.contains("readyState"),
            "first eval should be the readyState probe, got: {ready_text}"
        );
        assert!(
            ready_text.contains("location.href"),
            "the readiness evaluation must also sample href: {ready_text}"
        );
        assert!(
            nav_start.elapsed() < Duration::from_secs(4),
            "probe must return well inside the events budget; took {:?}",
            nav_start.elapsed()
        );
    }

    /// iter-124 review fix: `unit_navigate_probe_refresh_retries_after_transient_error`
    ///
    /// A `getTarget` failure (e.g. `noSuchActor` because the new docshell
    /// hasn't finished registering server-side yet) must NOT permanently
    /// latch `probe_refreshed` — the review found the original fix set the
    /// latch unconditionally, so one failed attempt stranded the probe on
    /// its stale actor for the rest of the wait, intermittently
    /// reintroducing the exact bug this PR fixes. This test drives the mock
    /// server through: error reply, then a successful `getTarget` reply on
    /// the *next* probe-timer tick, then the readyState + location.href
    /// evals — proving the probe recovers instead of staying stuck.
    #[test]
    fn unit_navigate_probe_refresh_retries_after_transient_error() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let console_actor = "conn0/console1";
        let tab_actor = "conn0/tabDescriptor1";

        // Mock Firefox: greeting, then a FAILED getTarget (noSuchActor) on the
        // first probe tick. `wait_for_doc_complete` still attempts
        // `probe_readystate_complete` with the stale actor after a failed
        // refresh (best-effort — see `refresh_probe_console_actor`'s doc
        // comment), so that stale-actor eval also errors before the loop
        // re-arms and tries again on the SECOND tick: a SUCCESSFUL getTarget,
        // then the atomic sample the now-fresh probe needs to short-circuit.
        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = std::io::BufReader::new(stream);

            let greeting = serde_json::json!({
                "from": "root", "applicationType": "browser", "traits": {}
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();

            answer_get_target_error(&mut reader, &mut writer, tab_actor);
            let stale_eval_text =
                answer_one_eval_error(&mut reader, &mut writer, "conn0/stale-console");

            answer_get_target(&mut reader, &mut writer, tab_actor, console_actor);

            let ready_text = answer_one_eval(
                &mut reader,
                &mut writer,
                console_actor,
                &scripted_readiness(200.0, "https://retry.example/", "complete"),
            );
            (stale_eval_text, ready_text)
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        // Empty channel — no document-event ever arrives, so the probe-timer
        // loop (with its refresh-retry) is the only way out other than the
        // timeout.
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<Resource>>();
        drop(tx);

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let bus_arc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));
        let tab = ff_rdp_core::ActorId::from(tab_actor);

        let nav_start = Instant::now();
        let mut probe = ReadyStateProbe {
            console_actor: Some(ff_rdp_core::ActorId::from("conn0/stale-console")),
            tab_actor: &tab,
            pre_epoch: Some(1.0),
            // Probe almost immediately, then again after a short interval so
            // the second (successful) getTarget attempt happens quickly.
            first_probe_at: nav_start,
            probe_interval: Duration::from_millis(50),
            poll_enabled: true,
            pre_href: String::new(),
            trust_event_url: true,
        };

        let result = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            5_000,
            WaitLevel::Complete,
            nav_start,
            Some(&mut probe),
            "https://retry.example/",
            // These mock-transport tests exercise `navigate`'s route, which
            // subscribes to `NetworkEvent` (iter-166).
            true,
        );

        let (stale_eval_text, ready_text) = server_handle.join().unwrap();

        let ci = result.expect(
            "probe must recover after a transient getTarget failure and \
             short-circuit to a CommitInfo on the retry",
        );
        assert_eq!(ci.ready_state, "complete");
        assert_eq!(
            ci.committed_url, "https://retry.example/",
            "committed_url must come from the post-recovery readiness sample"
        );
        assert!(
            stale_eval_text.contains("readyState"),
            "the best-effort eval attempted with the still-stale actor (after \
             the failed refresh) should be the readyState probe, got: {stale_eval_text}"
        );
        assert!(
            ready_text.contains("readyState"),
            "the eval after recovery should be the readyState probe, got: {ready_text}"
        );
        assert!(
            ready_text.contains("location.href"),
            "the recovered readiness sample must also carry href: {ready_text}"
        );
        assert!(
            nav_start.elapsed() < Duration::from_secs(4),
            "recovery must happen well inside the events budget; took {:?}",
            nav_start.elapsed()
        );
    }

    /// iter-124 review fix: `unit_navigate_probe_refresh_persistent_error_falls_back_to_timeout`
    ///
    /// When `getTarget` fails on every attempt (the docshell genuinely never
    /// becomes queryable during the wait), the probe must keep retrying
    /// without panicking and `wait_for_doc_complete` must fall through
    /// cleanly to the events-budget timeout — never a crash, never a hang
    /// past the deadline.
    #[test]
    fn unit_navigate_probe_refresh_persistent_error_falls_back_to_timeout() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let tab_actor = "conn0/tabDescriptor1";

        // Mock Firefox: greeting, then answer every getTarget-or-eval request
        // with an actor error for as long as the client keeps asking. The
        // client is expected to give up and close its socket once its
        // timeout budget is exhausted (well before the bounded 50-iteration
        // cap below) — that's a normal end condition for this test, not a
        // server bug, so the loop stops quietly on a read/write failure
        // instead of unwrapping and panicking the (detached, unjoined)
        // server thread.
        let server_handle = std::thread::spawn(move || {
            use ff_rdp_core::transport::recv_from;

            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = std::io::BufReader::new(stream);

            let greeting = serde_json::json!({
                "from": "root", "applicationType": "browser", "traits": {}
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();

            for _ in 0..50 {
                let Ok(_req) = recv_from(&mut reader) else {
                    break;
                };
                let error = serde_json::json!({
                    "from": tab_actor,
                    "error": "noSuchActor",
                    "message": "No such actor for ID: conn0/tabDescriptor1",
                });
                if writer
                    .write_all(encode_frame(&serde_json::to_string(&error).unwrap()).as_bytes())
                    .is_err()
                {
                    break;
                }
            }
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        // Empty channel — no document-event ever arrives, so with every
        // refresh attempt also failing, the only way out is the timeout.
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<Resource>>();
        drop(tx);

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let bus_arc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));
        let tab = ff_rdp_core::ActorId::from(tab_actor);

        let nav_start = Instant::now();
        let mut probe = ReadyStateProbe {
            console_actor: Some(ff_rdp_core::ActorId::from("conn0/stale-console")),
            tab_actor: &tab,
            pre_epoch: Some(1.0),
            first_probe_at: nav_start,
            // Short interval + short overall budget below so a persistently
            // failing refresh still exercises several retries without
            // making this test slow.
            probe_interval: Duration::from_millis(50),
            poll_enabled: true,
            pre_href: String::new(),
            trust_event_url: true,
        };

        let budget_ms = 800;
        let result = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            budget_ms,
            WaitLevel::Complete,
            nav_start,
            Some(&mut probe),
            "https://example.com/",
            // These mock-transport tests exercise `navigate`'s route, which
            // subscribes to `NetworkEvent` (iter-166).
            true,
        );

        // The server thread's loop is bounded (50 iterations) purely so it
        // cannot hang the test process if something unexpected happens; the
        // assertion under test is on `result`/timing, not on the server
        // thread's join (which may still be mid-loop when the client times
        // out and stops asking).
        drop(server_handle);

        match result {
            Err(AppError::Timeout(msg)) => {
                assert!(
                    msg.contains("dom-complete"),
                    "timeout message should name the awaited event: {msg}"
                );
            }
            other => panic!(
                "persistent getTarget failure must fall through to a clean \
                 Timeout, not panic or hang; got {other:?}"
            ),
        }
        assert!(
            nav_start.elapsed() < Duration::from_secs(3),
            "must not overrun the {budget_ms}ms budget by more than the poll \
             interval; took {:?}",
            nav_start.elapsed()
        );
    }

    /// iter-122 Theme B: `unit_navigate_dom_complete_empty_url_falls_back_to_href`
    ///
    /// When a `dom-complete` event commits with no URL (an SPA that never fired
    /// `dom-loading` with a real URL), `committed_url` must be resolved from
    /// `location.href` instead of surfacing as an empty string (about:blank).
    #[test]
    fn unit_navigate_dom_complete_empty_url_falls_back_to_href() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let console_actor = "conn0/console1";
        let tab_actor = "conn0/tabDescriptor1";

        // Server: greeting, then the dom-loading-triggered getTarget refresh
        // (iter-124), then a single location.href eval answer (the empty
        // dom-complete triggers exactly one href fetch).
        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = std::io::BufReader::new(stream);

            let greeting = serde_json::json!({
                "from": "root", "applicationType": "browser", "traits": {}
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();

            answer_get_target(&mut reader, &mut writer, tab_actor, console_actor);

            answer_one_eval(
                &mut reader,
                &mut writer,
                console_actor,
                &serde_json::json!("https://spa.example/app"),
            )
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        // Pre-load a dom-loading (empty url) + dom-complete (empty url) sequence:
        // commit_url becomes Some("") so dom-complete resolves, but the URL is
        // empty and must be back-filled via location.href.
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<Resource>>();
        tx.send(std::sync::Arc::new(Resource::DocumentEvent(
            serde_json::json!({ "name": "dom-loading", "url": "" }),
        )))
        .unwrap();
        tx.send(std::sync::Arc::new(Resource::DocumentEvent(
            serde_json::json!({ "name": "dom-complete", "url": "" }),
        )))
        .unwrap();
        drop(tx);

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let bus_arc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));
        let tab = ff_rdp_core::ActorId::from(tab_actor);

        // No probe timer needed — the empty dom-complete triggers the fallback
        // — but a probe must be present so the console_actor is available (and
        // gets refreshed to `console_actor` on the dom-loading event above).
        let nav_start = Instant::now();
        let mut probe = ReadyStateProbe {
            console_actor: Some(ff_rdp_core::ActorId::from("conn0/stale-console")),
            tab_actor: &tab,
            pre_epoch: Some(1.0),
            // Push the probe far into the future so only the dom-complete
            // fallback path (not the interleaved probe) fires.
            first_probe_at: nav_start + Duration::from_secs(30),
            probe_interval: Duration::from_secs(30),
            poll_enabled: true,
            pre_href: String::new(),
            trust_event_url: true,
        };

        let result = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            5_000,
            WaitLevel::Complete,
            nav_start,
            Some(&mut probe),
            "https://spa.example/app",
            // These mock-transport tests exercise `navigate`'s route, which
            // subscribes to `NetworkEvent` (iter-166).
            true,
        );

        server_handle.join().unwrap();

        let ci = result.expect("dom-complete should resolve to a CommitInfo");
        assert_eq!(ci.ready_state, "complete");
        assert_eq!(
            ci.committed_url, "https://spa.example/app",
            "empty dom-complete URL must fall back to location.href, not about:blank"
        );
    }

    /// iter-122 review fix: `unit_navigate_probe_ignored_for_non_complete_wait_level`
    ///
    /// The interleaved readystate probe (Theme A) can only ever observe
    /// `document.readyState === 'complete'`. When the caller asked for
    /// `--wait loading` (or `--wait interactive`), the probe must NOT be
    /// honored even if it fires and reports truthy — only the matching
    /// `dom-loading` event may resolve the wait, with the correct
    /// `ready_state: "loading"` and its own Theme B URL fallback.
    #[test]
    fn unit_navigate_probe_ignored_for_non_complete_wait_level() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let console_actor = "conn0/console1";
        let tab_actor = "conn0/tabDescriptor1";

        // Server: greeting, then the dom-loading-triggered getTarget refresh
        // (iter-124), then a single location.href eval answer used by the
        // dom-loading fallback. If the probe were (incorrectly) honored first it
        // would ask for `readyState` instead — asserted on below.
        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = std::io::BufReader::new(stream);

            let greeting = serde_json::json!({
                "from": "root", "applicationType": "browser", "traits": {}
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();

            answer_get_target(&mut reader, &mut writer, tab_actor, console_actor);

            answer_one_eval(
                &mut reader,
                &mut writer,
                console_actor,
                &serde_json::json!("https://spa.example/loading"),
            )
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        // A dom-loading event with an empty URL arrives after a short delay —
        // long enough for an (incorrectly) armed probe to have fired first.
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<Resource>>();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            tx.send(std::sync::Arc::new(Resource::DocumentEvent(
                serde_json::json!({ "name": "dom-loading", "url": "" }),
            )))
            .unwrap();
        });

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let bus_arc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));
        let tab = ff_rdp_core::ActorId::from(tab_actor);

        let nav_start = Instant::now();
        // Probe is armed to fire almost immediately and would report `true`
        // for `document.readyState` if it were ever evaluated — but it must
        // stay silent because wait_level is `Loading`, not `Complete`.
        let mut probe = ReadyStateProbe {
            console_actor: Some(ff_rdp_core::ActorId::from("conn0/stale-console")),
            tab_actor: &tab,
            pre_epoch: Some(1.0),
            first_probe_at: nav_start,
            probe_interval: Duration::from_millis(10),
            poll_enabled: true,
            pre_href: String::new(),
            trust_event_url: true,
        };

        let result = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            5_000,
            WaitLevel::Loading,
            nav_start,
            Some(&mut probe),
            "https://spa.example/loading",
            // These mock-transport tests exercise `navigate`'s route, which
            // subscribes to `NetworkEvent` (iter-166).
            true,
        );

        let href_text = server_handle.join().unwrap();

        let ci = result.expect("dom-loading should resolve to a CommitInfo");
        assert_eq!(
            ci.ready_state, "loading",
            "probe must not override the requested wait_level with 'complete'"
        );
        assert_eq!(
            ci.committed_url, "https://spa.example/loading",
            "Loading path must apply the same location.href fallback as Interactive/Complete"
        );
        assert!(
            href_text.contains("location.href"),
            "the only eval the mock server should see is the dom-loading URL fallback, got: {href_text}"
        );
    }

    // ── iter-130 Theme A: literal "about:blank" fallback ───────────────────

    #[test]
    fn unit_needs_href_fallback_covers_empty_and_stale_about_blank() {
        // Empty candidate: always needs the fallback (iter-122 Theme B, unchanged).
        assert!(needs_href_fallback("", "https://example.com/"));
        // Literal "about:blank" while a different URL was requested: the
        // comparis.ch SPA route-commit case — needs the fallback.
        assert!(needs_href_fallback(
            "about:blank",
            "https://www.comparis.ch/hypotheken"
        ));
        // A real URL that matches or differs from the request: never needs
        // the fallback — the event's own URL is trustworthy.
        assert!(!needs_href_fallback(
            "https://example.com/",
            "https://example.com/"
        ));
        assert!(!needs_href_fallback(
            "https://example.com/other",
            "https://example.com/"
        ));
        // A genuine navigation TO about:blank must not trigger a spurious
        // fallback round-trip — the requested and committed URLs agree.
        assert!(!needs_href_fallback("about:blank", "about:blank"));
    }

    // ── iter-138 Theme F: must_reresolve_href ──────────────────────────────

    fn probe_with_trust(trust_event_url: bool) -> (ff_rdp_core::ActorId, ReadyStateProbe<'static>) {
        // `tab_actor` needs a stable address for the probe's lifetime; leak a
        // tiny `ActorId` for the test (fine — unit tests are short-lived).
        let tab: &'static ff_rdp_core::ActorId =
            Box::leak(Box::new(ff_rdp_core::ActorId::from("conn0/tabDescriptor1")));
        let probe = ReadyStateProbe {
            console_actor: Some(ff_rdp_core::ActorId::from("conn0/console1")),
            tab_actor: tab,
            pre_epoch: Some(1.0),
            first_probe_at: Instant::now(),
            probe_interval: Duration::from_millis(50),
            poll_enabled: false,
            pre_href: String::new(),
            trust_event_url,
        };
        (tab.clone(), probe)
    }

    /// `unit_must_reresolve_href_no_probe_defers_to_needs_href_fallback` — with
    /// no probe at all (the `Events`-strategy `navigate` case), the decision
    /// collapses to the pre-existing `needs_href_fallback` check.
    #[test]
    fn unit_must_reresolve_href_no_probe_defers_to_needs_href_fallback() {
        assert!(!must_reresolve_href(
            None,
            "https://example.com/",
            "https://example.com/"
        ));
        assert!(must_reresolve_href(None, "", "https://example.com/"));
    }

    /// `unit_must_reresolve_href_trusted_probe_defers_to_needs_href_fallback`
    /// — `trust_event_url: true` (plain `navigate`) behaves exactly like the
    /// no-probe case: a well-formed candidate is trusted verbatim.
    #[test]
    fn unit_must_reresolve_href_trusted_probe_defers_to_needs_href_fallback() {
        let (_tab, probe) = probe_with_trust(true);
        assert!(!must_reresolve_href(
            Some(&probe),
            "https://example.com/",
            "https://example.com/"
        ));
        assert!(must_reresolve_href(
            Some(&probe),
            "about:blank",
            "https://example.com/"
        ));
    }

    /// `unit_must_reresolve_href_untrusted_probe_always_reresolves` — iter-138
    /// Theme F: `trust_event_url: false` (`back`/`forward`/`reload`) forces
    /// re-resolution even for a perfectly well-formed candidate URL, because
    /// it might be a subframe's.
    #[test]
    fn unit_must_reresolve_href_untrusted_probe_always_reresolves() {
        let (_tab, probe) = probe_with_trust(false);
        assert!(
            must_reresolve_href(
                Some(&probe),
                "https://cdn.optimizely.com/client_storage/x.html",
                "https://example.com/"
            ),
            "a well-formed but wrong (subframe) URL must not be trusted when \
             trust_event_url is false"
        );
        assert!(must_reresolve_href(
            Some(&probe),
            "https://example.com/",
            "https://example.com/"
        ));
    }

    // ── iter-138 Theme A/G: extract_document_status ────────────────────────

    fn doc_resource(url: &str, resource_id: u64) -> ff_rdp_core::NetworkResource {
        ff_rdp_core::NetworkResource {
            actor: ff_rdp_core::ActorId::from(format!("conn0/netEvent{resource_id}")),
            method: "GET".to_owned(),
            url: url.to_owned(),
            is_xhr: false,
            cause_type: "document".to_owned(),
            started_date_time: "2026-01-01T00:00:00Z".to_owned(),
            timestamp: 0.0,
            resource_id,
        }
    }

    fn status_update(resource_id: u64, status: &str) -> ff_rdp_core::NetworkResourceUpdate {
        ff_rdp_core::NetworkResourceUpdate {
            resource_id,
            status: Some(status.to_owned()),
            ..Default::default()
        }
    }

    /// The pre-iter-166 shape of `extract_document_status`: build the tracker
    /// and resolve it for a navigation that did not redirect, so the requested
    /// and committed URLs are the same string.
    fn doc_status(
        resources: &[ff_rdp_core::NetworkResource],
        updates: &[ff_rdp_core::NetworkResourceUpdate],
        url: &str,
    ) -> Option<u16> {
        extract_document_status(resources, updates)
            .resolve(url, url)
            .0
    }

    /// `unit_extract_document_status_matches_cause_and_url` — the AC-facing
    /// happy path: one `document`-cause resource matching the requested URL,
    /// with a status update.
    #[test]
    fn unit_extract_document_status_matches_cause_and_url() {
        let resources = vec![doc_resource("https://example.com/404", 1)];
        let updates = vec![status_update(1, "404")];
        assert_eq!(
            doc_status(&resources, &updates, "https://example.com/404"),
            Some(404)
        );
    }

    /// `unit_extract_document_status_ignores_subframe_document_resource` —
    /// iter-138 Theme A hardening: a subframe's own `document`-cause request
    /// (same cause type, different URL) must not be mistaken for the page's
    /// own navigation — the same contamination risk Theme F guards against
    /// for `document-event`.
    #[test]
    fn unit_extract_document_status_ignores_subframe_document_resource() {
        let resources = vec![
            doc_resource("https://cdn.example.com/iframe.html", 1),
            doc_resource("https://example.com/page", 2),
        ];
        let updates = vec![status_update(1, "200"), status_update(2, "503")];
        assert_eq!(
            doc_status(&resources, &updates, "https://example.com/page"),
            Some(503),
            "must report the requested URL's own status, not the subframe's"
        );
    }

    /// `unit_extract_document_status_none_when_no_document_resource_matches`
    /// — no matching resource (e.g. `--no-wait`, or the status update hadn't
    /// arrived yet) reports `None`, which the caller surfaces as JSON `null`.
    #[test]
    fn unit_extract_document_status_none_when_no_document_resource_matches() {
        let resources = vec![doc_resource("https://example.com/other", 1)];
        let updates = vec![status_update(1, "200")];
        assert_eq!(
            doc_status(&resources, &updates, "https://example.com/page"),
            None
        );
        assert_eq!(doc_status(&[], &[], "https://example.com/page"), None);
    }

    /// `unit_extract_document_status_prefers_last_match_on_redirect` — a
    /// redirect chain can produce more than one `document`-cause resource for
    /// the same requested URL; the LAST one (the hop that actually
    /// committed) wins over the first (the original, redirected request).
    #[test]
    fn unit_extract_document_status_prefers_last_match_on_redirect() {
        let resources = vec![
            doc_resource("https://example.com/page", 1),
            doc_resource("https://example.com/page", 2),
        ];
        let updates = vec![status_update(1, "302"), status_update(2, "200")];
        assert_eq!(
            doc_status(&resources, &updates, "https://example.com/page"),
            Some(200)
        );
    }

    /// `unit_extract_document_status_survives_later_update_without_status` —
    /// regression guard for a live-Firefox-only bug (not reproducible against
    /// any mock): `resources-updated-array` entries are incremental partial
    /// updates, and Firefox carries `status` only on the FIRST update for a
    /// resource — a SECOND update (e.g. carrying `totalTime`/`contentSize`)
    /// leaves `status: None`. Naively taking "the single most-recent update
    /// record" instead of "the most recent value seen per field" silently
    /// turned a real 200/404 into `null` the instant a second update arrived
    /// — which real Firefox traffic always produces. Caught by
    /// `live_138_with_network_keeps_envelope`, not by any prior unit test.
    #[test]
    fn unit_extract_document_status_survives_later_update_without_status() {
        let resources = vec![doc_resource("https://example.com/page", 1)];
        let updates = vec![
            status_update(1, "200"),
            ff_rdp_core::NetworkResourceUpdate {
                resource_id: 1,
                total_time: Some(45),
                ..Default::default()
            },
        ];
        assert_eq!(
            doc_status(&resources, &updates, "https://example.com/page"),
            Some(200),
            "a later update that doesn't carry `status` must not erase an \
             earlier one that did"
        );
    }

    // ── iter-166: URL canonicalisation and `status_reason` ──────────────────

    /// `unit_166_matches_document_across_url_canonicalisation` — the iteration's
    /// whole defect in one assertion. A caller types `https://example.com`;
    /// Firefox requests the canonical `https://example.com/` and reports that
    /// URL on the `network-event`. The pre-iter-166 exact-string comparison
    /// therefore matched nothing and `navigate` reported `status: null` for a
    /// page that had plainly returned 200 — measured live before the fix (see
    /// the plan's Theme A section) on all three routes.
    #[test]
    fn unit_166_matches_document_across_url_canonicalisation() {
        let resources = vec![doc_resource("https://example.com/", 1)];
        let updates = vec![status_update(1, "200")];
        assert_eq!(
            doc_status(&resources, &updates, "https://example.com"),
            Some(200),
            "a missing trailing slash must not hide the document's status"
        );
        // The reverse direction, and a fragment (never sent to the server, so
        // it can never appear on a `network-event`), match too.
        assert_eq!(
            doc_status(&resources, &updates, "https://example.com/#top"),
            Some(200)
        );
        // A differing path still must not match — canonicalisation is not
        // permission to be sloppy.
        assert_eq!(
            doc_status(&resources, &updates, "https://example.com/other"),
            None
        );
    }

    /// `unit_166_prefers_the_committed_url_over_the_requested_one` — on a
    /// cross-scheme redirect the requested URL never appears among the
    /// document resources; the URL that committed does, and its status is the
    /// one the caller ended up with.
    #[test]
    fn unit_166_prefers_the_committed_url_over_the_requested_one() {
        let resources = vec![
            doc_resource("http://example.com/", 1),
            doc_resource("https://example.com/", 2),
        ];
        let updates = vec![status_update(1, "301"), status_update(2, "200")];
        let tracker = extract_document_status(&resources, &updates);
        assert_eq!(
            tracker.resolve("http://example.com", "https://example.com/"),
            (Some(200), None),
            "the committed document's status wins over the redirect hop's"
        );
    }

    /// `unit_166_status_null_is_distinguishable` — the AC. A `null` status now
    /// always arrives with a `status_reason` naming which of the three
    /// situations produced it, so a caller can tell "the server sent no
    /// status" from "this route never looked". The two fields are mutually
    /// exclusive: `status_reason` is `None` exactly when a status was found.
    #[test]
    fn unit_166_status_null_is_distinguishable() {
        // 1. The route never subscribed to `network-event` (back/forward/
        //    reload, `--no-wait`, the readystate-only wait strategy).
        let blind = DocumentStatusTracker::default();
        assert_eq!(
            blind.resolve("https://example.com/", "https://example.com/"),
            (None, Some(StatusUnknown::NotObserved))
        );

        // 2. Network events were observed, but the committed document issued
        //    no request of its own (`about:blank`, a bfcache restore, a
        //    same-document navigation). A subframe's request is present and is
        //    deliberately NOT borrowed to fill the gap.
        let resources = vec![doc_resource("https://cdn.example.com/frame.html", 1)];
        let updates = vec![status_update(1, "200")];
        assert_eq!(
            extract_document_status(&resources, &updates).resolve("about:blank", "about:blank"),
            (None, Some(StatusUnknown::NoDocumentRequest)),
            "a subframe's status must never be reported as the page's"
        );

        // 3. The document's request was found, but no status was ever
        //    reported for it (response line not yet in, or the channel failed).
        let resources = vec![doc_resource("https://example.com/", 7)];
        assert_eq!(
            extract_document_status(&resources, &[])
                .resolve("https://example.com/", "https://example.com/"),
            (None, Some(StatusUnknown::NoStatusReported))
        );

        // And the success case carries no reason at all.
        let updates = vec![status_update(7, "204")];
        assert_eq!(
            extract_document_status(&resources, &updates)
                .resolve("https://example.com/", "https://example.com/"),
            (Some(204), None)
        );

        // The wire strings are stable — `--jq '.results.status_reason'` is a
        // scripting surface, so these are part of the contract.
        assert_eq!(StatusUnknown::NotObserved.as_str(), "not_observed");
        assert_eq!(
            StatusUnknown::NoDocumentRequest.as_str(),
            "no_document_request"
        );
        assert_eq!(
            StatusUnknown::NoStatusReported.as_str(),
            "no_status_reported"
        );
    }

    /// `unit_166_canonical_doc_url_leaves_unparseable_input_alone` — the
    /// comparison must degrade to the old exact-string behaviour rather than
    /// panic or normalise a non-URL into something that accidentally matches.
    #[test]
    fn unit_166_canonical_doc_url_leaves_unparseable_input_alone() {
        assert_eq!(canonical_doc_url("not a url"), "not a url");
        assert_eq!(canonical_doc_url("about:blank"), "about:blank");
        assert_eq!(
            canonical_doc_url("https://example.com"),
            "https://example.com/"
        );
        assert_eq!(
            canonical_doc_url("https://example.com/a?b=1#c"),
            "https://example.com/a?b=1"
        );
        // The query is deliberately preserved: two same-path requests that
        // differ only in query really are different requests.
        assert_ne!(
            canonical_doc_url("https://example.com/a?b=1"),
            canonical_doc_url("https://example.com/a?b=2")
        );
    }

    // ── iter-138 Theme B/C: probe_same_document_commit ──────────────────────

    /// `unit_probe_same_document_commit_returns_new_href_when_changed_and_complete`
    /// — the core same-document detection: `document.readyState === 'complete'
    /// && location.href !== pre_href` resolving truthy returns the new href.
    #[test]
    fn unit_probe_same_document_commit_returns_new_href_when_changed_and_complete() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let console_actor = "conn0/console1";

        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = std::io::BufReader::new(stream);

            let greeting = serde_json::json!({
                "from": "root", "applicationType": "browser", "traits": {}
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();

            answer_one_eval(
                &mut reader,
                &mut writer,
                console_actor,
                &serde_json::json!("https://example.com/route1"),
            )
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        let console_actor = ff_rdp_core::ActorId::from(console_actor);

        let result =
            probe_same_document_commit(&mut transport, &console_actor, "https://example.com/page");

        let eval_text = server_handle.join().unwrap();
        assert!(
            eval_text.contains("readyState") && eval_text.contains("location.href"),
            "condition must check both readyState and location.href: {eval_text}"
        );
        assert_eq!(result, Some("https://example.com/route1".to_owned()));
    }

    /// `unit_probe_same_document_commit_returns_none_when_href_unchanged` — a
    /// `null` result (the IIFE's own signal for "not yet") is treated as "not
    /// a same-document commit", not an error.
    #[test]
    fn unit_probe_same_document_commit_returns_none_when_href_unchanged() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let console_actor = "conn0/console1";

        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = std::io::BufReader::new(stream);

            let greeting = serde_json::json!({
                "from": "root", "applicationType": "browser", "traits": {}
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();

            answer_one_eval(
                &mut reader,
                &mut writer,
                console_actor,
                &serde_json::Value::Null,
            )
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        let console_actor = ff_rdp_core::ActorId::from(console_actor);

        let result =
            probe_same_document_commit(&mut transport, &console_actor, "https://example.com/page");
        server_handle.join().unwrap();
        assert_eq!(result, None);
    }

    /// `unit_probe_same_document_commit_empty_pre_href_returns_none` — an
    /// empty baseline (the pre-navigation `location.href` eval itself failed)
    /// disables the check entirely without attempting an eval round-trip.
    #[test]
    fn unit_probe_same_document_commit_empty_pre_href_returns_none() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        // The server sends only the greeting `connect()` requires, then never
        // answers anything else — if the function attempted an eval
        // round-trip despite the empty `pre_href`, this would block until
        // the transport's read timeout fires (proving the assertion below
        // wrong: the fast path must return well inside it).
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream;
            let greeting = serde_json::json!({
                "from": "root", "applicationType": "browser", "traits": {}
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        transport
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let console_actor = ff_rdp_core::ActorId::from("conn0/console1");

        let started = Instant::now();
        let result = probe_same_document_commit(&mut transport, &console_actor, "");
        let elapsed = started.elapsed();

        assert_eq!(result, None);
        assert!(
            elapsed < Duration::from_millis(40),
            "empty pre_href must return immediately without an eval round-trip, took {elapsed:?}"
        );
        drop(transport);
        let _ = server_handle.join();
    }

    /// iter-130 Theme A: `unit_navigate_dom_complete_literal_about_blank_falls_back_to_href`
    ///
    /// Reproduces the comparis.ch SPA route-commit finding (dogfooding-session-61
    /// #5): the `dom-complete` document-event's `url` field is the literal string
    /// `"about:blank"` even though the real requested URL has genuinely landed
    /// (`ready_state: complete`, and a manual `eval location.href` confirms it).
    /// `committed_url` must be resolved from `location.href`, not surfaced as a
    /// literal `about:blank` that would make a caller think navigation failed.
    #[test]
    fn unit_navigate_dom_complete_literal_about_blank_falls_back_to_href() {
        use std::io::Write as _;
        use std::net::TcpListener;

        use ff_rdp_core::transport::{RdpTransport, encode_frame};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let console_actor = "conn0/console1";
        let tab_actor = "conn0/tabDescriptor1";

        let server_handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = std::io::BufReader::new(stream);

            let greeting = serde_json::json!({
                "from": "root", "applicationType": "browser", "traits": {}
            });
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();

            answer_get_target(&mut reader, &mut writer, tab_actor, console_actor);

            answer_one_eval(
                &mut reader,
                &mut writer,
                console_actor,
                &serde_json::json!("https://www.comparis.ch/hypotheken"),
            )
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        // dom-loading commits with the initial "about:blank" placeholder, then
        // dom-complete fires — still reporting the literal "about:blank" — even
        // though the real page has landed (this is the exact shape observed on
        // comparis.ch route commits).
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<Resource>>();
        tx.send(std::sync::Arc::new(Resource::DocumentEvent(
            serde_json::json!({ "name": "dom-loading", "url": "about:blank" }),
        )))
        .unwrap();
        tx.send(std::sync::Arc::new(Resource::DocumentEvent(
            serde_json::json!({ "name": "dom-complete", "url": "about:blank" }),
        )))
        .unwrap();
        drop(tx);

        let watcher_actor = ff_rdp_core::ActorId::from("conn0/watcher1");
        let bus_arc = Arc::new(Mutex::new(ResourceCommand::new(watcher_actor)));
        let tab = ff_rdp_core::ActorId::from(tab_actor);

        let nav_start = Instant::now();
        let mut probe = ReadyStateProbe {
            console_actor: Some(ff_rdp_core::ActorId::from("conn0/stale-console")),
            tab_actor: &tab,
            pre_epoch: Some(1.0),
            // Push the probe far into the future — only the dom-complete
            // fallback path should fire, not the interleaved probe.
            first_probe_at: nav_start + Duration::from_secs(30),
            probe_interval: Duration::from_secs(30),
            poll_enabled: true,
            pre_href: String::new(),
            trust_event_url: true,
        };

        let result = wait_for_doc_complete(
            &mut transport,
            &bus_arc,
            &rx,
            5_000,
            WaitLevel::Complete,
            nav_start,
            Some(&mut probe),
            "https://www.comparis.ch/hypotheken",
            // These mock-transport tests exercise `navigate`'s route, which
            // subscribes to `NetworkEvent` (iter-166).
            true,
        );

        server_handle.join().unwrap();

        let ci = result.expect("dom-complete should resolve to a CommitInfo");
        assert_eq!(ci.ready_state, "complete");
        assert_eq!(
            ci.committed_url, "https://www.comparis.ch/hypotheken",
            "a literal about:blank dom-complete URL must fall back to location.href \
             when it does not match the requested URL"
        );
    }
}

#[cfg(test)]
mod blank_shortcut_tests {
    use super::*;
    use ff_rdp_core::transport::{encode_frame, recv_from};
    use std::io::{BufReader, Write};
    use std::net::{TcpListener, TcpStream};

    fn send(stream: &mut TcpStream, value: &Value) {
        stream
            .write_all(encode_frame(&value.to_string()).as_bytes())
            .unwrap();
    }

    #[test]
    fn unit_277_actual_wait_shortcut_matrix() {
        for (baseline, sampled, expected, requested) in [
            (
                "https://old.test/",
                "about:blank",
                "https://destination.test/",
                "https://destination.test/",
            ),
            (
                "about:blank",
                "about:blank",
                "https://destination.test/",
                "https://destination.test/",
            ),
            (
                "https://destination.test/",
                "https://destination.test/#fragment",
                "https://destination.test/#fragment",
                "https://destination.test/",
            ),
            (
                "https://destination.test/old",
                "https://destination.test/route",
                "https://destination.test/route",
                "https://destination.test/",
            ),
            (
                "https://old.test/",
                "about:blank",
                "about:blank",
                "about:blank",
            ),
            ("https://old.test/", "about:blank", "about:blank", ""),
            (
                "https://old.test/",
                "about:blank",
                "about:blank",
                "ABOUT:blank",
            ),
            (
                "https://old.test/",
                "about:blank",
                "https://destination.test/",
                "ABOUT:Blank",
            ),
        ] {
            let observing = requested.starts_with("https:");
            let (tx, rx) = std::sync::mpsc::channel();
            tx.send(Arc::new(Resource::NetworkEvent(
                ff_rdp_core::NetworkResource {
                    actor: "network".into(),
                    method: "GET".into(),
                    url: "https://destination.test/".into(),
                    is_xhr: false,
                    cause_type: "document".into(),
                    started_date_time: String::new(),
                    timestamp: 0.0,
                    resource_id: 7,
                },
            )))
            .unwrap();
            tx.send(Arc::new(Resource::NetworkUpdate(
                ff_rdp_core::NetworkResourceUpdate {
                    resource_id: 7,
                    status: Some("200".into()),
                    ..Default::default()
                },
            )))
            .unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                send(
                    &mut stream,
                    &json!({"from":"root", "applicationType":"browser"}),
                );
                let mut actions = 0;
                let mut shortcut_samples = 0;
                while let Ok(request) = recv_from(&mut reader) {
                    match request["type"].as_str().unwrap() {
                        "navigateTo" => {
                            actions += 1;
                            assert_eq!(actions, 1, "navigation must never be repeated");
                            assert_eq!(request["url"], "https://destination.test/");
                            send(&mut stream, &json!({"from":"target"}));
                        }
                        "getTarget" => {
                            assert_eq!(request["to"], "tab");
                            send(
                                &mut stream,
                                &json!({"from":"tab", "frame":{"actor":"target", "consoleActor":"console"}}),
                            );
                        }
                        "evaluateJSAsync" => {
                            assert_eq!(request["to"], "console");
                            let text = request["text"].as_str().unwrap();
                            let value = if text.contains("var h = window.location.href") {
                                shortcut_samples += 1;
                                assert_eq!(shortcut_samples, 1);
                                assert!(text.contains(&serde_json::to_string(baseline).unwrap()));
                                // Script the result of the complete-and-changed JS condition.
                                // This discriminates the caller, not Firefox's JS implementation.
                                if sampled == baseline {
                                    Value::Null
                                } else {
                                    json!(sampled)
                                }
                            } else {
                                assert!(text == READINESS_SAMPLE, "unexpected eval: {text}");
                                scripted_readiness(42.0, sampled, "complete")
                            };
                            send(&mut stream, &json!({"from":"console", "resultID":"r"}));
                            send(
                                &mut stream,
                                &json!({"from":"console", "type":"evaluationResult", "resultID":"r", "result":value}),
                            );
                            if text.contains("var h = window.location.href") {
                                for name in ["dom-loading", "dom-complete"] {
                                    tx.send(Arc::new(Resource::DocumentEvent(
                                        json!({"name":name,"url":"https://destination.test/"}),
                                    )))
                                    .unwrap();
                                }
                            }
                        }
                        other => panic!("unexpected request: {other}"),
                    }
                }
                (actions, shortcut_samples)
            });
            let mut transport =
                RdpTransport::connect("127.0.0.1", port, Duration::from_secs(2)).unwrap();
            WindowGlobalTarget::navigate_to(
                &mut transport,
                &"target".into(),
                "https://destination.test/",
            )
            .unwrap();
            let tab = "tab".into();
            let start = Instant::now();
            let mut probe = ReadyStateProbe {
                console_actor: Some("console".into()),
                tab_actor: &tab,
                pre_epoch: Some(42.0),
                first_probe_at: start,
                probe_interval: Duration::from_secs(1),
                poll_enabled: true,
                pre_href: baseline.into(),
                trust_event_url: true,
            };
            let bus = Arc::new(Mutex::new(ResourceCommand::new("watcher".into())));
            let result = wait_for_doc_complete(
                &mut transport,
                &bus,
                &rx,
                1500,
                WaitLevel::Complete,
                start,
                Some(&mut probe),
                requested,
                observing,
            );
            drop(transport);
            assert_eq!(server.join().unwrap(), (1, 1));
            let result = result.unwrap();
            eprintln!(
                "277 scripted baseline={baseline} sample={sampled} commit={} status={:?}",
                result.committed_url, result.http_status
            );
            assert_eq!(result.committed_url, expected);
            assert_eq!(result.ready_state, "complete");
            assert_eq!(result.http_status, observing.then_some(200));
            assert_eq!(
                result.status_reason,
                (!observing).then_some(StatusUnknown::NotObserved)
            );
        }
    }
}

#[cfg(test)]
mod atomic_readiness_tests {
    use super::*;
    use crate::commands::connect_tab::ConnectedTab;
    use ff_rdp_core::transport::recv_from;
    use std::io::{BufReader, Write};
    use std::net::{TcpListener, TcpStream};

    fn send(stream: &mut TcpStream, value: &Value) {
        let body = serde_json::to_vec(value).unwrap();
        write!(stream, "{}:", body.len()).unwrap();
        stream.write_all(&body).unwrap();
    }

    #[test]
    fn direct_poll_preserves_zero_budget_errors_and_deadline() {
        for case in [
            "ready",
            "not-ready",
            "malformed",
            "exception",
            "actor-error",
            "timeout",
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                send(
                    &mut stream,
                    &json!({"from":"root","applicationType":"browser"}),
                );
                let request = recv_from(&mut reader).unwrap();
                assert_eq!(request["to"], "console");
                assert_eq!(request["type"], "evaluateJSAsync");
                assert_eq!(request["text"], READINESS_SAMPLE);
                if case == "timeout" {
                    // No reply: the caller's positive absolute deadline must
                    // override the much longer connection timeout.
                    std::thread::sleep(Duration::from_millis(150));
                } else if case == "actor-error" {
                    send(
                        &mut stream,
                        &json!({"from":"console","error":"wrongState","message":"terminal"}),
                    );
                } else {
                    send(&mut stream, &json!({"from":"console","resultID":"r"}));
                    let value = if case == "malformed" {
                        json!("not JSON")
                    } else {
                        scripted_readiness(
                            43.0,
                            "https://new.test/",
                            if case == "not-ready" {
                                "loading"
                            } else {
                                "complete"
                            },
                        )
                    };
                    let mut reply = json!({"from":"console","type":"evaluationResult","resultID":"r","result":value});
                    if case == "exception" {
                        reply["exception"] = json!("\u{1b}[31munsafe\u{1b}[0m");
                    }
                    send(&mut stream, &reply);
                }
                assert!(
                    recv_from(&mut reader).is_err(),
                    "one evaluation; no follow-up href request"
                );
            });
            let transport =
                RdpTransport::connect("127.0.0.1", port, Duration::from_secs(2)).unwrap();
            let mut ctx = ConnectedTab::for_test(transport, "console".into());
            let start = Instant::now();
            let result = wait_for_readystate_complete(
                &mut ctx,
                if case == "timeout" { 50 } else { 0 },
                ReadinessCheck {
                    pre_epoch: Some(42.0),
                    pre_href: "https://old.test/",
                    requested_url: "https://new.test/",
                },
                start,
            );
            let elapsed = start.elapsed();
            drop(ctx);
            server.join().unwrap();
            match case {
                "ready" => assert_eq!(result.unwrap().committed_url, "https://new.test/"),
                "exception" => assert!(
                    matches!(result, Err(AppError::User(ref text)) if text.contains("unsafe") && !text.contains('\u{1b}'))
                ),
                "actor-error" => assert!(
                    matches!(result, Err(AppError::User(ref text)) if text.contains("wrongState"))
                ),
                _ => assert!(
                    matches!(result, Err(AppError::Timeout(ref text)) if text.contains("document.readyState")),
                    "{case}: {result:?}"
                ),
            }
            assert!(elapsed < Duration::from_secs(1), "{case}: {elapsed:?}");
        }
    }

    #[test]
    fn baseline_requires_successful_positive_epoch_and_href() {
        for (value, exceptional, expected) in [
            (json!(42), false, Some(42.0)),
            (json!(0), false, None),
            (json!(-1), false, None),
            (json!("42"), false, None),
            (Value::Null, false, None),
            (json!({"type":"NaN"}), false, None),
            (json!({"type":"Infinity"}), false, None),
            (json!(42), true, None),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                send(
                    &mut stream,
                    &json!({"from":"root","applicationType":"browser"}),
                );
                for (js, value) in [
                    ("performance.timing.navigationStart", value),
                    ("window.location.href", json!("https://old.test/")),
                ] {
                    let request = recv_from(&mut reader).unwrap();
                    assert_eq!(request["text"], js);
                    send(&mut stream, &json!({"from":"console","resultID":"r"}));
                    let mut reply = json!({"from":"console","type":"evaluationResult","resultID":"r","result":value});
                    if exceptional {
                        reply["exception"] = json!("unavailable");
                    }
                    send(&mut stream, &reply);
                }
            });
            let transport =
                RdpTransport::connect("127.0.0.1", port, Duration::from_secs(1)).unwrap();
            let mut ctx = ConnectedTab::for_test(transport, "console".into());
            assert_eq!(capture_pre_nav_epoch(&mut ctx, "test epoch"), expected);
            assert_eq!(
                eval_location_href(ctx.transport_mut(), &"console".into()),
                if exceptional { "" } else { "https://old.test/" }
            );
            drop(ctx);
            server.join().unwrap();
        }
    }
}
