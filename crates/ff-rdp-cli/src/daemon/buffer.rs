use std::collections::{HashMap, VecDeque};

use serde_json::Value;

const MAX_EVENTS_PER_TYPE: usize = 10_000;

/// A navigation boundary marker inserted into the network buffer when a new
/// top-level navigation is detected.
///
/// Each navigation gets a monotonically-increasing sequence number.  The
/// sequence doubles as the index into the `boundaries` vector in
/// [`EventBuffer`] (sequence 0 = first navigation recorded, etc.).
#[derive(Debug, Clone)]
pub(crate) struct NavBoundary {
    /// Monotonically-increasing navigation index (0-based).
    pub sequence: u64,
    /// The top-level document URL at the time the boundary was recorded.
    pub url: String,
    /// The offset into the `"network-event"` bucket at which this navigation
    /// started.  All entries at indices >= `start_index` belong to this
    /// navigation; entries before belong to earlier navigations.
    pub start_index: usize,
}

/// A ring-buffer-style event store keyed by resource type.
///
/// Each resource type has an independent cap of `MAX_EVENTS_PER_TYPE` entries.
/// When the cap is reached the oldest event is evicted before inserting the new one,
/// so the buffer never grows beyond the limit.
///
/// Internally uses `VecDeque` so front-eviction is O(1) instead of O(n).
///
/// Navigation boundaries are tracked separately (see [`NavBoundary`]).  The
/// boundaries list grows without bound (capped at 1000 entries) so that
/// `--since -2` etc. can look back into history.
pub(crate) struct EventBuffer {
    inner: HashMap<String, VecDeque<Value>>,
    /// Navigation boundaries in order of insertion (oldest first).
    boundaries: Vec<NavBoundary>,
    /// Total number of `network-event` entries ever inserted (before eviction).
    /// Used to derive `start_index` for each boundary.
    network_total_inserted: usize,
    /// Number of `network-event` entries that have been evicted (for index arithmetic).
    network_evicted: usize,
    /// Monotonically-increasing counter for navigation boundary sequences.
    /// Tracked independently of `boundaries.len()` so sequence numbers remain
    /// unique even after the `MAX_BOUNDARIES` ring-buffer truncation kicks in.
    next_nav_sequence: u64,
}

const MAX_BOUNDARIES: usize = 1000;

impl EventBuffer {
    pub(crate) fn new() -> Self {
        Self {
            inner: HashMap::new(),
            boundaries: Vec::new(),
            network_total_inserted: 0,
            network_evicted: 0,
            next_nav_sequence: 0,
        }
    }

    /// Insert an event for `resource_type`.  If the bucket is already at
    /// `MAX_EVENTS_PER_TYPE` the oldest entry is evicted first (O(1)).
    pub(crate) fn insert(&mut self, resource_type: &str, event: Value) {
        let bucket = self.inner.entry(resource_type.to_owned()).or_default();
        if resource_type == "network-event" {
            if bucket.len() >= MAX_EVENTS_PER_TYPE {
                bucket.pop_front();
                self.network_evicted = self.network_evicted.saturating_add(1);
            }
            self.network_total_inserted = self.network_total_inserted.saturating_add(1);
        } else if bucket.len() >= MAX_EVENTS_PER_TYPE {
            bucket.pop_front();
        }
        bucket.push_back(event);
    }

    /// Record a navigation boundary for the `network-event` bucket.
    ///
    /// `url` is the top-level document URL of the new page.  The sequence
    /// number is assigned from a monotonic counter (0, 1, 2, …), independent
    /// of the bounded `boundaries` ring buffer, so it never wraps or repeats.
    ///
    /// Returns the assigned sequence number.
    pub(crate) fn record_nav_boundary(&mut self, url: String) -> u64 {
        let sequence = self.next_nav_sequence;
        self.next_nav_sequence = self.next_nav_sequence.saturating_add(1);
        let start_index = self.network_total_inserted;
        if self.boundaries.len() >= MAX_BOUNDARIES {
            self.boundaries.remove(0);
        }
        self.boundaries.push(NavBoundary {
            sequence,
            url,
            start_index,
        });
        sequence
    }

    /// Return a snapshot of all recorded navigation boundaries.
    #[allow(dead_code)]
    pub(crate) fn nav_boundaries(&self) -> &[NavBoundary] {
        &self.boundaries
    }

    /// Drain events for `resource_type` that occurred after the boundary
    /// identified by `since_nav_index`.
    ///
    /// - `since_nav_index == 0` (or `None`) returns the full buffer (same as [`drain`]).
    /// - `since_nav_index == -1` returns only events from the most-recent navigation.
    /// - `since_nav_index == -2` returns events from the second-most-recent, etc.
    ///
    /// When `resource_type` is not `"network-event"` or there are no boundaries,
    /// falls back to draining the full bucket.
    ///
    /// Returns `(events, boundary_used)` where `boundary_used` is `Some` when a
    /// boundary filtered the result.
    pub(crate) fn drain_since(
        &mut self,
        resource_type: &str,
        since_nav_index: i64,
    ) -> (Vec<Value>, Option<NavBoundary>) {
        // Index 0 / "all" → no boundary filtering.
        if since_nav_index == 0 || resource_type != "network-event" || self.boundaries.is_empty() {
            return (self.drain(resource_type), None);
        }

        // Resolve negative index relative to the end of the boundaries list.
        let n_boundaries = self.boundaries.len();
        let resolved: Option<usize> = if since_nav_index < 0 {
            // -1 → last, -2 → second-to-last, etc.
            let offset = usize::try_from(-since_nav_index).unwrap_or(usize::MAX);
            n_boundaries.checked_sub(offset)
        } else {
            // 1-based positive index (uncommon path)
            let idx = usize::try_from(since_nav_index).unwrap_or(usize::MAX);
            idx.checked_sub(1)
        };

        let Some(resolved_idx) = resolved else {
            // Asked for a boundary further back than we have — return all.
            return (self.drain(resource_type), None);
        };

        if resolved_idx >= n_boundaries {
            // Out-of-range positive index — return all.
            return (self.drain(resource_type), None);
        }

        let boundary = self.boundaries[resolved_idx].clone();
        // How many events currently in the buffer are from before this boundary?
        let current_len = self.inner.get(resource_type).map_or(0, VecDeque::len);
        let before_boundary = boundary.start_index;

        // Events before the boundary that are still in the buffer:
        // = total_inserted_before_boundary - evicted
        let inserted_before = before_boundary.saturating_sub(self.network_evicted);
        let skip = inserted_before.min(current_len);

        let bucket = self.inner.entry(resource_type.to_owned()).or_default();
        let events: Vec<Value> = bucket.drain(skip..).collect();

        (events, Some(boundary))
    }

    /// Drain all events for `resource_type` and return them in insertion order.
    ///
    /// The bucket is left empty (but still present in the map).  Returns an
    /// empty `Vec` if the type is unknown.
    pub(crate) fn drain(&mut self, resource_type: &str) -> Vec<Value> {
        match self.inner.get_mut(resource_type) {
            Some(bucket) => std::mem::take(bucket).into_iter().collect(),
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
        self.inner.values().all(VecDeque::is_empty)
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
    fn record_nav_boundary_returns_monotonic_sequence() {
        let mut buf = EventBuffer::new();
        let s0 = buf.record_nav_boundary("https://a/".into());
        let s1 = buf.record_nav_boundary("https://b/".into());
        let s2 = buf.record_nav_boundary("https://c/".into());
        assert_eq!((s0, s1, s2), (0, 1, 2));
        assert_eq!(buf.nav_boundaries().last().map(|b| b.sequence), Some(2));
    }

    #[test]
    fn nav_boundary_sequence_does_not_wrap_after_cap() {
        // Past the MAX_BOUNDARIES cap, sequences must keep incrementing even
        // though the boundaries vector is truncated from the front.
        let mut buf = EventBuffer::new();
        for i in 0..(MAX_BOUNDARIES + 5) {
            let seq = buf.record_nav_boundary(format!("https://example/{i}"));
            assert_eq!(
                usize::try_from(seq).ok(),
                Some(i),
                "sequence should equal insertion index"
            );
        }
        // Ring buffer stays capped.
        assert_eq!(buf.nav_boundaries().len(), MAX_BOUNDARIES);
        // The most recent boundary's sequence reflects the total insertions.
        let last_seq = buf.nav_boundaries().last().map(|b| b.sequence);
        assert_eq!(last_seq, Some((MAX_BOUNDARIES + 4) as u64));
        // The oldest retained boundary is not sequence 0 — it was evicted.
        let first_seq = buf.nav_boundaries().first().map(|b| b.sequence);
        assert_eq!(first_seq, Some(5_u64));
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
