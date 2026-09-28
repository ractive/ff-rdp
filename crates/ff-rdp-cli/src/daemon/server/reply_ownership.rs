//! Per-origin ownership boundary. Never reassign unfinished work to a new slot.
use super::{
    Arc, ClientId, ClientWriter, DaemonInfo, EventSource, FirefoxWriter, HashMap, Mutex,
    ProtocolError, Result, SharedState, StopReason, TcpStream, Value, json, registry,
    send_to_recorded_client,
};
use ff_rdp_core::{ConnectionKey, ReplyAccounting, ReplyContract, ReplyDisposition};

pub(super) struct Origin {
    pub(super) socket: Mutex<Option<TcpStream>>,
    pub(super) ledger: Mutex<Ledger>,
    pub(super) writer: Mutex<Option<FirefoxWriter>>,
}
#[derive(Default)]
pub(super) struct Ledger {
    pub(super) accounting: ReplyAccounting,
    owner: Option<(ClientId, ClientWriter)>,
    departures: u64,
}
impl Default for Origin {
    fn default() -> Self {
        Self {
            socket: Mutex::new(None),
            ledger: Mutex::new(Ledger::default()),
            writer: Mutex::new(None),
        }
    }
}
pub(super) fn origins() -> Mutex<HashMap<EventSource, Arc<Origin>>> {
    Mutex::new(HashMap::from([(
        EventSource::Primary,
        Arc::new(Origin::default()),
    )]))
}
impl EventSource {
    pub(super) fn key(self) -> ConnectionKey {
        ConnectionKey(match self {
            Self::Primary => 0,
            Self::Lazy(g) => g,
        })
    }
    fn from_key(key: ConnectionKey) -> Self {
        if key.0 == 0 {
            Self::Primary
        } else {
            Self::Lazy(key.0)
        }
    }
}
impl Ledger {
    pub(super) fn register(
        &mut self,
        id: ClientId,
        writer: &ClientWriter,
        message: &Value,
        contract: ReplyContract,
    ) -> Result<(), &'static str> {
        if self.owner.as_ref().is_some_and(|(old, _)| *old != id)
            && self.accounting.active_pending() != 0
        {
            self.accounting.retire("owner_changed_with_outstanding");
        }
        self.accounting
            .register(message["to"].as_str().unwrap_or_default(), contract)?;
        self.owner = Some((id, writer.clone()));
        Ok(())
    }
    pub(super) fn depart(&mut self, id: ClientId) -> Option<&'static str> {
        if self.owner.as_ref().is_some_and(|(old, _)| *old == id) {
            self.owner = None;
            // Never let a new owner inherit this client's debt. Actor quarantine
            // preserves every obligation while permitting disjoint actor sends.
            // Old orphans do not inflate this client's departure count.
            if self.accounting.quarantine_current() != 0 {
                self.departures = self.departures.saturating_add(1);
            }
        }
        self.accounting.reason()
    }
    pub(super) fn snapshot(&self) -> Value {
        let mut value = self.accounting.snapshot();
        value["departures_with_outstanding"] = self.departures.into();
        value
    }
}
impl SharedState {
    pub(super) fn origin(&self, source: EventSource) -> Option<Arc<Origin>> {
        lock_or_recover!(self.reply_origins).get(&source).cloned()
    }
    pub(super) fn retire_origin(&self, source: EventSource, reason: &'static str) {
        tracing::warn!(?source, reason, "daemon: reply ownership retired origin");
        if let Some(origin) = self.origin(source) {
            lock_or_recover!(origin.ledger).accounting.retire(reason);
        }
        // Publish the primary reason before the registered socket/writer wakes.
        // An interrupted reader must not win with FirefoxConnectionLost. The
        // ledger guard is gone before cancellation or lazy-origin interruption.
        match source {
            EventSource::Primary => self.stop(StopReason::ReplyOwnership),
            EventSource::Lazy(generation) => self.invalidate_lazy(generation),
        }
    }
    pub(super) fn ownership_health(&self) -> Value {
        self.origin(EventSource::Primary)
            .map_or(Value::Null, |origin| {
                lock_or_recover!(origin.ledger).snapshot()
            })
    }
    pub(super) fn send_owned(
        &self,
        source: EventSource,
        packet: &Value,
        terminal: bool,
    ) -> Result<(), ProtocolError> {
        let error = |s: &str| ProtocolError::InvalidPacket(s.into());
        let Some(origin) = self.origin(source) else {
            return Err(error("origin_connection_retired"));
        };
        let writer = lock_or_recover!(origin.writer).clone();
        let Some(writer) = writer else {
            self.retire_origin(source, "origin_writer_missing");
            return Err(error("origin_writer_missing"));
        };
        let Some(mut lease) = writer.acquire_cancellable() else {
            return Err(error("origin_connection_retired"));
        };
        let result = {
            let mut ledger = lock_or_recover!(origin.ledger);
            let actor = packet["to"].as_str().unwrap_or_default();
            if terminal {
                ledger.accounting.terminal(actor)
            } else {
                ledger.accounting.register(actor, ReplyContract::OneWay)
            }
        };
        if let Err(reason) = result {
            drop(lease);
            self.retire_origin(source, reason);
            return Err(error(reason));
        }
        let result = if self.is_stopping() || !self.source_active(source) {
            Err(error("origin_connection_retired"))
        } else {
            lease.send(packet)
        };
        drop(lease);
        if result.is_err() {
            self.retire_origin(source, "firefox_write_uncertain");
        }
        result
    }
    pub(super) fn release_on_origin(&self, request: &ff_rdp_core::ReleaseRequest) {
        let Some(key) = request.origin else {
            tracing::error!("daemon release has no connection origin; refused");
            return;
        };
        let source = EventSource::from_key(key);
        if !self.source_active(source) || self.origin(source).is_none() {
            tracing::debug!("daemon release origin retired; connection owns reclamation");
            return;
        }
        let packet = json!({"to":request.actor_id.as_ref(),"type":request.method});
        if let Err(error) = self.send_owned(source, &packet, true) {
            tracing::warn!(%error,"daemon release refused");
        }
    }
}

/// True consumes a reply/sink/error; false leaves normal event dispatch intact.
pub(super) fn route(state: &SharedState, source: EventSource, message: &Value) -> bool {
    let Some(origin) = state.origin(source) else {
        if message.get("type").is_some() && message["type"] != "evaluationResult" {
            return false;
        }
        state.retire_origin(source, "reply_origin_missing");
        return true;
    };
    let (disposition, owner) = {
        let mut ledger = lock_or_recover!(origin.ledger);
        let disposition = ledger.accounting.receive(message);
        // `Discard` has already consumed an orphan completion under this lock.
        // Capture a live recipient here, never by rereading the later RPC slot.
        (disposition, ledger.owner.clone())
    };
    match disposition {
        Ok(ReplyDisposition::Event) => false,
        Ok(ReplyDisposition::Discard) => true,
        Ok(ReplyDisposition::Reply) => {
            if let Some((id, writer)) = owner {
                send_to_recorded_client(state, message, id, &writer);
            }
            // Synchronous setup owns its own replies; it never becomes a client.
            true
        }
        Err(reason) => {
            state.retire_origin(source, reason);
            true
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Forward {
    to: String,
    #[serde(rename = "type")]
    kind: String,
    reply_contract: ReplyContract,
    packet: Value,
}
pub(super) fn unwrap(message: Value) -> Result<(Value, Option<ReplyContract>), &'static str> {
    if message["to"] == "daemon" && message["type"] != "forward" {
        return Ok((message, None));
    }
    let forward: Forward =
        serde_json::from_value(message).map_err(|_| "proxy_request_requires_reply_contract")?;
    if forward.to != "daemon"
        || forward.kind != "forward"
        || !forward.packet.is_object()
        || forward.packet["to"]
            .as_str()
            .is_none_or(|to| to.is_empty() || to == "daemon")
        || forward.packet["type"].as_str().is_none_or(str::is_empty)
        || forward.packet.get("auth").is_some()
    {
        return Err("invalid_forward_packet");
    }
    Ok((forward.packet, Some(forward.reply_contract)))
}

/// Declared before WorkerOwner: unwind joins workers before this diagnostic write.
pub(super) struct Finalizer {
    state: Arc<SharedState>,
    info: DaemonInfo,
    finished: bool,
    directory: std::path::PathBuf,
}
impl Finalizer {
    pub(super) fn new(state: Arc<SharedState>, info: DaemonInfo) -> Result<Self> {
        Ok(Self {
            directory: registry::registry_dir()?,
            state,
            info,
            finished: false,
        })
    }
    #[cfg(test)]
    pub(super) fn in_directory(
        state: Arc<SharedState>,
        info: DaemonInfo,
        directory: std::path::PathBuf,
    ) -> Self {
        Self {
            state,
            info,
            directory,
            finished: false,
        }
    }
    pub(super) fn finish(&mut self) -> Result<()> {
        self.finished = true;
        let mut snapshot = self.state.ownership_health();
        snapshot["shutdown_reason"] = json!(format!("{:?}", self.state.cancellation.reason()));
        let written = registry::write_ownership_receipt_in(&self.directory, &self.info, &snapshot)
            .map_err(|error| {
                eprintln!("daemon: ownership receipt persistence failed: {error:#}");
                error
            })?;
        if !written {
            tracing::info!("ownership receipt superseded by newer session");
        }
        Ok(())
    }
}
impl Drop for Finalizer {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish();
        }
    }
}
