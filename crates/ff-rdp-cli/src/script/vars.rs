//! Variable substitution and secret redaction for script steps.
//!
//! Substitution syntax: `{{env.NAME}}`, `{{vars.NAME}}`, `{{steps[N].results.X}}`
//! (N is 1-based step index).  Resolution happens at step execution time.

use std::collections::{HashMap, HashSet};

use anyhow::{Context as _, bail};
use serde_json::Value;

/// Context available when resolving variable references.
pub struct VarContext<'a> {
    /// Variables from script `vars:` section plus `--vars` overrides.
    pub vars: &'a HashMap<String, String>,
    /// Results from completed steps, indexed 0-based (step 1 → index 0).
    pub step_results: &'a [Value],
    /// Show secrets in output (from `--show-secrets`). Reserved for future use.
    #[allow(dead_code)]
    pub show_secrets: bool,
    /// Policy controlling which `{{env.X}}` references are allowed.
    pub env_policy: &'a EnvPolicy,
}

/// Safe-default env vars that may be interpolated without an explicit
/// allowlist entry. Picked to be ubiquitously needed and free of secret
/// content on typical workstations / CI environments.
pub const SAFE_DEFAULTS: &[&str] = &["HOME", "USER", "LANG", "LC_ALL", "TZ"];

/// Policy for `{{env.X}}` resolution.
///
/// Fail-closed: a name must appear in [`SAFE_DEFAULTS`] or `allowlist`
/// to be resolved. Names matching [`is_secret_name`] are refused
/// unconditionally — even an explicit allowlist entry will not unlock
/// them.
#[derive(Debug, Default, Clone)]
pub struct EnvPolicy {
    /// Caller-supplied allowlist (typically from `--allow-env`).
    pub allowlist: HashSet<String>,
}

impl EnvPolicy {
    /// Build a policy from an iterator of names.
    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            allowlist: names.into_iter().map(Into::into).collect(),
        }
    }

    /// Return `Ok` if `{{env.NAME}}` is allowed under this policy.
    pub fn check(&self, name: &str) -> anyhow::Result<()> {
        if is_secret_name(name) {
            bail!(
                "env var `{name}` refused: name matches secret-name pattern (cannot be allowlisted — rename the variable or pass the value via --vars instead)"
            );
        }
        if SAFE_DEFAULTS.contains(&name) {
            return Ok(());
        }
        if self.allowlist.contains(name) {
            return Ok(());
        }
        bail!("env var `{name}` not in allowlist (use --allow-env {name})");
    }
}

/// Variable names (case-insensitive check) that trigger secret redaction.
const SECRET_PATTERNS: &[&str] = &["password", "token", "secret", "key", "passwd", "pwd"];

/// Returns `true` when a variable name should be treated as a secret.
pub fn is_secret_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    SECRET_PATTERNS.iter().any(|p| lower.contains(p))
}

/// Substitute all `{{...}}` placeholders in a string.
///
/// Errors if a placeholder references an undefined variable or step result.
pub fn substitute(template: &str, ctx: &VarContext<'_>) -> anyhow::Result<String> {
    let mut result = String::with_capacity(template.len());
    let mut remaining = template;

    while let Some(start) = remaining.find("{{") {
        result.push_str(&remaining[..start]);
        remaining = &remaining[start + 2..];

        let end = remaining
            .find("}}")
            .with_context(|| format!("unclosed `{{{{` in template: {template:?}"))?;

        let expr = remaining[..end].trim();
        remaining = &remaining[end + 2..];

        let value = resolve_expr(expr, ctx)
            .with_context(|| format!("resolving `{{{{{expr}}}}}` in template: {template:?}"))?;
        result.push_str(&value);
    }

    result.push_str(remaining);
    Ok(result)
}

/// Resolve a single `{{...}}` expression to its string value.
fn resolve_expr(expr: &str, ctx: &VarContext<'_>) -> anyhow::Result<String> {
    if let Some(name) = expr.strip_prefix("env.") {
        ctx.env_policy.check(name)?;
        let val = std::env::var(name)
            .with_context(|| format!("environment variable `{name}` is not set"))?;
        return Ok(val);
    }

    if let Some(name) = expr.strip_prefix("vars.") {
        let val = ctx.vars.get(name).with_context(|| {
            format!("variable `{name}` is not defined — pass it with `--vars {name}=<value>`")
        })?;
        return Ok(val.clone());
    }

    // steps[N].results.X — 1-based step index
    if let Some(rest) = expr.strip_prefix("steps[") {
        let bracket = rest
            .find(']')
            .context("missing `]` in steps[N] reference")?;
        let idx_str = &rest[..bracket];
        let idx: usize = idx_str
            .parse::<usize>()
            .with_context(|| format!("invalid step index `{idx_str}`"))?;
        if idx == 0 {
            bail!("step index in `steps[N]` is 1-based — use `steps[1]` for the first step");
        }
        let zero_idx = idx - 1;
        let step_result = ctx.step_results.get(zero_idx).with_context(|| {
            format!(
                "step {idx} has not completed yet — step references are resolved at execution time"
            )
        })?;

        let path = rest.get(bracket + 2..).unwrap_or(""); // skip '].'
        if path.is_empty() {
            return Ok(step_result.to_string());
        }
        // Walk the dot-separated path into the JSON value.
        let mut current = step_result;
        for key in path.split('.') {
            current = current.get(key).with_context(|| {
                format!("key `{key}` not found in step {idx} results: {step_result}")
            })?;
        }
        return Ok(match current {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        });
    }

    bail!(
        "unknown variable expression `{{{{{expr}}}}}` — supported: env.NAME, vars.NAME, steps[N].results.FIELD"
    );
}

/// Redact secret values in a JSON value tree for output.
///
/// Walks the value and replaces strings in fields whose key matches the
/// secret name patterns with `"[REDACTED]"`.
pub fn redact_secrets(value: &Value, vars: &HashMap<String, String>, show_secrets: bool) -> Value {
    if show_secrets {
        return value.clone();
    }
    redact_value(value, vars)
}

/// Minimum length for substring-based secret redaction.
///
/// Short values (e.g. "a") would aggressively wipe unrelated output via
/// substring replacement, so we only do substring redaction when the value
/// is long enough to be unambiguous.  Exact-match redaction (field-key
/// based) is not length-gated.
const MIN_SECRET_SUBSTRING_LEN: usize = 4;

fn redact_value(value: &Value, secrets: &HashMap<String, String>) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                let is_secret_key = is_secret_name(k);
                let new_val = if is_secret_key {
                    Value::String("[REDACTED]".to_owned())
                } else {
                    redact_value(v, secrets)
                };
                out.insert(k.clone(), new_val);
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(|v| redact_value(v, secrets)).collect()),
        Value::String(s) => {
            // Also redact literal secret values embedded in strings.
            // Only apply substring replacement when the secret value is long enough
            // to avoid false-positive matches.
            let mut out = s.clone();
            for (k, v) in secrets {
                if is_secret_name(k)
                    && v.len() >= MIN_SECRET_SUBSTRING_LEN
                    && out.contains(v.as_str())
                {
                    out = out.replace(v.as_str(), "[REDACTED]");
                }
            }
            Value::String(out)
        }
        other => other.clone(),
    }
}

/// Scan a template string for `{{env.X}}` references and return a map of
/// the referenced environment variable names to their current values.
///
/// Values that are undefined in the environment are silently skipped.
/// This map is used to extend secret redaction to env-sourced values.
pub fn collect_env_secrets(template: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let mut remaining = template;
    while let Some(start) = remaining.find("{{") {
        remaining = &remaining[start + 2..];
        let Some(end) = remaining.find("}}") else {
            break;
        };
        let expr = remaining[..end].trim();
        remaining = &remaining[end + 2..];
        if let Some(name) = expr.strip_prefix("env.")
            && let Ok(val) = std::env::var(name)
        {
            result.insert(name.to_owned(), val);
        }
    }
    result
}

/// Validate that all `{{vars.X}}` references in a string are defined.
///
/// Used by `--dry-run` to catch missing variables before execution.
pub fn check_undefined_vars(template: &str, vars: &HashMap<String, String>) -> anyhow::Result<()> {
    let mut remaining = template;
    while let Some(start) = remaining.find("{{") {
        remaining = &remaining[start + 2..];
        let end = remaining
            .find("}}")
            .with_context(|| format!("unclosed `{{{{` in template: {template:?}"))?;
        let expr = remaining[..end].trim();
        remaining = &remaining[end + 2..];

        if let Some(name) = expr.strip_prefix("vars.")
            && !vars.contains_key(name)
        {
            bail!(
                "variable `{name}` is not defined — pass it with `--vars {name}=<value>` or define it in the script's `vars:` section"
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    // iter-105 Theme D: these tests call `std::env::set_var`/`remove_var`, which
    // are `unsafe` on edition 2024.  The crate default is
    // `unsafe_code = "deny"`; scope the allowance to the test module only (each
    // block carries its own `// SAFETY:` note) so production code stays denied.
    #![allow(unsafe_code)]

    use super::*;
    use serde_json::json;

    static EMPTY_POLICY: std::sync::LazyLock<EnvPolicy> =
        std::sync::LazyLock::new(EnvPolicy::default);

    fn ctx<'a>(vars: &'a HashMap<String, String>, results: &'a [Value]) -> VarContext<'a> {
        VarContext {
            vars,
            step_results: results,
            show_secrets: false,
            env_policy: &EMPTY_POLICY,
        }
    }

    fn ctx_with_policy<'a>(
        vars: &'a HashMap<String, String>,
        results: &'a [Value],
        policy: &'a EnvPolicy,
    ) -> VarContext<'a> {
        VarContext {
            vars,
            step_results: results,
            show_secrets: false,
            env_policy: policy,
        }
    }

    #[test]
    fn substitutes_vars() {
        let mut vars = HashMap::new();
        vars.insert("email".to_owned(), "user@example.com".to_owned());
        let ctx = ctx(&vars, &[]);
        let result = substitute("hello {{vars.email}}", &ctx).unwrap();
        assert_eq!(result, "hello user@example.com");
    }

    #[test]
    fn missing_var_errors() {
        let vars = HashMap::new();
        let ctx = ctx(&vars, &[]);
        let err = substitute("{{vars.missing}}", &ctx).unwrap_err();
        assert!(format!("{err:#}").contains("missing"), "{err:#}");
    }

    #[test]
    fn step_result_reference() {
        let vars = HashMap::new();
        // step_results entries are wrapped as {"results": ...}
        let results = vec![json!({"results": {"url": "https://example.com"}})];
        let ctx = ctx(&vars, &results);
        let result = substitute("was: {{steps[1].results.url}}", &ctx).unwrap();
        assert_eq!(result, "was: https://example.com");
    }

    #[test]
    fn step_zero_index_errors() {
        let vars = HashMap::new();
        let ctx = ctx(&vars, &[]);
        let err = substitute("{{steps[0].url}}", &ctx).unwrap_err();
        assert!(format!("{err:#}").contains("1-based"), "{err:#}");
    }

    #[test]
    fn secret_detection() {
        assert!(is_secret_name("password"));
        assert!(is_secret_name("user_password"));
        assert!(is_secret_name("api_token"));
        assert!(is_secret_name("SECRET_KEY"));
        assert!(!is_secret_name("email"));
        assert!(!is_secret_name("username"));
    }

    #[test]
    fn redacts_secret_fields() {
        let secrets: HashMap<String, String> = [("password".to_owned(), "hunter2".to_owned())]
            .into_iter()
            .collect();
        let val = json!({"password": "hunter2", "email": "user@example.com"});
        let redacted = redact_secrets(&val, &secrets, false);
        assert_eq!(redacted["password"], "[REDACTED]");
        assert_eq!(redacted["email"], "user@example.com");
    }

    #[test]
    fn show_secrets_bypasses_redaction() {
        let secrets: HashMap<String, String> = [("password".to_owned(), "hunter2".to_owned())]
            .into_iter()
            .collect();
        let val = json!({"password": "hunter2"});
        let out = redact_secrets(&val, &secrets, true);
        assert_eq!(out["password"], "hunter2");
    }

    #[test]
    fn check_undefined_vars_catches_missing() {
        let vars = HashMap::new();
        let err = check_undefined_vars("hello {{vars.email}}", &vars).unwrap_err();
        assert!(err.to_string().contains("email"), "{err}");
    }

    #[test]
    fn check_undefined_vars_passes_when_defined() {
        let mut vars = HashMap::new();
        vars.insert("email".to_owned(), "x@y.com".to_owned());
        check_undefined_vars("hello {{vars.email}}", &vars).unwrap();
    }

    #[test]
    fn env_substitution_rejects_unallowed() {
        // SAFETY: tests rely on a uniquely-named env var.
        unsafe {
            std::env::set_var("FFRDP_TEST_VAR_67A", "secret-value");
        }
        let vars = HashMap::new();
        let ctx = ctx(&vars, &[]);
        let err = substitute("{{env.FFRDP_TEST_VAR_67A}}", &ctx).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("not in allowlist") && msg.contains("--allow-env"),
            "unexpected error: {msg}"
        );
        unsafe {
            std::env::remove_var("FFRDP_TEST_VAR_67A");
        }
    }

    #[test]
    fn env_substitution_allowlist_works() {
        unsafe {
            std::env::set_var("FFRDP_TEST_FOO_67", "foo-val");
            std::env::set_var("FFRDP_TEST_BAR_67", "bar-val");
        }
        let vars = HashMap::new();
        let policy = EnvPolicy::from_names(["FFRDP_TEST_FOO_67"]);
        let ctx = ctx_with_policy(&vars, &[], &policy);
        let out = substitute("hello {{env.FFRDP_TEST_FOO_67}}", &ctx).unwrap();
        assert_eq!(out, "hello foo-val");

        let err = substitute("{{env.FFRDP_TEST_BAR_67}}", &ctx).unwrap_err();
        assert!(format!("{err:#}").contains("not in allowlist"));
        unsafe {
            std::env::remove_var("FFRDP_TEST_FOO_67");
            std::env::remove_var("FFRDP_TEST_BAR_67");
        }
    }

    #[test]
    fn env_substitution_refuses_secret_names() {
        // Even an explicit allowlist entry must not unlock secret-shaped names.
        unsafe {
            std::env::set_var("AWS_SECRET_ACCESS_KEY", "leak-me");
        }
        let vars = HashMap::new();
        let policy = EnvPolicy::from_names(["AWS_SECRET_ACCESS_KEY"]);
        let ctx = ctx_with_policy(&vars, &[], &policy);
        let err = substitute("{{env.AWS_SECRET_ACCESS_KEY}}", &ctx).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("secret-name pattern"), "{msg}");
        unsafe {
            std::env::remove_var("AWS_SECRET_ACCESS_KEY");
        }
    }

    #[test]
    fn env_substitution_allows_safe_defaults() {
        // Save & restore LANG so this test doesn't bleed into others that
        // run in parallel and may read the variable.
        let prev = std::env::var("LANG").ok();
        unsafe {
            std::env::set_var("LANG", "C.UTF-8");
        }
        let vars = HashMap::new();
        let ctx = ctx(&vars, &[]);
        let out = substitute("{{env.LANG}}", &ctx).unwrap();
        assert_eq!(out, "C.UTF-8");
        unsafe {
            match prev {
                Some(v) => std::env::set_var("LANG", v),
                None => std::env::remove_var("LANG"),
            }
        }
    }

    #[test]
    fn no_placeholders_passes_through() {
        let vars = HashMap::new();
        let ctx = ctx(&vars, &[]);
        let result = substitute("plain text", &ctx).unwrap();
        assert_eq!(result, "plain text");
    }
}
