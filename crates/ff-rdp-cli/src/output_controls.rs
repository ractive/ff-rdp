use serde_json::Value;

use crate::error::AppError;

/// Sort direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// Output control options parsed from CLI flags.
pub struct OutputControls {
    pub(crate) limit: Option<usize>,
    pub(crate) all: bool,
    pub(crate) sort_field: Option<String>,
    pub(crate) sort_dir: SortDir,
    pub(crate) fields: Option<Vec<String>>,
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
    ///
    /// Errors (iter-161 Theme D) when `--sort` names a field that appears on
    /// no entry of `results`: the old behaviour was a silent no-op —
    /// `compare_values(None, None)` is `Equal` for every pair, the sort is
    /// stable, and the caller got document order plus exit 0 with no way to
    /// tell the sort had been ignored.
    pub fn apply_sort(&self, results: &mut [Value]) -> Result<(), AppError> {
        if let Some(ref field) = self.sort_field {
            validate_names("--sort", std::slice::from_ref(field), results)?;
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
        Ok(())
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

    /// Validate that every `--fields` name appears on at least one entry of
    /// `results` (iter-161 Theme D).
    ///
    /// Callers MUST invoke this against the full, pre-[`Self::apply_limit`]
    /// result set — never the already-truncated page. `apply_fields` itself
    /// only projects and does not re-validate, because doing the validation
    /// after truncation would make a legitimate field name fail purely
    /// because of `--limit`'s cutoff: e.g. `dom` defaults to `--limit 20`,
    /// so `dom 'a' --fields text` on a page where anchors 1-20 happen to
    /// have no text but anchor 21 does would otherwise reject `text` even
    /// though it is a real key elsewhere in the same result set.
    pub fn validate_fields(&self, results: &[Value]) -> Result<(), AppError> {
        let Some(ref fields) = self.fields else {
            return Ok(());
        };
        validate_names("--fields", fields, results)
    }

    /// Filter to only the requested fields on each result entry.
    ///
    /// Does NOT validate `--fields` names — call [`Self::validate_fields`]
    /// against the pre-limit result set first (iter-161 Theme D; see that
    /// method's doc comment for why this must run before truncation, not
    /// here). The old behaviour, before either existed, silently destroyed
    /// the data: `ff-rdp dom 'a' --limit 2 --fields bogusfield` printed
    /// `{"results": [{}, {}], "total": 2}` and exited 0.
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

    /// Filter to only the requested fields on a single object value.
    ///
    /// This is the single-object counterpart to [`Self::apply_fields`] for
    /// commands that return one record (e.g. `perf vitals`) rather than a
    /// list. Non-object values are returned unchanged.
    ///
    /// Errors on an unknown `--fields` name for the same reason
    /// [`Self::apply_fields`] does (`perf vitals --fields bogus` → `{}`).
    pub fn apply_fields_object(&self, value: Value) -> Result<Value, AppError> {
        let Some(ref fields) = self.fields else {
            return Ok(value);
        };
        if let Value::Object(map) = value {
            let available: Vec<&str> = map.keys().map(String::as_str).collect();
            validate_against("--fields", fields, available)?;
            let filtered: serde_json::Map<String, Value> = map
                .into_iter()
                .filter(|(k, _)| fields.iter().any(|f| f == k))
                .collect();
            Ok(Value::Object(filtered))
        } else {
            Ok(value)
        }
    }
}

// ---------------------------------------------------------------------------
// --query / --query-regex (iter-211 Theme A)
// ---------------------------------------------------------------------------

/// The `--query` / `--query-regex` predicate, shared by `page-text`,
/// `snapshot`, `a11y summary` and `dom`.
///
/// Why this exists (iter-211): in the axi benchmark
/// (`kb/research/axi-benchmark-comparison.md`) every extraction task became
/// the same loop — `page-text | head -100`, then a guessed `dom` selector,
/// then three to six `eval` scripts until one hit. The agent had no way to
/// say "show me the part of the page that contains *billion*". `--query` is
/// the one-word form of the question it was actually asking, and one
/// implementation serves all four read commands so a recipe written against
/// one works on the others.
///
/// Matching is a **case-insensitive substring** by default, because that is
/// what `grep` gives an agent today and what the benchmark trajectories
/// reached for. `--query-regex` opts into a real regular expression; an
/// invalid pattern is rejected by clap's value parser (usage exit code 2),
/// not deep inside the command after a browser round-trip.
///
/// A filter built from a [`QueryArgs`](crate::cli::args::QueryArgs) with
/// neither flag set is inactive: [`QueryFilter::is_active`] is `false` and
/// [`QueryFilter::matches`] never runs, so a command that always constructs
/// one pays nothing when no flag was passed.
pub struct QueryFilter {
    matcher: Option<Matcher>,
}

enum Matcher {
    /// Already lowercased — the haystack is lowercased at match time.
    Substring(String),
    Regex(regex::Regex),
}

impl QueryFilter {
    /// Build from the flattened `--query` / `--query-regex` pair.
    ///
    /// The two are mutually exclusive at the clap level, so at most one arm
    /// is ever `Some`; `--query` wins if both somehow arrive.
    pub fn from_query_args(args: &crate::cli::args::QueryArgs) -> Self {
        let matcher = if let Some(text) = args.query.as_deref() {
            Some(Matcher::Substring(text.to_lowercase()))
        } else {
            args.query_regex.clone().map(Matcher::Regex)
        };
        Self { matcher }
    }

    /// Whether a `--query`/`--query-regex` was supplied.
    pub fn is_active(&self) -> bool {
        self.matcher.is_some()
    }

    /// Whether `haystack` satisfies the predicate.
    ///
    /// An inactive filter returns `false` rather than `true`: callers guard
    /// on [`Self::is_active`] before filtering at all, and "matches
    /// everything" would silently turn a bug in that guard into a no-op
    /// filter that reported `meta.matches` for rows nobody asked about.
    pub fn matches(&self, haystack: &str) -> bool {
        match &self.matcher {
            None => false,
            Some(Matcher::Substring(needle)) => haystack.to_lowercase().contains(needle.as_str()),
            Some(Matcher::Regex(re)) => re.is_match(haystack),
        }
    }

    /// Whether any string *directly* reachable in `value` matches: the value
    /// itself when it is a string, every element of an array, and every
    /// value of an object.
    ///
    /// Deliberately one level of container, not a deep walk — the snapshot
    /// filter needs "does this node's own text or attribute value match",
    /// and a deep walk would report every ancestor of a match as a match
    /// itself, collapsing the pruning this feeds.
    pub fn matches_shallow(&self, value: &Value) -> bool {
        if !self.is_active() {
            return false;
        }
        match value {
            Value::String(s) => self.matches(s),
            Value::Array(arr) => arr.iter().any(|v| match v {
                Value::String(s) => self.matches(s),
                _ => false,
            }),
            Value::Object(map) => map.values().any(|v| match v {
                Value::String(s) => self.matches(s),
                _ => false,
            }),
            _ => false,
        }
    }
}

/// Reject `--fields`/`--sort` names that appear on no entry of `results`
/// (iter-161 Theme D).
///
/// The schema is the data: the union of keys present across the object
/// entries in hand. Design decisions, recorded in DEC-035:
///
/// - **Union, not intersection.** A key present on some entries and absent on
///   others is legitimate — `dom` emits `text` only for elements that have
///   it — so validating against the intersection would break working
///   commands.
/// - **Skip when there is nothing to validate against.** An empty result set,
///   or one holding no object entries (a list of strings), yields an empty
///   union; erroring there would turn a legitimate empty query into a
///   failure. `ff-rdp dom '.no-such-class' --fields tag` stays exit 0.
/// - **Strict by default, no `--fields-lax`.** Unlike a `--jq` filter that
///   resolves to nothing — a legitimate probe, which is why `--jq-strict` is
///   opt-in — a `--fields`/`--sort` name matching no entry is always a typo
///   or a renamed field, and the old outcome was strictly worse than an
///   error in every case a caller could want.
fn validate_names(flag: &str, names: &[String], results: &[Value]) -> Result<(), AppError> {
    let mut available: Vec<&str> = Vec::new();
    for entry in results {
        if let Value::Object(map) = entry {
            for key in map.keys() {
                let key = key.as_str();
                if !available.contains(&key) {
                    available.push(key);
                }
            }
        }
    }
    validate_against(flag, names, available)
}

/// Reject any of `names` missing from `available` — see [`validate_names`],
/// which computes `available` as the union of keys over a result *list*;
/// [`OutputControls::apply_fields_object`] passes the keys of its single
/// record directly.
fn validate_against(
    flag: &str,
    names: &[String],
    mut available: Vec<&str>,
) -> Result<(), AppError> {
    // Nothing to validate against — an empty result set is not an error.
    if available.is_empty() {
        return Ok(());
    }

    let unknown: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|n| !available.contains(n))
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }

    available.sort_unstable();
    let noun = if unknown.len() == 1 { "name" } else { "names" };
    Err(AppError::User(format!(
        "{flag}: unknown field {noun} {}; available: {}",
        unknown
            .iter()
            .map(|n| format!("'{n}'"))
            .collect::<Vec<_>>()
            .join(", "),
        available.join(", ")
    )))
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
    use crate::cli::args::QueryArgs;

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
        c.apply_sort(&mut items).expect("field is present");
        assert_eq!(items[0]["n"], 1);
        assert_eq!(items[1]["n"], 2);
        assert_eq!(items[2]["n"], 3);
    }

    #[test]
    fn sort_numeric_desc() {
        let mut items = vec![json!({"n": 1}), json!({"n": 3}), json!({"n": 2})];
        let c = make_controls(None, false, Some("n"), SortDir::Desc, None);
        c.apply_sort(&mut items).expect("field is present");
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
        c.apply_sort(&mut items).expect("field is present");
        assert_eq!(items[0]["s"], "apple");
        assert_eq!(items[1]["s"], "banana");
        assert_eq!(items[2]["s"], "cherry");
    }

    #[test]
    fn sort_missing_field_sorts_before_present() {
        let mut items = vec![json!({"n": 5}), json!({"other": 1}), json!({"n": 2})];
        let c = make_controls(None, false, Some("n"), SortDir::Asc, None);
        c.apply_sort(&mut items).expect("field is present");
        // None sorts first in Asc
        assert_eq!(items[0].get("n"), None);
        assert_eq!(items[1]["n"], 2);
        assert_eq!(items[2]["n"], 5);
    }

    #[test]
    fn sort_noop_when_no_field() {
        let mut items = vec![json!({"n": 3}), json!({"n": 1})];
        let c = make_controls(None, false, None, SortDir::Asc, None);
        c.apply_sort(&mut items).expect("field is present");
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

    // ── iter-161 Theme D: unknown --fields/--sort names fail loud ────────────

    /// `--fields` validation must run against the full, pre-`apply_limit`
    /// result set, not the truncated page — otherwise a real field name that
    /// simply doesn't appear within the `--limit` window would be rejected
    /// as unknown. `dom` alone defaults `--limit` to 20, so this is reachable
    /// without the caller ever passing `--limit` explicitly. Regression test
    /// for a bug found in review: [`OutputControls::apply_fields`] used to
    /// validate internally, and every call site invoked it *after*
    /// [`OutputControls::apply_limit`].
    #[test]
    fn unit_161_fields_validated_before_limit_not_after() {
        // `text` is absent from the first two entries and present only on the
        // third — the exact shape `dom` produces for elements without text.
        let items = vec![
            json!({"tag": "a"}),
            json!({"tag": "img"}),
            json!({"tag": "a", "text": "hi"}),
        ];
        let c = make_controls(Some(2), false, None, SortDir::Asc, Some(vec!["text"]));

        // Validating against the full set (correct order): `text` is a real
        // field somewhere in `items`, so this must succeed.
        c.validate_fields(&items)
            .expect("text is present on the third entry of the full result set");

        // Confirm the bug this guards against: validating against the
        // already-limited page (the wrong order) does reject `text`, which is
        // exactly why callers must call `validate_fields` before
        // `apply_limit`, never after.
        let (limited, _, truncated) = c.apply_limit(items, Some(2));
        assert!(truncated);
        assert_eq!(limited.len(), 2);
        c.validate_fields(&limited)
            .expect_err("text is genuinely absent from the truncated page");
    }

    /// AC `unit_161_field_validation_union_and_empty_set`: the union of keys
    /// present is the schema, and there is nothing to validate against when
    /// the result set is empty or holds no objects.
    #[test]
    fn unit_161_field_validation_union_and_empty_set() {
        // Union, not intersection: `text` is on only one of two entries
        // (exactly what `dom` emits for elements that have text).
        let items = vec![json!({"tag": "a", "text": "hi"}), json!({"tag": "img"})];
        let c = make_controls(None, false, Some("text"), SortDir::Asc, Some(vec!["text"]));
        let mut sortable = items.clone();
        c.apply_sort(&mut sortable)
            .expect("a key on only one entry is still in the union");
        c.validate_fields(&items)
            .expect("a key on only one entry is still in the union");
        let out = c.apply_fields(items);
        assert_eq!(out.len(), 2);

        // Empty result set: no union to check, so no error and nothing filtered.
        let c = make_controls(None, false, Some("nope"), SortDir::Asc, Some(vec!["nope"]));
        let mut empty: Vec<Value> = vec![];
        c.apply_sort(&mut empty).expect("empty set is not an error");
        c.validate_fields(&empty)
            .expect("empty set is not an error");
        assert_eq!(c.apply_fields(vec![]), Vec::<Value>::new());

        // A result set of non-object values: same reasoning, empty union.
        let strings = vec![json!("a"), json!("b")];
        let mut sortable = strings.clone();
        c.apply_sort(&mut sortable)
            .expect("a list of strings has no keys to validate against");
        c.validate_fields(&strings)
            .expect("a list of strings has no keys to validate against");
        assert_eq!(c.apply_fields(strings.clone()), strings);

        // Single-record counterpart rejects an unknown name.
        let err = c
            .apply_fields_object(json!({"lcp_ms": 1, "cls": 0.1}))
            .expect_err("apply_fields_object must reject an unknown name");
        let msg = err.to_string();
        assert!(msg.contains("--fields"), "must name the flag: {msg}");
        assert!(msg.contains("'nope'"), "must name the offender: {msg}");
        assert!(msg.contains("lcp_ms"), "must list what is available: {msg}");
    }

    /// The defect this replaces: `--fields bogusfield` used to return
    /// `[{}, {}]` with exit 0, and `--sort nosuchfield` used to be a silent
    /// no-op.
    #[test]
    fn unit_161_unknown_names_are_rejected_with_available_keys() {
        let items = vec![json!({"tag": "a", "text": "x"}), json!({"tag": "b"})];

        let c = make_controls(None, false, None, SortDir::Asc, Some(vec!["bogusfield"]));
        let err = c
            .validate_fields(&items)
            .expect_err("--fields bogusfield must be an error, not [{}, {}]");
        let msg = err.to_string();
        assert!(msg.contains("--fields"), "must name the flag: {msg}");
        assert!(
            msg.contains("'bogusfield'"),
            "must name the offender: {msg}"
        );
        assert!(msg.contains("tag"), "must list available keys: {msg}");
        assert!(msg.contains("text"), "must list available keys: {msg}");

        let c = make_controls(None, false, Some("nosuchfield"), SortDir::Asc, None);
        let mut sortable = items.clone();
        let err = c
            .apply_sort(&mut sortable)
            .expect_err("--sort nosuchfield must be an error, not a silent no-op");
        let msg = err.to_string();
        assert!(msg.contains("--sort"), "must name the flag: {msg}");
        assert!(
            msg.contains("'nosuchfield'"),
            "must name the offender: {msg}"
        );
        assert!(msg.contains("tag"), "must list available keys: {msg}");

        // Every name is reported, not just the first.
        let c = make_controls(None, false, None, SortDir::Asc, Some(vec!["tag", "x", "y"]));
        let msg = c
            .validate_fields(&items)
            .expect_err("two unknown names must still be an error")
            .to_string();
        assert!(msg.contains("'x'") && msg.contains("'y'"), "got: {msg}");
        assert!(msg.contains("names"), "plural wording expected: {msg}");
    }

    // ── iter-211 Theme A: --query / --query-regex ───────────────────────────

    fn query_args(query: Option<&str>, regex: Option<&str>) -> QueryArgs {
        QueryArgs {
            query: query.map(str::to_owned),
            query_regex: regex.map(|r| regex::Regex::new(r).expect("test pattern must compile")),
        }
    }

    /// AC `query_filter_is_case_insensitive_substring_by_default`.
    #[test]
    fn query_filter_is_case_insensitive_substring_by_default() {
        let f = QueryFilter::from_query_args(&query_args(Some("Billion"), None));
        assert!(f.is_active());
        assert!(f.matches("8.1 billion people"), "lowercase haystack");
        assert!(f.matches("BILLION"), "uppercase haystack");
        assert!(f.matches("multibillionaire"), "substring, not word-boundary");
        assert!(!f.matches("8.1 million people"));

        // Not a regex: metacharacters are literal in the default mode, so a
        // caller pasting a URL or a price does not get a surprise match.
        let f = QueryFilter::from_query_args(&query_args(Some("a.c"), None));
        assert!(f.matches("xxa.cxx"));
        assert!(!f.matches("abc"), "'.' must be literal without --query-regex");
    }

    #[test]
    fn query_regex_matches_as_a_pattern_and_respects_its_own_case_rules() {
        let f = QueryFilter::from_query_args(&query_args(None, Some(r"^\d{4}$")));
        assert!(f.is_active());
        assert!(f.matches("1804"));
        assert!(!f.matches("in 1804"));

        // Case sensitivity is the pattern's own business — `(?i)` opts in.
        let f = QueryFilter::from_query_args(&query_args(None, Some("Babbage")));
        assert!(f.matches("Charles Babbage"));
        assert!(!f.matches("charles babbage"));
        let f = QueryFilter::from_query_args(&query_args(None, Some("(?i)babbage")));
        assert!(f.matches("Charles Babbage"));
    }

    /// AC `query_regex_rejects_invalid_pattern_with_exit_2`.
    ///
    /// The rejection happens in clap's value parser, so it is a *usage* error
    /// — exit code 2, printed before any connection to Firefox is opened.
    /// `AppError::User` would have been exit 1 and would have cost a browser
    /// round-trip first.
    #[test]
    fn query_regex_rejects_invalid_pattern_with_exit_2() {
        use clap::Parser as _;
        let Err(err) =
            crate::cli::args::Cli::try_parse_from(["ff-rdp", "page-text", "--query-regex", "([unclosed"])
        else {
            panic!("an unparseable pattern must not reach the browser");
        };
        assert_eq!(err.exit_code(), 2, "usage error, not runtime error");
        let msg = err.to_string();
        assert!(
            msg.contains("invalid regular expression"),
            "the message must say what is wrong: {msg}"
        );
    }

    #[test]
    fn query_and_query_regex_are_mutually_exclusive() {
        use clap::Parser as _;
        let Err(err) = crate::cli::args::Cli::try_parse_from([
            "ff-rdp",
            "page-text",
            "--query",
            "a",
            "--query-regex",
            "b",
        ]) else {
            panic!("--query and --query-regex must not combine");
        };
        assert_eq!(err.exit_code(), 2);
    }

    /// An inactive filter matches nothing, so a guard bug shows up as an
    /// empty result rather than as a filter that silently passes everything.
    #[test]
    fn inactive_filter_is_never_a_match() {
        let f = QueryFilter::from_query_args(&QueryArgs::default());
        assert!(!f.is_active());
        assert!(!f.matches("anything at all"));
        assert!(!f.matches_shallow(&json!({"a": "anything at all"})));
    }

    #[test]
    fn matches_shallow_covers_strings_arrays_and_object_values_one_level_deep() {
        let f = QueryFilter::from_query_args(&query_args(Some("needle"), None));
        assert!(f.matches_shallow(&json!("a needle here")));
        assert!(f.matches_shallow(&json!(["x", "a needle here"])));
        assert!(f.matches_shallow(&json!({"href": "/needle", "id": "x"})));
        assert!(!f.matches_shallow(&json!({"id": "x"})));
        // One level only: a nested object is not searched, because the
        // snapshot pruning that consumes this needs "did THIS node match",
        // not "did anything below it match".
        assert!(!f.matches_shallow(&json!({"child": {"id": "needle"}})));
        // Non-string scalars never match.
        assert!(!f.matches_shallow(&json!(42)));
    }
}
