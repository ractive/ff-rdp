use std::collections::HashMap;

use serde_json::Value;

const MAX_EVENTS_PER_TYPE: usize = 10_000;

/// A ring-buffer-style event store keyed by resource type.
///
/// Each resource type has an independent cap of `MAX_EVENTS_PER_TYPE` entries.
/// When the cap is reached the oldest event is evicted before inserting the new one,
/// so the buffer never grows beyond the limit.
pub(crate) struct EventBuffer {
    inner: HashMap<String, Vec<Value>>,
}

impl EventBuffer {
    pub(crate) fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    /// Insert an event for `resource_type`.  If the bucket is already at
    /// `MAX_EVENTS_PER_TYPE` the oldest entry is removed first.
    pub(crate) fn insert(&mut self, resource_type: &str, event: Value) {
        let bucket = self.inner.entry(resource_type.to_owned()).or_default();
        if bucket.len() >= MAX_EVENTS_PER_TYPE {
            bucket.remove(0);
        }
        bucket.push(event);
    }

    /// Drain all events for `resource_type` and return them.
    ///
    /// The bucket is left empty (but still present in the map).  Returns an
    /// empty `Vec` if the type is unknown.
    pub(crate) fn drain(&mut self, resource_type: &str) -> Vec<Value> {
        match self.inner.get_mut(resource_type) {
            Some(bucket) => std::mem::take(bucket),
            None => Vec::new(),
        }
    }

    /// Return the number of buffered events per resource type, omitting empty
    /// buckets.
    pub(crate) fn sizes(&self) -> HashMap<String, usize> {
        self.inner
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| (k.clone(), v.len()))
            .collect()
    }

    /// Returns `true` when every bucket is empty (or no buckets exist).
    #[allow(dead_code)]
    pub(crate) fn is_empty(&self) -> bool {
        self.inner.values().all(Vec::is_empty)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn ev(n: u64) -> Value {
        json!({ "seq": n })
    }

    #[test]
    fn new_buffer_is_empty() {
        let buf = EventBuffer::new();
        assert!(buf.is_empty());
        assert!(buf.sizes().is_empty());
    }

    #[test]
    fn insert_and_drain_roundtrip() {
        let mut buf = EventBuffer::new();
        buf.insert("network", ev(1));
        buf.insert("network", ev(2));
        buf.insert("css", ev(3));

        assert!(!buf.is_empty());

        let net = buf.drain("network");
        assert_eq!(net, vec![ev(1), ev(2)]);

        // After draining network, only css remains.
        assert!(!buf.is_empty());

        let css = buf.drain("css");
        assert_eq!(css, vec![ev(3)]);

        assert!(buf.is_empty());
    }

    #[test]
    fn drain_clears_bucket() {
        let mut buf = EventBuffer::new();
        buf.insert("network", ev(1));
        let first = buf.drain("network");
        assert_eq!(first.len(), 1);

        // Draining again must return empty.
        let second = buf.drain("network");
        assert!(second.is_empty());
    }

    #[test]
    fn drain_unknown_type_returns_empty() {
        let mut buf = EventBuffer::new();
        let result = buf.drain("nonexistent");
        assert!(result.is_empty());
    }

    #[test]
    fn eviction_at_cap() {
        let mut buf = EventBuffer::new();

        // Fill to exactly the cap.
        for i in 0..MAX_EVENTS_PER_TYPE {
            buf.insert("t", ev(i as u64));
        }

        let sizes = buf.sizes();
        assert_eq!(sizes["t"], MAX_EVENTS_PER_TYPE);

        // Insert one more — oldest (seq=0) must be gone.
        buf.insert("t", ev(MAX_EVENTS_PER_TYPE as u64));

        let events = buf.drain("t");
        assert_eq!(events.len(), MAX_EVENTS_PER_TYPE);
        assert_eq!(events[0], ev(1), "oldest event should have been evicted");
        assert_eq!(
            events[MAX_EVENTS_PER_TYPE - 1],
            ev(MAX_EVENTS_PER_TYPE as u64),
            "newest event should be last"
        );
    }

    #[test]
    fn sizes_only_includes_non_empty() {
        let mut buf = EventBuffer::new();
        buf.insert("a", ev(1));
        buf.insert("b", ev(2));
        buf.insert("b", ev(3));

        let sizes = buf.sizes();
        assert_eq!(sizes.len(), 2);
        assert_eq!(sizes["a"], 1);
        assert_eq!(sizes["b"], 2);

        // After draining "a" it must disappear from sizes.
        buf.drain("a");
        let sizes2 = buf.sizes();
        assert!(!sizes2.contains_key("a"));
        assert_eq!(sizes2["b"], 2);
    }

    #[test]
    fn is_empty_after_all_drained() {
        let mut buf = EventBuffer::new();
        buf.insert("x", ev(0));
        assert!(!buf.is_empty());
        buf.drain("x");
        assert!(buf.is_empty());
    }
}
