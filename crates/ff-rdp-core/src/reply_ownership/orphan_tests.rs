//! Quarantine is completion accounting, never cancellation or a timeout policy.
use super::*;
use serde_json::json;

#[test]
fn departed_ordinary_is_discarded_while_distinct_actor_reply_remains_live() {
    let mut ledger = ReplyAccounting::default();
    ledger.register("old", ReplyContract::Ordinary).unwrap();
    assert_eq!(ledger.quarantine_current(), 1);
    ledger.register("new", ReplyContract::Ordinary).unwrap();
    let status = ledger.snapshot();
    assert_eq!(status["pending"], 2);
    assert_eq!(status["active_pending"], 1);
    assert_eq!(status["orphan_pending_ordinary"], 1);
    assert_eq!(status["quarantined_actor_count"], 1);
    assert_eq!(
        ledger.receive(&json!({"from":"old","type":"tabDetached"})),
        Ok(ReplyDisposition::Event)
    );
    assert_eq!(ledger.pending(), 2, "lifecycle does not complete a reply");
    assert_eq!(
        ledger.receive(&json!({"from":"old","frames":["old"]})),
        Ok(ReplyDisposition::Discard)
    );
    assert_eq!(ledger.snapshot()["orphan_pending"], 0);
    assert_eq!(ledger.active_pending(), 1);
    assert_eq!(
        ledger.receive(&json!({"from":"new","frames":["new"]})),
        Ok(ReplyDisposition::Reply)
    );
    assert_eq!(ledger.pending(), 0);
    // Only an actual protocol completion permits actor reuse; no FIFO across owners.
    ledger.register("old", ReplyContract::Ordinary).unwrap();
    assert_eq!(
        ledger.receive(&json!({"from":"old","fresh":true})),
        Ok(ReplyDisposition::Reply)
    );
    assert_eq!(ledger.snapshot()["discarded_replies"], 1);
    assert_eq!(ledger.reason(), None);
}

#[test]
fn every_send_contract_refuses_quarantined_actor_without_new_reservation() {
    for contract in [
        ReplyContract::Ordinary,
        ReplyContract::AsyncEvaluation,
        ReplyContract::OneWay,
    ] {
        let mut ledger = ReplyAccounting::default();
        ledger.register("old", ReplyContract::Ordinary).unwrap();
        ledger.quarantine_current();
        assert_eq!(ledger.register("old", contract), Err("orphan_actor_reused"));
        assert_eq!(ledger.pending(), 1);
        assert_eq!(ledger.active_pending(), 0);
        assert_eq!(ledger.snapshot()["orphan_pending_ordinary"], 1);
    }
}

#[test]
fn orphan_async_ack_and_exact_result_are_both_owned_until_completion() {
    for ack_before_departure in [false, true] {
        let mut ledger = ReplyAccounting::default();
        ledger
            .register("console", ReplyContract::AsyncEvaluation)
            .unwrap();
        let ack = json!({"from":"console","resultID":"old-result"});
        if ack_before_departure {
            assert_eq!(ledger.receive(&ack), Ok(ReplyDisposition::Reply));
        }
        assert_eq!(ledger.quarantine_current(), 1);
        ledger
            .register("new-console", ReplyContract::AsyncEvaluation)
            .unwrap();
        if !ack_before_departure {
            assert_eq!(ledger.receive(&ack), Ok(ReplyDisposition::Discard));
        }
        assert_eq!(
            ledger.snapshot()["orphan_pending_async"],
            1,
            "ACK cannot forgive result debt"
        );
        assert_eq!(
            ledger.receive(&json!({"from":"new-console","resultID":"old-result"})),
            Ok(ReplyDisposition::Reply)
        );
        assert_eq!(ledger.receive(&json!({"from":"console","type":"evaluationResult","resultID":"old-result","exception":"old"})), Ok(ReplyDisposition::Discard));
        assert_eq!(ledger.active_pending(), 1);
        assert_eq!(ledger.receive(&json!({"from":"new-console","type":"evaluationResult","resultID":"old-result","result":2})), Ok(ReplyDisposition::Reply));
        assert_eq!(ledger.pending(), 0);
        assert_eq!(
            ledger.snapshot()["discarded_replies"],
            if ack_before_departure { 1 } else { 2 }
        );
    }
}

#[test]
fn orphan_errors_complete_only_unambiguous_contracts() {
    for contract in [ReplyContract::Ordinary, ReplyContract::AsyncEvaluation] {
        let mut ledger = ReplyAccounting::default();
        ledger.register("old", contract).unwrap();
        ledger.quarantine_current();
        assert_eq!(
            ledger.receive(&json!({"from":"old","error":"noSuchActor"})),
            Ok(ReplyDisposition::Discard)
        );
        assert_eq!(ledger.pending(), 0);
        assert_eq!(ledger.reason(), None);
    }
    for hazard in [false, true] {
        let mut ledger = ReplyAccounting::default();
        ledger.register("old", ReplyContract::Ordinary).unwrap();
        ledger
            .register(
                "old",
                if hazard {
                    ReplyContract::OneWay
                } else {
                    ReplyContract::AsyncEvaluation
                },
            )
            .unwrap();
        ledger.quarantine_current();
        let debt = ledger.pending();
        assert_eq!(
            ledger.receive(&json!({"from":"old","error":"noSuchActor"})),
            Err(if hazard {
                "ambiguous_oneway_error"
            } else {
                "ambiguous_error_contract"
            })
        );
        assert_eq!(ledger.pending(), debt, "ambiguity must not discharge debt");
    }
}

#[test]
fn orphan_unknown_result_retires_without_guessing_or_forgiving() {
    for acknowledged in [false, true] {
        let mut ledger = ReplyAccounting::default();
        ledger
            .register("old", ReplyContract::AsyncEvaluation)
            .unwrap();
        if acknowledged {
            ledger
                .receive(&json!({"from":"old","resultID":"expected"}))
                .unwrap();
        }
        ledger.quarantine_current();
        assert_eq!(
            ledger.receive(&json!({"from":"old","type":"evaluationResult","resultID":"unknown"})),
            Err("unowned_evaluation_result")
        );
        assert_eq!(ledger.pending(), 1);
        assert_eq!(ledger.snapshot()["orphan_pending_async"], 1);
    }
}

#[test]
fn orphan_oneway_hazard_stays_quarantined_after_ordinary_completion() {
    let mut ledger = ReplyAccounting::default();
    ledger.register("old", ReplyContract::OneWay).unwrap();
    ledger.register("old", ReplyContract::Ordinary).unwrap();
    ledger.quarantine_current();
    assert_eq!(
        ledger.receive(&json!({"from":"old","ok":true})),
        Ok(ReplyDisposition::Discard)
    );
    assert_eq!(ledger.pending(), 0);
    assert_eq!(ledger.snapshot()["quarantined_actor_count"], 1);
    assert_eq!(ledger.snapshot()["oneway_hazard_actors"], 1);
    ledger.terminal("released").unwrap();
    let completed = ledger.snapshot();
    assert_eq!(completed["terminal_actor_count"], 1);
    assert_eq!(ledger.active_pending(), 0);
    assert_eq!(ledger.quarantine_current(), 0);
    assert_eq!(
        ledger.snapshot(),
        completed,
        "zero-debt departure must retain sticky orphan, hazard and terminal sinks"
    );
    assert_eq!(
        ledger.receive(&json!({"from":"released","late":true})),
        Ok(ReplyDisposition::Discard),
        "zero-debt departure must not remove the terminal reply sink"
    );
    assert_eq!(ledger.reason(), None);
    assert_eq!(
        ledger.register("old", ReplyContract::Ordinary),
        Err("orphan_actor_reused")
    );
    let retired = ledger.snapshot();
    assert_eq!(ledger.active_pending(), 0);
    assert_eq!(ledger.quarantine_current(), 0);
    assert_eq!(ledger.reason(), Some("orphan_actor_reused"));
    assert_eq!(
        ledger.snapshot(),
        retired,
        "departure cannot clear retirement"
    );
}

#[test]
fn multiple_departures_keep_all_orphans_and_capacity_is_not_eviction() {
    let mut ledger = ReplyAccounting::default();
    for generation in 0..3 {
        ledger
            .register(&format!("old-{generation}"), ReplyContract::Ordinary)
            .unwrap();
        assert_eq!(
            ledger.quarantine_current(),
            1,
            "old debt must not count as a new departure"
        );
    }
    assert_eq!(ledger.quarantine_current(), 0);
    assert_eq!(ledger.pending(), 3);
    assert_eq!(ledger.active_pending(), 0);
    for n in 3..ReplyAccounting::LIMIT {
        ledger
            .register(&format!("old-{n}"), ReplyContract::Ordinary)
            .unwrap();
    }
    assert_eq!(ledger.quarantine_current(), ReplyAccounting::LIMIT - 3);
    assert_eq!(
        ledger.snapshot()["quarantined_actor_count"],
        ReplyAccounting::LIMIT
    );
    assert_eq!(
        ledger.register("extra", ReplyContract::Ordinary),
        Err("ownership_capacity_exceeded")
    );
    assert_eq!(ledger.pending(), ReplyAccounting::LIMIT);
    assert_eq!(ledger.active_pending(), 0);
    assert_eq!(ledger.actors.len(), ReplyAccounting::LIMIT);
}

#[test]
fn orphan_debt_blocks_recovery_and_terminal_release() {
    let mut recovery = ReplyAccounting::default();
    recovery.register("old", ReplyContract::Ordinary).unwrap();
    recovery.quarantine_current();
    assert_eq!(
        recovery.reserve_recovery("watcher"),
        Err("recovery_conflicts_with_request")
    );
    assert_eq!(recovery.pending(), 1);
    for asynchronous in [false, true] {
        let mut release = ReplyAccounting::default();
        release
            .register(
                "old",
                if asynchronous {
                    ReplyContract::AsyncEvaluation
                } else {
                    ReplyContract::Ordinary
                },
            )
            .unwrap();
        if asynchronous {
            release
                .receive(&json!({"from":"old","resultID":"r"}))
                .unwrap();
        }
        release.quarantine_current();
        assert_eq!(
            release.terminal("old"),
            Err(if asynchronous {
                "release_conflicts_with_async"
            } else {
                "orphan_actor_reused"
            })
        );
        assert_eq!(release.pending(), 1);
    }
}

#[test]
fn partially_completed_orphan_actor_cannot_be_reassigned() {
    let mut ledger = ReplyAccounting::default();
    ledger.register("old", ReplyContract::Ordinary).unwrap();
    ledger.register("old", ReplyContract::Ordinary).unwrap();
    assert_eq!(ledger.quarantine_current(), 2);
    assert_eq!(
        ledger.receive(&json!({"from":"old","first":true})),
        Ok(ReplyDisposition::Discard)
    );
    assert_eq!(ledger.snapshot()["orphan_pending_ordinary"], 1);
    assert_eq!(ledger.snapshot()["quarantined_actor_count"], 1);
    assert_eq!(
        ledger.register("old", ReplyContract::Ordinary),
        Err("orphan_actor_reused")
    );
    assert_eq!(ledger.pending(), 1);
}

#[test]
fn departed_oneway_only_hazard_cannot_complete_successor_ordinary_error() {
    let mut ledger = ReplyAccounting::default();
    ledger.register("watcher", ReplyContract::OneWay).unwrap();
    assert_eq!(
        ledger.quarantine_current(),
        0,
        "one-way has no promised completion"
    );
    assert_eq!(ledger.snapshot()["oneway_hazard_actors"], 1);
    // There is no abandoned ordinary/async reply on this actor, but the error
    // hazard must survive handover and must not pay the successor's new debt.
    ledger.register("watcher", ReplyContract::Ordinary).unwrap();
    assert_eq!(
        ledger.receive(&json!({"from":"watcher","error":"noSuchActor"})),
        Err("ambiguous_oneway_error")
    );
    assert_eq!(ledger.active_pending(), 1);
    assert_eq!(ledger.pending(), 1);
}
