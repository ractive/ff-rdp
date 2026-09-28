//! Reply accounting shared by synchronous setup and its daemon continuation.
//! It never assumes reply ordering across owners or expires ambiguous work.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Contract of an authenticated internal proxy send; direct RDP is unchanged.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplyContract {
    Ordinary,
    AsyncEvaluation,
    OneWay,
}

/// Session-local connection identity carried by deferred grip releases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ConnectionKey(pub u64);

#[derive(Default, Debug)]
struct Actor {
    ordinary: usize,
    acknowledgements: usize,
    hazard: bool,
    terminal: bool,
    // All obligations on this actor belong to departed clients. New sends are
    // forbidden until every recorded completion arrives (hazards remain sticky).
    orphan: bool,
}

/// Bounded accounting transferred at the daemon's synchronous transport split.
/// This type describes protocol completion, not acceptance by an application.
/// In daemon continuation, every non-orphan obligation belongs to its single
/// active RPC owner; quarantined actors are disjoint from every later owner.
#[derive(Default, Debug)]
pub struct ReplyAccounting {
    actors: HashMap<String, Actor>,
    results: HashSet<(String, String)>,
    pending: usize,
    reason: Option<&'static str>,
    discarded: u64,
    ambiguous: u64,
}

/// Whether a packet completes recorded work, is an event, or is consumed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyDisposition {
    Reply,
    Event,
    Discard,
}

impl ReplyAccounting {
    const LIMIT: usize = 4096;
    /// The first reason this origin became unsafe for subsequent sends.
    pub fn reason(&self) -> Option<&'static str> {
        self.reason
    }
    /// Unfinished ordinary replies and async acknowledgements/results.
    pub fn pending(&self) -> usize {
        self.pending
    }
    /// Obligations still owned by the single active client (or synchronous setup).
    /// Earlier departed clients' unanswered work remains in `pending()`.
    pub fn active_pending(&self) -> usize {
        if self.pending == 0 {
            return 0;
        }
        self.pending - self.orphan_counts().0
    }
    /// Quarantine the departing owner's unfinished actors without completing work.
    /// The daemon calls this while holding the same lock used for admission and
    /// reply routing. Previously orphaned actors remain orphaned; no owner queue
    /// or cross-owner ordering assumption is needed because admission is disjoint.
    pub fn quarantine_current(&mut self) -> usize {
        // Zero obligations need no scan; sticky hazard and terminal records stay.
        if self.pending == 0 {
            return 0;
        }
        let newly_orphaned = self.active_pending();
        for entry in self.actors.values_mut() {
            if entry.ordinary != 0 || entry.acknowledgements != 0 {
                entry.orphan = true;
            }
        }
        for (actor, _) in &self.results {
            if let Some(entry) = self.actors.get_mut(actor) {
                entry.orphan = true;
            }
        }
        newly_orphaned
    }
    fn orphan_counts(&self) -> (usize, usize, usize) {
        let ordinary = self
            .actors
            .values()
            .filter(|a| a.orphan)
            .map(|a| a.ordinary)
            .sum::<usize>();
        let asynchronous = self
            .actors
            .values()
            .filter(|a| a.orphan)
            .map(|a| a.acknowledgements)
            .sum::<usize>()
            + self
                .results
                .iter()
                .filter(|(actor, _)| self.actors.get(actor).is_some_and(|a| a.orphan))
                .count();
        (ordinary + asynchronous, ordinary, asynchronous)
    }
    /// Permanently close admission on this origin.
    pub fn retire(&mut self, reason: &'static str) {
        self.reason.get_or_insert(reason);
    }
    fn fail<T>(&mut self, reason: &'static str) -> Result<T, &'static str> {
        self.ambiguous = self.ambiguous.saturating_add(1);
        self.retire(reason);
        Err(reason)
    }
    fn actor(&mut self, actor: &str) -> Result<&mut Actor, &'static str> {
        if let Some(reason) = self.reason {
            return Err(reason);
        }
        if actor.is_empty() {
            return self.fail("missing_actor");
        }
        if !self.actors.contains_key(actor) && self.actors.len() >= Self::LIMIT {
            return self.fail("ownership_capacity_exceeded");
        }
        if self.actors.get(actor).is_some_and(|entry| entry.orphan) {
            return self.fail("orphan_actor_reused");
        }
        Ok(self.actors.entry(actor.to_owned()).or_default())
    }
    /// Reserve before the first byte of the associated send.
    pub fn register(&mut self, actor: &str, contract: ReplyContract) -> Result<(), &'static str> {
        if contract != ReplyContract::OneWay && self.pending >= Self::LIMIT {
            return self.fail("ownership_capacity_exceeded");
        }
        let entry = self.actor(actor)?;
        if entry.terminal {
            return self.fail("terminal_actor_reused");
        }
        match contract {
            ReplyContract::OneWay => entry.hazard = true,
            ReplyContract::Ordinary => {
                entry.ordinary += 1;
                self.pending += 1;
            }
            ReplyContract::AsyncEvaluation => {
                entry.acknowledgements += 1;
                self.pending += 1;
            }
        }
        Ok(())
    }
    /// Install the terminal sink before release; an ACK is never awaited.
    pub fn terminal(&mut self, actor: &str) -> Result<(), &'static str> {
        if self.results.iter().any(|(a, _)| a == actor) {
            return self.fail("release_conflicts_with_async");
        }
        let entry = self.actor(actor)?;
        if entry.ordinary != 0 || entry.acknowledgements != 0 {
            return self.fail("release_conflicts_with_request");
        }
        entry.terminal = true;
        Ok(())
    }
    /// Exclusivity check for the primary startup recovery composite sink.
    pub fn reserve_recovery(&mut self, actor: &str) -> Result<(), &'static str> {
        if self.pending != 0 {
            return self.fail("recovery_conflicts_with_request");
        }
        let entry = self.actor(actor)?;
        if entry.hazard || entry.terminal {
            return self.fail("recovery_reply_ambiguous");
        }
        // Recovery has its own bounded reply sink. Future errors stay hazardous.
        entry.hazard = true;
        Ok(())
    }
    /// Attribute a packet only to this origin's recorded obligations.
    pub fn receive(&mut self, message: &Value) -> Result<ReplyDisposition, &'static str> {
        if self.reason.is_some() {
            self.discarded = self.discarded.saturating_add(1);
            return Ok(ReplyDisposition::Discard);
        }
        let actor = message["from"].as_str().unwrap_or_default();
        if self.actors.get(actor).is_some_and(|a| a.terminal) {
            self.discarded = self.discarded.saturating_add(1);
            return Ok(ReplyDisposition::Discard);
        }
        let orphan = self.actors.get(actor).is_some_and(|entry| entry.orphan);
        if message["type"] == "evaluationResult" {
            let Some(id) = message["resultID"].as_str() else {
                return self.fail("missing_result_id");
            };
            if !self.results.remove(&(actor.to_owned(), id.to_owned())) {
                return self.fail("unowned_evaluation_result");
            }
            self.pending -= 1;
            return Ok(self.accounted_reply(actor, orphan));
        }
        if message.get("type").is_some() {
            return Ok(ReplyDisposition::Event);
        }
        let Some(entry) = self.actors.get_mut(actor) else {
            return self.fail("unexpected_reply");
        };
        let error = message.get("error").is_some();
        if error && entry.hazard {
            return self.fail("ambiguous_oneway_error");
        }
        if error && entry.ordinary != 0 && entry.acknowledgements != 0 {
            return self.fail("ambiguous_error_contract");
        }
        if let Some(id) = message["resultID"].as_str() {
            if entry.acknowledgements == 0
                || !self.results.insert((actor.to_owned(), id.to_owned()))
            {
                return self.fail("unexpected_async_ack");
            }
            entry.acknowledgements -= 1; // ACK becomes an unfinished result.
        } else if entry.ordinary != 0 {
            entry.ordinary -= 1;
            self.pending -= 1;
        } else if error && entry.acknowledgements != 0 {
            entry.acknowledgements -= 1;
            self.pending -= 1;
        } else {
            return self.fail("unexpected_reply");
        }
        Ok(self.accounted_reply(actor, orphan))
    }
    fn accounted_reply(&mut self, actor: &str, orphan: bool) -> ReplyDisposition {
        // Validate and account even discarded replies. An ACK is not a result;
        // ambiguous errors still retire, and events never reach this path.
        self.prune(actor);
        if orphan {
            self.discarded = self.discarded.saturating_add(1);
            ReplyDisposition::Discard
        } else {
            ReplyDisposition::Reply
        }
    }
    fn prune(&mut self, actor: &str) {
        if self
            .actors
            .get(actor)
            .is_some_and(|a| a.ordinary == 0 && a.acknowledgements == 0 && !a.hazard && !a.terminal)
            && !self.results.iter().any(|(a, _)| a == actor)
        {
            self.actors.remove(actor);
        }
    }
    /// Scalar status; no actor IDs or request contents.
    pub fn snapshot(&self) -> Value {
        let (orphan_pending, orphan_ordinary, orphan_async) = self.orphan_counts();
        serde_json::json!({"retiring":self.reason.is_some(),"terminal_reason":self.reason,
            "pending":self.pending,"pending_ordinary":self.actors.values().map(|a|a.ordinary).sum::<usize>(),
            "pending_async":self.results.len()+self.actors.values().map(|a|a.acknowledgements).sum::<usize>(),
            "active_pending":self.pending - orphan_pending,"orphan_pending":orphan_pending,
            "orphan_pending_ordinary":orphan_ordinary,"orphan_pending_async":orphan_async,
            "quarantined_actor_count":self.actors.values().filter(|a|a.orphan).count(),
            "oneway_hazard_actors":self.actors.values().filter(|a|a.hazard).count(),
            "terminal_actor_count":self.actors.values().filter(|a|a.terminal).count(),
            "ambiguous_replies":self.ambiguous,"discarded_replies":self.discarded})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn ordinary_accounting_preserves_interleaved_events() {
        let mut ledger = ReplyAccounting::default();
        ledger.register("root", ReplyContract::Ordinary).unwrap();
        assert_eq!(
            ledger
                .receive(&json!({"from":"watcher","type":"resources-available-array"}))
                .unwrap(),
            ReplyDisposition::Event
        );
        assert_eq!(ledger.pending(), 1);
        assert_eq!(
            ledger.receive(&json!({"from":"root","tabs":[]})).unwrap(),
            ReplyDisposition::Reply
        );
        assert_eq!(ledger.pending(), 0);
    }
    #[test]
    fn async_ack_keeps_completion_owned_and_duplicate_result_retires() {
        let mut ledger = ReplyAccounting::default();
        ledger
            .register("console", ReplyContract::AsyncEvaluation)
            .unwrap();
        ledger
            .receive(&json!({"from":"console","resultID":"r1"}))
            .unwrap();
        assert_eq!(ledger.pending(), 1);
        let result = json!({"from":"console","type":"evaluationResult","resultID":"r1"});
        assert_eq!(ledger.receive(&result).unwrap(), ReplyDisposition::Reply);
        assert_eq!(ledger.pending(), 0);
        assert_eq!(ledger.receive(&result), Err("unowned_evaluation_result"));
    }
    #[test]
    fn one_way_error_never_completes_later_ordinary_request() {
        let mut ledger = ReplyAccounting::default();
        ledger.register("watcher", ReplyContract::OneWay).unwrap();
        ledger.register("watcher", ReplyContract::Ordinary).unwrap();
        assert_eq!(
            ledger.receive(&json!({"from":"watcher","error":"noSuchActor"})),
            Err("ambiguous_oneway_error")
        );
        assert_eq!(ledger.pending(), 1);
        assert!(ledger.reason().is_some());
    }
    #[test]
    fn terminal_release_needs_no_ack_and_never_reopens_actor() {
        let mut ledger = ReplyAccounting::default();
        ledger.terminal("symbol").unwrap();
        assert_eq!(ledger.pending(), 0);
        assert_eq!(
            ledger
                .receive(&json!({"from":"symbol","error":"noSuchActor"}))
                .unwrap(),
            ReplyDisposition::Discard
        );
        assert_eq!(
            ledger.register("symbol", ReplyContract::Ordinary),
            Err("terminal_actor_reused")
        );
    }
    #[test]
    fn terminal_release_cannot_erase_true_inflight_work() {
        let mut ledger = ReplyAccounting::default();
        ledger.register("object", ReplyContract::Ordinary).unwrap();
        assert_eq!(
            ledger.terminal("object"),
            Err("release_conflicts_with_request")
        );
        assert_eq!(ledger.pending(), 1);
    }
    #[test]
    fn unknown_or_pre_ack_result_retires_without_guessing_owner() {
        let mut ledger = ReplyAccounting::default();
        ledger
            .register("console", ReplyContract::AsyncEvaluation)
            .unwrap();
        assert_eq!(
            ledger.receive(&json!({"from":"console","type":"evaluationResult","resultID":"r1"})),
            Err("unowned_evaluation_result")
        );
        assert_eq!(ledger.pending(), 1);
    }
    #[test]
    fn capacity_is_explicit_retirement_without_eviction() {
        let mut ledger = ReplyAccounting::default();
        for n in 0..ReplyAccounting::LIMIT {
            ledger
                .register(&format!("actor{n}"), ReplyContract::OneWay)
                .unwrap();
        }
        assert_eq!(
            ledger.register("extra", ReplyContract::OneWay),
            Err("ownership_capacity_exceeded")
        );
        assert_eq!(ledger.actors.len(), ReplyAccounting::LIMIT);
    }
    #[test]
    fn uncertain_write_keeps_reservation_and_closes_admission() {
        let mut ledger = ReplyAccounting::default();
        ledger.register("root", ReplyContract::Ordinary).unwrap();
        ledger.retire("firefox_write_uncertain");
        assert_eq!(
            ledger.register("root", ReplyContract::Ordinary),
            Err("firefox_write_uncertain")
        );
        assert_eq!(ledger.pending(), 1);
    }
}

#[cfg(test)]
#[path = "reply_ownership/orphan_tests.rs"]
mod orphan_tests;
