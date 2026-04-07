use serde_json::Value;

/// Sort direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// Output control options parsed from CLI flags.
pub struct OutputControls {
    pub limit: Option<usize>,
    pub all: bool,
    pub sort_field: Option<String>,
    pub sort_dir: SortDir,
    pub fields: Option<Vec<String>>,
}

impl OutputControls {
    /// Create from CLI global flags.
    pub fn from_cli(cli: &crate::cli::args::Cli, default_sort_dir: SortDir) -> Self {
        let sort_dir = if cli.asc {
            SortDir::Asc
        } else if cli.desc {
            SortDir::Desc
        } else {
            default_sort_dir
        };
        Self {
            limit: cli.limit,
            all: cli.all,
            sort_field: cli.sort.clone(),
            sort_dir,
            fields: cli.fields.clone(),
        }
    }

    /// Apply sorting to results in-place.
    pub fn apply_sort(&self, results: &mut [Value]) {
        if let Some(ref field) = self.sort_field {
            let dir = self.sort_dir;
            results.sort_by(|a, b| {
                let va = a.get(field);
                let vb = b.get(field);
                let cmp = compare_values(va, vb);
                match dir {
                    SortDir::Asc => cmp,
                    SortDir::Desc => cmp.reverse(),
                }
            });
        }
    }

    /// Apply limit and return `(limited_results, total_before_limit, was_truncated)`.
    pub fn apply_limit(
        &self,
        results: Vec<Value>,
        default_limit: Option<usize>,
    ) -> (Vec<Value>, usize, bool) {
        let total = results.len();
        if self.all {
            return (results, total, false);
        }
        let effective_limit = self.limit.or(default_limit);
        match effective_limit {
            Some(limit) if total > limit => {
                let truncated = results.into_iter().take(limit).collect();
                (truncated, total, true)
            }
            _ => (results, total, false),
        }
    }

    /// Filter to only the requested fields on each result entry.
    pub fn apply_fields(&self, results: Vec<Value>) -> Vec<Value> {
        let Some(ref fields) = self.fields else {
            return results;
        };
        results
            .into_iter()
            .map(|entry| {
                if let Value::Object(map) = entry {
                    let filtered: serde_json::Map<String, Value> = map
                        .into_iter()
                        .filter(|(k, _)| fields.iter().any(|f| f == k))
                        .collect();
                    Value::Object(filtered)
                } else {
                    entry
                }
            })
            .collect()
    }
}

/// Compare two optional JSON values for sorting purposes.
///
/// `None` sorts before any value (nulls first in ascending order).
/// Numeric values are compared numerically; everything else falls back to
/// string representation comparison.
fn compare_values(a: Option<&Value>, b: Option<&Value>) -> std::cmp::Ordering {
    match (a, b) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (Some(a), Some(b)) => {
            // Prefer numeric comparison when both sides are numbers.
            if let (Some(na), Some(nb)) = (a.as_f64(), b.as_f64()) {
                return na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal);
            }
            // Fall back to string comparison.
            let sa = a.as_str().unwrap_or_default();
            let sb = b.as_str().unwrap_or_default();
            sa.cmp(sb)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_controls(
        limit: Option<usize>,
        all: bool,
        sort_field: Option<&str>,
        sort_dir: SortDir,
        fields: Option<Vec<&str>>,
    ) -> OutputControls {
        OutputControls {
            limit,
            all,
            sort_field: sort_field.map(str::to_owned),
            sort_dir,
            fields: fields.map(|v| v.into_iter().map(str::to_owned).collect()),
        }
    }

    // ── sorting ─────────────────────────────────────────────────────────────

    #[test]
    fn sort_numeric_asc() {
        let mut items = vec![json!({"n": 3}), json!({"n": 1}), json!({"n": 2})];
        let c = make_controls(None, false, Some("n"), SortDir::Asc, None);
        c.apply_sort(&mut items);
        assert_eq!(items[0]["n"], 1);
        assert_eq!(items[1]["n"], 2);
        assert_eq!(items[2]["n"], 3);
    }

    #[test]
    fn sort_numeric_desc() {
        let mut items = vec![json!({"n": 1}), json!({"n": 3}), json!({"n": 2})];
        let c = make_controls(None, false, Some("n"), SortDir::Desc, None);
        c.apply_sort(&mut items);
        assert_eq!(items[0]["n"], 3);
        assert_eq!(items[1]["n"], 2);
        assert_eq!(items[2]["n"], 1);
    }

    #[test]
    fn sort_string_asc() {
        let mut items = vec![
            json!({"s": "banana"}),
            json!({"s": "apple"}),
            json!({"s": "cherry"}),
        ];
        let c = make_controls(None, false, Some("s"), SortDir::Asc, None);
        c.apply_sort(&mut items);
        assert_eq!(items[0]["s"], "apple");
        assert_eq!(items[1]["s"], "banana");
        assert_eq!(items[2]["s"], "cherry");
    }

    #[test]
    fn sort_missing_field_sorts_before_present() {
        let mut items = vec![json!({"n": 5}), json!({"other": 1}), json!({"n": 2})];
        let c = make_controls(None, false, Some("n"), SortDir::Asc, None);
        c.apply_sort(&mut items);
        // None sorts first in Asc
        assert_eq!(items[0].get("n"), None);
        assert_eq!(items[1]["n"], 2);
        assert_eq!(items[2]["n"], 5);
    }

    #[test]
    fn sort_noop_when_no_field() {
        let mut items = vec![json!({"n": 3}), json!({"n": 1})];
        let c = make_controls(None, false, None, SortDir::Asc, None);
        c.apply_sort(&mut items);
        // Order unchanged
        assert_eq!(items[0]["n"], 3);
        assert_eq!(items[1]["n"], 1);
    }

    // ── limiting ─────────────────────────────────────────────────────────────

    #[test]
    fn limit_with_explicit_limit() {
        let items = vec![json!(1), json!(2), json!(3)];
        let c = make_controls(Some(2), false, None, SortDir::Asc, None);
        let (out, total, truncated) = c.apply_limit(items, None);
        assert_eq!(out.len(), 2);
        assert_eq!(total, 3);
        assert!(truncated);
    }

    #[test]
    fn limit_with_default_limit() {
        let items = vec![json!(1), json!(2), json!(3)];
        let c = make_controls(None, false, None, SortDir::Asc, None);
        let (out, total, truncated) = c.apply_limit(items, Some(2));
        assert_eq!(out.len(), 2);
        assert_eq!(total, 3);
        assert!(truncated);
    }

    #[test]
    fn limit_all_overrides_default() {
        let items = vec![json!(1), json!(2), json!(3)];
        let c = make_controls(None, true, None, SortDir::Asc, None);
        let (out, total, truncated) = c.apply_limit(items, Some(2));
        assert_eq!(out.len(), 3);
        assert_eq!(total, 3);
        assert!(!truncated);
    }

    #[test]
    fn limit_not_triggered_when_under_limit() {
        let items = vec![json!(1), json!(2)];
        let c = make_controls(Some(5), false, None, SortDir::Asc, None);
        let (out, total, truncated) = c.apply_limit(items, None);
        assert_eq!(out.len(), 2);
        assert_eq!(total, 2);
        assert!(!truncated);
    }

    // ── field filtering ──────────────────────────────────────────────────────

    #[test]
    fn fields_filters_object_keys() {
        let items = vec![json!({"a": 1, "b": 2, "c": 3})];
        let c = make_controls(None, false, None, SortDir::Asc, Some(vec!["a", "c"]));
        let out = c.apply_fields(items);
        assert_eq!(out[0]["a"], 1);
        assert_eq!(out[0]["c"], 3);
        assert!(out[0].get("b").is_none());
    }

    #[test]
    fn fields_noop_when_not_set() {
        let items = vec![json!({"a": 1, "b": 2})];
        let c = make_controls(None, false, None, SortDir::Asc, None);
        let out = c.apply_fields(items);
        assert_eq!(out[0]["a"], 1);
        assert_eq!(out[0]["b"], 2);
    }

    #[test]
    fn fields_passthrough_non_object() {
        let items = vec![json!("a string"), json!(42)];
        let c = make_controls(None, false, None, SortDir::Asc, Some(vec!["x"]));
        let out = c.apply_fields(items);
        // Non-object entries pass through unchanged
        assert_eq!(out[0], json!("a string"));
        assert_eq!(out[1], json!(42));
    }
}
