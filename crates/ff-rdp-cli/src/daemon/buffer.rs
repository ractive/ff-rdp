use std::collections::{HashMap, VecDeque};

use ff_rdp_core::{ConsoleResource, NetworkResource, NetworkResourceUpdate, Resource};
use serde_json::{Value, json};

const MAX_EVENTS: usize = 50_000;
const MAX_BOUNDARIES: usize = 1_000;
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
pub(crate) struct ResourceBuffer {
    store: VecDeque<Entry>,
    boundaries: Vec<NavBoundary>,
    next_nav_sequence: u64,
    total_inserted: u64,
}

impl ResourceBuffer {
    pub(crate) fn new() -> Self {
        Self {
            store: VecDeque::new(),
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
                self.store.retain(|e| {
                    !(e.resource_type == *resource_type
                        && e.resource_id.as_deref() == Some(resource_id.as_str()))
                });
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
            self.store.pop_front();
        }
        let seq = self.total_inserted;
        self.total_inserted = self.total_inserted.saturating_add(1);
        self.store.push_back(Entry {
            resource_type: resource_type.to_owned(),
            resource_id,
            data,
            seq,
        });
    }

    /// Insert a raw JSON value (for `store-events` IPC back-compat).
    pub(crate) fn insert_raw(&mut self, resource_type: &str, data: Value) {
        let resource_id = data.get("resourceId").map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        });
        self.push(resource_type, resource_id, data);
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
            url.chars().take(MAX_NAV_URL_LEN).collect()
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
        for entry in self.store.drain(..) {
            if entry.resource_type == resource_type && entry.seq >= min_seq {
                results.push(entry.data);
            } else {
                remaining.push_back(entry);
            }
        }
        self.store = remaining;
        (results, boundary)
    }

    pub(crate) fn drain(&mut self, resource_type: &str) -> Vec<Value> {
        self.drain_since(resource_type, 0).0
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
}
