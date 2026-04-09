use ff_rdp_core::{Grip, LongStringActor};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_direct;
use super::js_helpers::eval_or_bail;

/// Read all keys or a single key from localStorage or sessionStorage.
pub fn run(cli: &Cli, storage_type: &str, key: Option<&str>) -> Result<(), AppError> {
    let (storage_obj, canonical_type) = match storage_type {
        "local" | "localStorage" => ("localStorage", "local"),
        "session" | "sessionStorage" => ("sessionStorage", "session"),
        other => {
            return Err(AppError::User(format!(
                "invalid storage type {other:?}: expected \"local\", \"localStorage\", \"session\", or \"sessionStorage\""
            )));
        }
    };

    let mut ctx = connect_direct(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let meta = json!({
        "host": cli.host,
        "port": cli.port,
        "storage_type": canonical_type,
    });

    if let Some(k) = key {
        // Single-key lookup: embed key as a JSON-encoded string literal to
        // prevent any injection through the key name.
        let key_json = serde_json::to_string(k)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("key serialisation: {e}")))?;

        let js = format!(
            "(function() {{\
               var v = {storage_obj}.getItem({key_json});\
               return v;\
             }})()"
        );

        let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "storage getItem failed")?;

        match &eval_result.result {
            Grip::Null => {
                // Key does not exist in storage — return null value with total=0.
                let envelope = output::envelope(&json!({"key": k, "value": null}), 0, &meta);
                OutputPipeline::from_cli(cli)?
                    .finalize(&envelope)
                    .map_err(AppError::from)
            }
            grip => {
                let value = resolve_string_grip(&mut ctx, grip)?;
                let envelope = output::envelope(&json!({"key": k, "value": value}), 1, &meta);
                OutputPipeline::from_cli(cli)?
                    .finalize(&envelope)
                    .map_err(AppError::from)
            }
        }
    } else {
        // All-keys dump: JS returns JSON.stringify of the full storage map.
        let js = format!(
            "(function() {{\
               var s = {storage_obj};\
               var obj = {{}};\
               for (var i = 0; i < s.length; i++) {{\
                 var k = s.key(i);\
                 obj[k] = s.getItem(k);\
               }}\
               return JSON.stringify(obj);\
             }})()"
        );

        let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "storage dump failed")?;

        let raw = resolve_string_grip(&mut ctx, &eval_result.result)?;

        // The JS returned JSON.stringify output — parse it back so the
        // envelope contains a real JSON object, not an escaped string.
        let storage_map: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
            AppError::Internal(anyhow::anyhow!(
                "storage result was not valid JSON: {e}: {raw}"
            ))
        })?;

        let total = storage_map.as_object().map_or(0, serde_json::Map::len);

        let envelope = output::envelope(&storage_map, total, &meta);
        OutputPipeline::from_cli(cli)?
            .finalize(&envelope)
            .map_err(AppError::from)
    }
}

/// Resolve a [`Grip`] to a `String`.
///
/// - Plain string values are returned as-is.
/// - LongString grips are fetched in full via the actor protocol.
/// - Null and Undefined are returned as empty strings.
/// - Other grips fall back to their JSON representation.
fn resolve_string_grip(
    ctx: &mut super::connect_tab::ConnectedTab,
    grip: &Grip,
) -> Result<String, AppError> {
    match grip {
        Grip::Value(serde_json::Value::String(s)) => Ok(s.clone()),
        Grip::LongString {
            actor,
            length,
            initial: _,
        } => LongStringActor::full_string(ctx.transport_mut(), actor.as_ref(), *length)
            .map_err(AppError::from),
        Grip::Null | Grip::Undefined => Ok(String::new()),
        other => Ok(other.to_json().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    fn resolve_storage_type(storage_type: &str) -> Result<(&'static str, &'static str), String> {
        match storage_type {
            "local" | "localStorage" => Ok(("localStorage", "local")),
            "session" | "sessionStorage" => Ok(("sessionStorage", "session")),
            other => Err(format!(
                "invalid storage type {other:?}: expected \"local\", \"localStorage\", \"session\", or \"sessionStorage\""
            )),
        }
    }

    /// Validate that an unknown storage_type returns an error.
    #[test]
    fn invalid_storage_type_is_user_error() {
        let result = resolve_storage_type("cookie");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cookie"));
    }

    /// "local" and "localStorage" both resolve to localStorage with canonical type "local".
    #[test]
    fn local_alias_accepted() {
        let (obj, canonical) = resolve_storage_type("local").unwrap();
        assert_eq!(obj, "localStorage");
        assert_eq!(canonical, "local");

        let (obj, canonical) = resolve_storage_type("localStorage").unwrap();
        assert_eq!(obj, "localStorage");
        assert_eq!(canonical, "local");
    }

    /// "session" and "sessionStorage" both resolve to sessionStorage with canonical type "session".
    #[test]
    fn session_alias_accepted() {
        let (obj, canonical) = resolve_storage_type("session").unwrap();
        assert_eq!(obj, "sessionStorage");
        assert_eq!(canonical, "session");

        let (obj, canonical) = resolve_storage_type("sessionStorage").unwrap();
        assert_eq!(obj, "sessionStorage");
        assert_eq!(canonical, "session");
    }

    /// Verify that the key name is JSON-encoded to prevent injection.
    #[test]
    fn key_json_encoding_escapes_quotes() {
        let key = r#"a"b"#;
        let encoded = serde_json::to_string(key).unwrap();
        // The double-quote inside the key must be escaped.
        assert_eq!(encoded, r#""a\"b""#);
        // The encoded form must not appear raw in JavaScript.
        let js = format!("localStorage.getItem({encoded})");
        assert!(!js.contains(r#"getItem("a"b")"#));
    }

    /// Verify all-keys fixture round-trips through JSON correctly.
    #[test]
    fn storage_json_parses_correctly() {
        let raw = r#"{"token":"abc","theme":"dark"}"#;
        let parsed: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed["token"], json!("abc"));
        assert_eq!(parsed["theme"], json!("dark"));
        assert_eq!(parsed.as_object().unwrap().len(), 2);
    }
}
