use std::collections::{HashMap, VecDeque};

use ff_rdp_core::{ConsoleResource, NetworkResource, NetworkResourceUpdate, Resource};
use serde_json::{Value, json};

const MAX_EVENTS: usize = 50_000;
const MAX_BOUNDARIES: usize = 1_000;

/// Per-type reserved floor (iter-101 Theme C).
///
/// When the buffer is at its global [`MAX_EVENTS`] capacity and a new entry is
/// pushed, eviction is **type-aware**: the oldest entry belonging to a type
/// that is *above* this floor is dropped first, so a burst of one resource type
/// (e.g. thousands of `network-event`s) can never evict the last
/// [`TYPE_RESERVED_FLOOR`] entries of another type (e.g. `console-message` /
/// `error-message`).
///
/// The floor is a soft reservation, not a hard per-type cap: a single type may
/// still grow to fill the whole buffer when no other type has entries.  The
/// guarantee is only that the *oldest* `TYPE_RESERVED_FLOOR` entries of any type
/// that currently has at most that many entries are protected from cross-type
/// eviction.
const TYPE_RESERVED_FLOOR: usize = 500;
/// Maximum byte length of a navigation URL stored in a [`NavBoundary`].
///
/// URLs longer than this are silently truncated on insert to bound memory usage
/// and prevent very long URLs from being echoed back to operators in logs.
const MAX_NAV_URL_LEN: usize = 4096;

/// A navigation boundary recorded when `tabNavigated` fires.
#[derive(Debug, Clone)]
pub(crate) struct NavBoundary {
    pub sequence: u64,
    pub url: String,
    /// Insertion sequence number of the first store entry belonging to this
    /// navigation.  Entries with `seq >= store_start` belong to this epoch.
    pub store_start: u64,
}

struct Entry {
    resource_type: String,
    resource_id: Option<String>,
    data: Value,
    /// Monotonically-increasing insertion sequence number.
    ///
    /// Compared against [`NavBoundary::store_start`] to determine whether an
    /// entry belongs to a particular navigation epoch without being confused by
    /// Destroyed-pruning removing entries from the middle of the queue.
    seq: u64,
}

/// Single-queue resource buffer populated from the `ResourceCommand` bus.
///
/// The backing store is one insertion-ordered [`VecDeque`], but eviction is
/// **type-aware** (iter-101 Theme C): `type_counts` tracks how many live
/// entries each resource type holds so that overflow eviction can protect the
/// [`TYPE_RESERVED_FLOOR`] oldest entries of each type from being purged by a
/// burst of another type.
pub(crate) struct ResourceBuffer {
    store: VecDeque<Entry>,
    /// Live entry count per resource type, kept in sync with `store` on every
    /// push, evict, drain, and Destroyed-prune.  Used by [`Self::evict_one`] to
    /// pick a victim type that is above its reserved floor.
    type_counts: HashMap<String, usize>,
    boundaries: Vec<NavBoundary>,
    next_nav_sequence: u64,
    total_inserted: u64,
}

impl ResourceBuffer {
    pub(crate) fn new() -> Self {
        Self {
            store: VecDeque::new(),
            type_counts: HashMap::new(),
            boundaries: Vec::new(),
            next_nav_sequence: 0,
            total_inserted: 0,
        }
    }

    /// Ingest a typed resource; `Destroyed` prunes, all others append.
    pub(crate) fn on_resource(&mut self, r: &Resource) {
        match r {
            Resource::Destroyed {
                resource_type,
                resource_id,
            } => {
                // Retain all entries that do NOT match (resource_type, resource_id).
                // A single resource_id may have multiple buffered entries (e.g.
                // an initial network-event + subsequent network-event updates),
                // so we remove ALL of them to avoid stale data.
                let mut removed = 0usize;
                self.store.retain(|e| {
                    let matches = e.resource_type == *resource_type
                        && e.resource_id.as_deref() == Some(resource_id.as_str());
                    if matches {
                        removed += 1;
                    }
                    !matches
                });
                if removed > 0 {
                    Self::dec_type_count(&mut self.type_counts, resource_type, removed);
                }
            }
            Resource::NetworkEvent(n) => self.push(
                "network-event",
                Some(n.resource_id.to_string()),
                net_to_val(n),
            ),
            Resource::NetworkUpdate(u) => self.push(
                "network-event",
                Some(u.resource_id.to_string()),
                update_to_val(u),
            ),
            Resource::ConsoleMessage(c) => {
                let rid = c.resource_id.map(|id| id.to_string());
                self.push("console-message", rid, console_to_val(c));
            }
            Resource::ErrorMessage(c) => {
                let rid = c.resource_id.map(|id| id.to_string());
                self.push("error-message", rid, console_to_val(c));
            }
            Resource::DocumentEvent(v) => self.push("document-event", None, v.clone()),
        }
    }

    fn push(&mut self, resource_type: &str, resource_id: Option<String>, data: Value) {
        if self.store.len() >= MAX_EVENTS {
            self.evict_one(resource_type);
        }
        let seq = self.total_inserted;
        self.total_inserted = self.total_inserted.saturating_add(1);
        *self
            .type_counts
            .entry(resource_type.to_owned())
            .or_insert(0) += 1;
        self.store.push_back(Entry {
            resource_type: resource_type.to_owned(),
            resource_id,
            data,
            seq,
        });
    }

    /// Evict exactly one entry to make room for a new `incoming_type` push,
    /// respecting the per-type reserved floor (iter-101 Theme C).
    ///
    /// Victim selection walks the store from oldest to newest and drops the
    /// first entry whose type currently holds **more than** [`TYPE_RESERVED_FLOOR`]
    /// entries — so a type sitting at or below its floor is never chosen.  This
    /// guarantees that a burst of one type cannot evict the oldest
    /// `TYPE_RESERVED_FLOOR` entries of another type.
    ///
    /// Fallbacks (all types at/below their floor, which only happens once the
    /// number of *distinct* types times the floor exceeds [`MAX_EVENTS`]):
    /// evict the oldest entry of `incoming_type` if it has any, otherwise the
    /// globally-oldest entry.  Either way exactly one entry is removed so the
    /// caller's subsequent push cannot exceed [`MAX_EVENTS`].
    fn evict_one(&mut self, incoming_type: &str) {
        // Preferred: oldest entry of a type that is above its reserved floor.
        let victim_idx = self.store.iter().position(|e| {
            self.type_counts.get(&e.resource_type).copied().unwrap_or(0) > TYPE_RESERVED_FLOOR
        });

        let idx = victim_idx.or_else(|| {
            // Fallback 1: oldest entry of the incoming type (the type we are
            // about to grow), so we prefer to cannibalise our own type.
            self.store
                .iter()
                .position(|e| e.resource_type == incoming_type)
        });

        // Fallback 2: globally-oldest entry (front of the deque).
        let idx = idx.unwrap_or(0);

        if let Some(entry) = self.store.remove(idx) {
            Self::dec_type_count(&mut self.type_counts, &entry.resource_type, 1);
        }
    }

    /// Decrement the live count for `resource_type` by `n`, removing the map
    /// entry when it reaches zero so `type_counts` never accumulates stale
    /// zero-valued keys.
    fn dec_type_count(counts: &mut HashMap<String, usize>, resource_type: &str, n: usize) {
        if let Some(c) = counts.get_mut(resource_type) {
            *c = c.saturating_sub(n);
            if *c == 0 {
                counts.remove(resource_type);
            }
        }
    }

    /// Insert a raw JSON value (for `store-events` IPC back-compat).
    pub(crate) fn insert_raw(&mut self, resource_type: &str, data: Value) {
        let resource_id = data.get("resourceId").map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        });
        self.push(resource_type, resource_id, data);
    }

    /// URL of the most recent recorded navigation boundary, if any.
    ///
    /// Used by the daemon to compare against the URL on an incoming
    /// `tabNavigated` event so it can emit a warning when the redirect
    /// crossed a scheme boundary (iter-75 E / Hg-8).
    pub(crate) fn last_nav_url(&self) -> Option<&str> {
        self.boundaries.last().map(|b| b.url.as_str())
    }

    /// Record a navigation boundary.  Returns the assigned sequence number.
    pub(crate) fn record_nav_boundary(&mut self, url: String) -> u64 {
        let sequence = self.next_nav_sequence;
        self.next_nav_sequence = self.next_nav_sequence.saturating_add(1);
        if self.boundaries.len() >= MAX_BOUNDARIES {
            self.boundaries.remove(0);
        }
        // Truncate the URL to bound memory usage and prevent very long URLs
        // from being stored in the boundary log.
        let url = if url.len() > MAX_NAV_URL_LEN {
            // Cut at a UTF-8 char boundary at or before MAX_NAV_URL_LEN bytes
            // so the byte-length bound is actually honored even for non-ASCII URLs.
            let mut end = MAX_NAV_URL_LEN;
            while end > 0 && !url.is_char_boundary(end) {
                end -= 1;
            }
            url[..end].to_owned()
        } else {
            url
        };
        // `store_start` is the insertion sequence number of the *next* entry
        // to be pushed.  Entries with `seq >= store_start` belong to this
        // navigation epoch or later.
        self.boundaries.push(NavBoundary {
            sequence,
            url,
            store_start: self.total_inserted,
        });
        sequence
    }

    /// Drain entries for `resource_type`, optionally filtered by nav boundary.
    ///
    /// When `since_nav_index != 0`, only entries whose insertion sequence number
    /// (`seq`) is >= the boundary's `store_start` are included.  Because `seq`
    /// is assigned at push-time and is never affected by Destroyed-pruning or
    /// prior drains, the comparison is always correct regardless of how many
    /// entries have been removed in the interim.
    pub(crate) fn drain_since(
        &mut self,
        resource_type: &str,
        since_nav_index: i64,
    ) -> (Vec<Value>, Option<NavBoundary>) {
        let boundary = resolve_boundary(&self.boundaries, since_nav_index);
        let min_seq: u64 = boundary.as_ref().map_or(0, |b| b.store_start);

        let mut results = Vec::new();
        let mut remaining = VecDeque::new();
        let mut drained_of_type = 0usize;
        for entry in self.store.drain(..) {
            if entry.resource_type == resource_type && entry.seq >= min_seq {
                drained_of_type += 1;
                results.push(entry.data);
            } else {
                remaining.push_back(entry);
            }
        }
        self.store = remaining;
        if drained_of_type > 0 {
            Self::dec_type_count(&mut self.type_counts, resource_type, drained_of_type);
        }
        (results, boundary)
    }

    pub(crate) fn drain(&mut self, resource_type: &str) -> Vec<Value> {
        self.drain_since(resource_type, 0).0
    }

    /// Purge every entry that was inserted **before** the current insertion
    /// point, i.e. drop all currently-buffered resource entries (iter-101
    /// Theme A).
    ///
    /// Called when a **top-level, cross-process target switch** is observed
    /// (`target-destroyed-form` for the old top-level target): the resources
    /// buffered for the destroyed document are stale and must never be mixed
    /// into a post-switch drain window.  Nav boundaries are left intact — the
    /// switch does not reset navigation-scope bookkeeping, and `store_start`
    /// values remain valid because `total_inserted` is never rewound.
    ///
    /// Returns the number of entries purged (for logging / test assertions).
    pub(crate) fn purge_destroyed_target(&mut self) -> usize {
        let purged = self.store.len();
        self.store.clear();
        self.type_counts.clear();
        purged
    }

    pub(crate) fn sizes(&self) -> HashMap<String, usize> {
        let mut map = HashMap::new();
        for e in &self.store {
            *map.entry(e.resource_type.clone()).or_insert(0) += 1;
        }
        map
    }
}

fn resolve_boundary(boundaries: &[NavBoundary], since_nav_index: i64) -> Option<NavBoundary> {
    if since_nav_index == 0 || boundaries.is_empty() {
        return None;
    }
    let n = boundaries.len();
    let idx = if since_nav_index < 0 {
        n.checked_sub(usize::try_from(-since_nav_index).unwrap_or(usize::MAX))?
    } else {
        let i = usize::try_from(since_nav_index).unwrap_or(usize::MAX);
        i.checked_sub(1).filter(|&i| i < n)?
    };
    boundaries.get(idx).cloned()
}

fn net_to_val(n: &NetworkResource) -> Value {
    json!({
        "actor": n.actor.as_ref(), "resourceId": n.resource_id,
        "method": n.method, "url": n.url, "isXHR": n.is_xhr,
        "causeType": n.cause_type, "startedDateTime": n.started_date_time,
        "timeStamp": n.timestamp,
    })
}

fn update_to_val(u: &NetworkResourceUpdate) -> Value {
    // Build the update object inline using Value::Object insertions.
    let mut m = serde_json::Map::new();
    m.insert("resourceId".into(), json!(u.resource_id));
    let opt_str = [
        ("status", u.status.as_deref()),
        ("httpVersion", u.http_version.as_deref()),
        ("mimeType", u.mime_type.as_deref()),
        ("remoteAddress", u.remote_address.as_deref()),
        ("securityState", u.security_state.as_deref()),
    ];
    for (k, v) in opt_str {
        if let Some(v) = v {
            m.insert(k.into(), json!(v));
        }
    }
    let opt_u64 = [
        ("totalTime", u.total_time),
        ("contentSize", u.content_size),
        ("transferredSize", u.transferred_size),
    ];
    for (k, v) in opt_u64 {
        if let Some(v) = v {
            m.insert(k.into(), json!(v));
        }
    }
    if let Some(v) = u.from_cache {
        m.insert("fromCache".into(), json!(v));
    }
    json!({ "resourceUpdates": [Value::Object(m)] })
}

fn console_to_val(c: &ConsoleResource) -> Value {
    let mut v = json!({
        "level": c.level, "message": c.message, "source": c.source,
        "lineNumber": c.line, "columnNumber": c.column, "timeStamp": c.timestamp,
    });
    if let Some(rid) = c.resource_id {
        v["resourceId"] = json!(rid);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use ff_rdp_core::ActorId;

    fn net(id: u64, url: &str) -> Resource {
        Resource::NetworkEvent(NetworkResource {
            actor: ActorId::from("conn0/n1"),
            method: "GET".into(),
            url: url.into(),
            is_xhr: false,
            cause_type: "document".into(),
            started_date_time: "2026-01-01T00:00:00Z".into(),
            timestamp: 0.0,
            resource_id: id,
        })
    }

    #[test]
    fn append_and_drain() {
        let mut buf = ResourceBuffer::new();
        buf.on_resource(&net(1, "https://a.com"));
        buf.on_resource(&net(2, "https://b.com"));
        let events = buf.drain("network-event");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["url"], "https://a.com");
        assert_eq!(events[1]["url"], "https://b.com");
        assert!(buf.drain("network-event").is_empty());
    }

    #[test]
    fn destroyed_prunes_all_matching_entries() {
        // Two entries with the same resource_id (initial + update); both must
        // be removed by a single Destroyed event.
        let mut buf = ResourceBuffer::new();
        buf.on_resource(&net(1, "https://a.com"));
        // Push a second entry with the same resource_id via insert_raw.
        buf.insert_raw(
            "network-event",
            json!({"resourceId": "1", "url": "https://a.com/update"}),
        );
        buf.on_resource(&net(2, "https://b.com"));
        buf.on_resource(&Resource::Destroyed {
            resource_type: "network-event".into(),
            resource_id: "1".into(),
        });
        let events = buf.drain("network-event");
        assert_eq!(
            events.len(),
            1,
            "both entries for resource_id=1 must be removed"
        );
        assert_eq!(events[0]["url"], "https://b.com");
    }

    #[test]
    fn drain_since_filters_by_nav_boundary() {
        let mut buf = ResourceBuffer::new();
        buf.on_resource(&net(1, "https://before.com"));
        buf.record_nav_boundary("https://after.com".into());
        buf.on_resource(&net(2, "https://after.com/page"));
        let (events, boundary) = buf.drain_since("network-event", -1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["url"], "https://after.com/page");
        assert!(boundary.is_some());
    }

    #[test]
    fn drain_since_zero_returns_all() {
        let mut buf = ResourceBuffer::new();
        buf.on_resource(&net(1, "https://a.com"));
        buf.record_nav_boundary("https://b.com".into());
        buf.on_resource(&net(2, "https://b.com/page"));
        let (events, boundary) = buf.drain_since("network-event", 0);
        assert_eq!(events.len(), 2);
        assert!(boundary.is_none());
    }

    #[test]
    fn drain_since_correct_after_destroyed_removals() {
        // Insert 3 events, destroy the first, then drain_since the boundary
        // that was recorded after the first insert.  Only the post-boundary
        // surviving entry (id=3) should be returned.
        let mut buf = ResourceBuffer::new();
        buf.on_resource(&net(1, "https://before.com"));
        buf.record_nav_boundary("https://nav.com".into());
        buf.on_resource(&net(2, "https://nav.com/a"));
        buf.on_resource(&net(3, "https://nav.com/b"));
        // Destroy id=2: this shifts VecDeque positions but must not affect
        // the seq-based boundary calculation.
        buf.on_resource(&Resource::Destroyed {
            resource_type: "network-event".into(),
            resource_id: "2".into(),
        });
        let (events, boundary) = buf.drain_since("network-event", -1);
        assert!(boundary.is_some());
        assert_eq!(
            events.len(),
            1,
            "only post-boundary surviving entry expected"
        );
        assert_eq!(events[0]["url"], "https://nav.com/b");
    }

    #[test]
    fn insert_raw_back_compat() {
        let mut buf = ResourceBuffer::new();
        buf.insert_raw(
            "network-event",
            json!({"resourceId": 99, "url": "https://x.com"}),
        );
        let events = buf.drain("network-event");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["url"], "https://x.com");
    }

    fn console(id: u64, msg: &str) -> Resource {
        Resource::ConsoleMessage(ConsoleResource {
            level: "log".into(),
            message: msg.into(),
            source: "console-api".into(),
            line: 0,
            column: 0,
            timestamp: 0.0,
            resource_id: Some(id),
        })
    }

    /// AC: `unit_buffer_eviction_per_type` — flooding the buffer to overflow
    /// with `network-event`s must NOT evict pre-existing `console-message`
    /// entries that sit within the per-type reserved floor.
    #[test]
    fn unit_buffer_eviction_per_type() {
        let mut buf = ResourceBuffer::new();

        // Seed a modest number of console messages (well under the floor).
        let console_count: usize = 50;
        for i in 0..console_count {
            buf.on_resource(&console(i as u64, &format!("hello-{i}")));
        }

        // Flood with 10× MAX_EVENTS network events to force heavy eviction.
        for i in 0..(MAX_EVENTS as u64 * 10) {
            buf.on_resource(&net(1_000_000 + i, "https://flood.example/"));
        }

        // The store is capped at MAX_EVENTS.
        assert!(
            buf.store.len() <= MAX_EVENTS,
            "store must never exceed MAX_EVENTS; got {}",
            buf.store.len()
        );

        // All seeded console messages must still be drainable: they are below
        // the reserved floor, so the network flood could not evict them.
        let drained = buf.drain("console-message");
        assert_eq!(
            drained.len(),
            console_count,
            "all {console_count} console messages must survive a network flood"
        );
        // And they must be the original messages, in order.
        assert_eq!(drained[0]["message"], "hello-0");
        assert_eq!(
            drained[console_count - 1]["message"],
            format!("hello-{}", console_count - 1)
        );
    }

    /// The reserved floor is a *soft* reservation: a single type may still fill
    /// the whole buffer when it is the only type present.
    #[test]
    fn single_type_can_fill_buffer() {
        let mut buf = ResourceBuffer::new();
        for i in 0..(MAX_EVENTS as u64 + 100) {
            buf.on_resource(&net(i, "https://a.example/"));
        }
        assert_eq!(buf.store.len(), MAX_EVENTS, "single type fills to the cap");
        assert_eq!(
            buf.type_counts.get("network-event").copied(),
            Some(MAX_EVENTS),
            "type_counts must stay in sync with the store"
        );
    }

    /// `purge_destroyed_target` drops all buffered entries and resets the
    /// per-type counts, but leaves nav-boundary bookkeeping intact.
    #[test]
    fn purge_destroyed_target_clears_entries_keeps_boundaries() {
        let mut buf = ResourceBuffer::new();
        buf.on_resource(&net(1, "https://old.example/"));
        buf.on_resource(&console(1, "old-log"));
        let seq = buf.record_nav_boundary("https://old.example/".into());
        buf.on_resource(&net(2, "https://old.example/asset"));

        let purged = buf.purge_destroyed_target();
        assert_eq!(purged, 3, "all three buffered entries must be purged");
        assert!(buf.store.is_empty(), "store must be empty after purge");
        assert!(buf.type_counts.is_empty(), "type_counts must be cleared");

        // Boundaries survive so `--since` bookkeeping remains valid.
        assert_eq!(buf.boundaries.len(), 1);
        assert_eq!(buf.boundaries[0].sequence, seq);

        // New entries after the purge are drainable and carry monotonic seqs.
        buf.on_resource(&net(3, "https://new.example/"));
        let drained = buf.drain("network-event");
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0]["url"], "https://new.example/");
    }

    /// Theme I (iter-61x): `record_nav_boundary` truncates URLs longer than
    /// `MAX_NAV_URL_LEN` bytes, and the truncated value is:
    /// - at most `MAX_NAV_URL_LEN` bytes long, and
    /// - valid UTF-8 (i.e. `std::str::from_utf8` round-trips without error).
    ///
    /// The URL is constructed from 1-byte ASCII chars plus a multi-byte Unicode
    /// code point at the end to exercise the char-boundary walk.
    #[test]
    fn test_nav_boundary_url_truncated() {
        let mut buf = ResourceBuffer::new();

        // Build a URL that is exactly MAX_NAV_URL_LEN + 256 bytes when encoded.
        // End with a 4-byte code point (U+1F600 GRINNING FACE = 0xF0 0x9F 0x98 0x80)
        // so the boundary walk has to step back past a multi-byte sequence.
        let base: String = "a".repeat(MAX_NAV_URL_LEN + 252);
        let url = format!("{base}\u{1F600}"); // base + 4 UTF-8 bytes = MAX_NAV_URL_LEN + 256

        buf.record_nav_boundary(url);

        let stored_url = buf
            .boundaries
            .last()
            .expect("boundary must have been recorded")
            .url
            .clone();

        assert!(
            stored_url.len() <= MAX_NAV_URL_LEN,
            "stored URL must be <= MAX_NAV_URL_LEN bytes, got {}",
            stored_url.len()
        );
        assert!(
            std::str::from_utf8(stored_url.as_bytes()).is_ok(),
            "stored URL must be valid UTF-8"
        );
    }
}
