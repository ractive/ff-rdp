use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Compiler, Ctx, Native, Vars, data};
use jaq_json::Val;
use serde_json::Value;

use crate::hints::Hint;

/// Type alias matching jaq-core 3.x idiom: a filter with no imported
/// modules beyond the built-in function lookup table.
type D = data::JustLut<Val>;

/// Build the standard JSON output envelope.
pub fn envelope(results: &Value, total: usize, meta: &Value) -> Value {
    envelope_with_truncation(results, total, total, false, meta)
}

/// Build the standard JSON output envelope with optional truncation info.
///
/// When `truncated` is true, a `"truncated": true` field and a human-readable
/// `"hint"` field are included so callers know results were capped.
pub fn envelope_with_truncation(
    results: &Value,
    shown: usize,
    total: usize,
    truncated: bool,
    meta: &Value,
) -> Value {
    let mut env = serde_json::json!({
        "results": results,
        "total": total,
        "meta": meta,
    });
    if truncated && let Some(obj) = env.as_object_mut() {
        obj.insert("truncated".to_string(), Value::Bool(true));
        obj.insert(
            "hint".to_string(),
            Value::String(format!(
                "showing {shown} of {total}, use --all for complete list"
            )),
        );
    }
    env
}

/// Inject contextual hints into a pre-built envelope.
///
/// Adds `"hints": [...]` as a top-level key. The array is always present
/// (empty when no hints are provided).
pub fn inject_hints(envelope: &mut Value, hints: &[Hint]) {
    if let Some(obj) = envelope.as_object_mut() {
        let hints_json: Vec<Value> = hints
            .iter()
            .map(|h| serde_json::to_value(h).unwrap_or(Value::Null))
            .collect();
        obj.insert("hints".to_string(), Value::Array(hints_json));
    }
}

/// Compile and execute a jq filter on a JSON value.
///
/// Returns the filtered results as a `Vec<Value>`. Each output item from the
/// filter becomes one element. If the filter produces no outputs the vec is
/// empty. Parse and runtime errors are surfaced as `anyhow::Error`.
pub fn apply_jq_filter(input: &Value, filter: &str) -> anyhow::Result<Vec<Value>> {
    let compiled = compile_jq_filter(filter)?;
    execute_jq_filter(&compiled, input)
}

/// Compile a jq filter string into an owned, reusable `Filter`.
///
/// The `Arena` used by the `Loader` is a temporary scratch pad that is dropped
/// at the end of this function — the returned `Filter` owns all its data.
fn compile_jq_filter(filter_code: &str) -> anyhow::Result<jaq_core::compile::Filter<Native<D>>> {
    let program = File {
        code: filter_code,
        path: (),
    };

    let defs = jaq_core::defs()
        .chain(jaq_std::defs())
        .chain(jaq_json::defs());
    let loader = Loader::new(defs);
    let arena = Arena::default();

    let modules = loader.load(&arena, program).map_err(|errs| {
        let msg = errs
            .iter()
            .map(|(_file, e)| format!("{e:#?}"))
            .collect::<Vec<_>>()
            .join("; ");
        anyhow::anyhow!("jq parse error: {msg}")
    })?;

    let funs = jaq_core::funs::<D>()
        .chain(jaq_std::funs::<D>())
        .chain(jaq_json::funs::<D>());

    Compiler::default()
        .with_funs(funs)
        .compile(modules)
        .map_err(|errs| {
            let msg = errs
                .iter()
                .map(|(_file, e)| format!("{e:#?}"))
                .collect::<Vec<_>>()
                .join("; ");
            anyhow::anyhow!("jq compile error: {msg}")
        })
}

/// Execute a pre-compiled jq filter against a JSON value.
fn execute_jq_filter(
    filter: &jaq_core::compile::Filter<Native<D>>,
    value: &Value,
) -> anyhow::Result<Vec<Value>> {
    let input: Val = serde_json::from_value(value.clone())
        .map_err(|e| anyhow::anyhow!("jq input conversion error: {e}"))?;

    let ctx = Ctx::<D>::new(&filter.lut, Vars::new([]));

    let mut results = Vec::new();
    for result in filter.id.run((ctx, input)).map(jaq_core::unwrap_valr) {
        let val = result.map_err(|e| anyhow::anyhow!("jq runtime error: {e}"))?;
        let json = val_to_value(&val)?;
        results.push(json);
    }

    Ok(results)
}

/// Convert a jaq `Val` to a `serde_json::Value`.
///
/// `Val` does not implement `Serialize`, but it does implement `Display`
/// which outputs JSON. We format to string and re-parse.
fn val_to_value(val: &Val) -> anyhow::Result<Value> {
    let json_str = val.to_string();
    serde_json::from_str(&json_str)
        .map_err(|e| anyhow::anyhow!("jq output is not valid JSON: {e}: {json_str}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn envelope_structure() {
        let results = json!(["a", "b"]);
        let meta = json!({"tab": "test"});
        let env = envelope(&results, 2, &meta);
        assert_eq!(env["total"], 2);
        assert_eq!(env["results"], results);
        assert_eq!(env["meta"], meta);
        assert!(env.get("truncated").is_none());
    }

    #[test]
    fn envelope_with_truncation_not_truncated() {
        let results = json!(["a", "b"]);
        let meta = json!({});
        let env = envelope_with_truncation(&results, 2, 2, false, &meta);
        assert_eq!(env["total"], 2);
        assert!(env.get("truncated").is_none());
        assert!(env.get("hint").is_none());
    }

    #[test]
    fn envelope_with_truncation_truncated() {
        let results = json!(["a"]);
        let meta = json!({});
        let env = envelope_with_truncation(&results, 1, 5, true, &meta);
        assert_eq!(env["total"], 5);
        assert_eq!(env["truncated"], true);
        let hint = env["hint"].as_str().expect("hint should be a string");
        assert!(hint.contains("1 of 5"));
        assert!(hint.contains("--all"));
    }

    #[test]
    fn jq_identity_filter() {
        let val = json!({"name": "test"});
        let results = apply_jq_filter(&val, ".").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], val);
    }

    #[test]
    fn jq_field_access() {
        let val = json!({"name": "hello", "count": 3});
        let results = apply_jq_filter(&val, ".name").unwrap();
        assert_eq!(results, vec![json!("hello")]);
    }

    #[test]
    fn jq_array_iteration() {
        let val = json!([1, 2, 3]);
        let results = apply_jq_filter(&val, ".[]").unwrap();
        assert_eq!(results, vec![json!(1), json!(2), json!(3)]);
    }

    #[test]
    fn jq_invalid_filter() {
        let val = json!({"x": 1});
        let result = apply_jq_filter(&val, "this is not valid %%%");
        assert!(result.is_err());
    }
}
