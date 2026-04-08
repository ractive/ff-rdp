use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::{ActorId, Grip};

/// Result of a JavaScript evaluation.
#[derive(Debug)]
pub struct EvalResult {
    /// The evaluated result as a grip (present when no exception occurred).
    pub result: Grip,
    /// Exception grip, if the evaluation threw an error.
    pub exception: Option<EvalException>,
    /// Server-assigned timestamp for the evaluation.
    pub timestamp: Option<u64>,
}

/// Information about a JS evaluation exception.
#[derive(Debug)]
pub struct EvalException {
    /// The exception value as a grip.
    pub value: Grip,
    /// Human-readable error message extracted from the exception preview.
    pub message: Option<String>,
}

/// A parsed console message from `getCachedMessages`.
#[derive(Debug, Clone)]
pub struct ConsoleMessage {
    /// Message level: "log", "warn", "error", "info", "debug", "trace".
    pub level: String,
    /// The message text (joined from arguments).
    pub message: String,
    /// Source file where the message was emitted.
    pub source: String,
    /// Line number in the source file.
    pub line: u32,
    /// Column number in the source file.
    pub column: u32,
    /// Timestamp in milliseconds since epoch.
    pub timestamp: f64,
}

/// Operations on a WebConsole actor (JavaScript evaluation, message retrieval).
pub struct WebConsoleActor;

impl WebConsoleActor {
    /// Start listeners for console events.
    ///
    /// Valid listener types: `"PageError"`, `"ConsoleAPI"`.
    pub fn start_listeners(
        transport: &mut RdpTransport,
        console_actor: &ActorId,
        listeners: &[&str],
    ) -> Result<Value, ProtocolError> {
        let types: Vec<Value> = listeners.iter().map(|l| json!(l)).collect();
        let params = json!({ "listeners": types });
        actor_request(
            transport,
            console_actor.as_ref(),
            "startListeners",
            Some(&params),
        )
    }

    /// Retrieve cached console messages from the tab.
    ///
    /// Returns a list of parsed console messages. Pass message types like
    /// `["PageError", "ConsoleAPI"]` to control which message types are included.
    pub fn get_cached_messages(
        transport: &mut RdpTransport,
        console_actor: &ActorId,
        message_types: &[&str],
    ) -> Result<Vec<ConsoleMessage>, ProtocolError> {
        let types: Vec<Value> = message_types.iter().map(|t| json!(t)).collect();
        let params = json!({ "messageTypes": types });
        let response = actor_request(
            transport,
            console_actor.as_ref(),
            "getCachedMessages",
            Some(&params),
        )?;

        let messages = response
            .get("messages")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|wrapper| {
                        parse_console_message(wrapper).or_else(|| parse_page_error(wrapper))
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(messages)
    }

    /// Evaluate a JavaScript expression asynchronously.
    ///
    /// Sends `evaluateJSAsync` to the console actor, captures the `resultID`
    /// from the immediate response, then reads messages until the matching
    /// `evaluationResult` event arrives.
    pub fn evaluate_js_async(
        transport: &mut RdpTransport,
        console_actor: &ActorId,
        text: &str,
    ) -> Result<EvalResult, ProtocolError> {
        let params = json!({
            "text": text,
            "eager": false,
        });

        // Send the eval request and get the immediate response with resultID.
        let immediate = actor_request(
            transport,
            console_actor.as_ref(),
            "evaluateJSAsync",
            Some(&params),
        )?;

        let result_id = immediate
            .get("resultID")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProtocolError::InvalidPacket(
                    "evaluateJSAsync response missing 'resultID' field".into(),
                )
            })?
            .to_owned();

        // Read messages until we find the evaluationResult with matching resultID.
        // Safety: the underlying socket has a read timeout (set during connect), so
        // this loop will eventually fail with a timeout error if Firefox stops responding.
        loop {
            let msg = transport.recv()?;

            let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
            let msg_result_id = msg
                .get("resultID")
                .and_then(Value::as_str)
                .unwrap_or_default();

            if msg_type == "evaluationResult" && msg_result_id == result_id {
                return Ok(Self::parse_eval_result(&msg));
            }

            // Other messages (e.g. tabNavigated events) are discarded while
            // waiting for the eval result.
        }
    }

    fn parse_eval_result(msg: &Value) -> EvalResult {
        // Firefox sends timestamp as a float (milliseconds since epoch).
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let timestamp = msg
            .get("timestamp")
            .and_then(Value::as_f64)
            .map(|f| f as u64);

        // Check for exception first.
        let exception = match msg.get("exception") {
            Some(exc) if !exc.is_null() => {
                let grip = Grip::from_result_value(exc);
                let message = Self::extract_exception_message(exc);
                Some(EvalException {
                    value: grip,
                    message,
                })
            }
            _ => None,
        };

        // Parse the result grip (may be null when exception occurred).
        let result_value = msg.get("result").unwrap_or(&Value::Null);
        let result = Grip::from_result_value(result_value);

        EvalResult {
            result,
            exception,
            timestamp,
        }
    }

    /// Try to extract a human-readable message from an exception grip.
    fn extract_exception_message(exc: &Value) -> Option<String> {
        // Exception objects typically have a preview with a message field.
        exc.get("preview")
            .and_then(|p| p.get("message"))
            .and_then(Value::as_str)
            .map(String::from)
            .or_else(|| {
                // Some exceptions are plain strings.
                exc.as_str().map(String::from)
            })
    }
}

/// Parse a direct console notification pushed by the console actor when
/// `startListeners` is active.
///
/// Firefox 149+ pushes `consoleAPICall` and `pageError` events directly to the
/// console actor connection in addition to (or instead of) routing them through
/// the Watcher's `resources-available-array` stream.  This matters in particular
/// when a `console.log()` is executed via `evaluateJSAsync` — the message arrives
/// as a direct push notification on the console actor rather than as a watcher
/// resource event.
///
/// Returns `None` when the message is not a console notification.
pub fn parse_console_notification(msg: &Value) -> Option<ConsoleMessage> {
    let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
    match msg_type {
        "consoleAPICall" => {
            // Direct consoleAPICall: { "type": "consoleAPICall", "message": { ... }, "from": "..." }
            let wrapper = msg;
            parse_console_message(wrapper)
        }
        "pageError" => {
            // Direct pageError: { "type": "pageError", "pageError": { ... }, "from": "..." }
            let wrapper = msg;
            parse_page_error(wrapper)
        }
        _ => None,
    }
}

/// Parse a `pageError` wrapper from `getCachedMessages`.
///
/// Firefox emits `{ "pageError": { ... }, "type": "pageError" }` entries
/// alongside `consoleAPICall` entries. The inner object uses different field
/// names than the `consoleAPICall` format.
fn parse_page_error(wrapper: &Value) -> Option<ConsoleMessage> {
    let err = wrapper.get("pageError")?;

    let level = "error".to_owned();
    let message = err
        .get("errorMessage")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let source = err
        .get("sourceName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let line = err
        .get("lineNumber")
        .and_then(Value::as_u64)
        .unwrap_or_default() as u32;

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let column = err
        .get("columnNumber")
        .and_then(Value::as_u64)
        .unwrap_or_default() as u32;

    let timestamp = err
        .get("timeStamp")
        .and_then(Value::as_f64)
        .unwrap_or_default();

    Some(ConsoleMessage {
        level,
        message,
        source,
        line,
        column,
        timestamp,
    })
}

/// Parse a single console message from the `getCachedMessages` response.
///
/// Firefox wraps messages in `{ "message": { ... }, "type": "consoleAPICall" }`.
/// The inner `message` object contains `level`, `arguments`, `filename`, etc.
fn parse_console_message(wrapper: &Value) -> Option<ConsoleMessage> {
    let msg = wrapper.get("message")?;

    let level = msg
        .get("level")
        .and_then(Value::as_str)
        .unwrap_or("log")
        .to_owned();

    // Arguments is an array of values; join them as strings.
    let message = msg
        .get("arguments")
        .and_then(Value::as_array)
        .map(|args| {
            args.iter()
                .map(|a| match a.as_str() {
                    Some(s) => s.to_owned(),
                    None => a.to_string(),
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();

    let source = msg
        .get("filename")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let line = msg
        .get("lineNumber")
        .and_then(Value::as_u64)
        .unwrap_or_default() as u32;

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let column = msg
        .get("columnNumber")
        .and_then(Value::as_u64)
        .unwrap_or_default() as u32;

    let timestamp = msg
        .get("timeStamp")
        .and_then(Value::as_f64)
        .unwrap_or_default();

    Some(ConsoleMessage {
        level,
        message,
        source,
        line,
        column,
        timestamp,
    })
}

#[cfg(test)]
#[allow(clippy::unreadable_literal)]
mod tests {
    use serde_json::json;

    use super::*;

    // Unit tests use the real Firefox wire format (recorded from headless Firefox).

    #[test]
    fn parse_eval_result_string() {
        let msg = json!({
            "from": "server1.conn0.child2/consoleActor3",
            "hasException": false,
            "input": "document.title",
            "result": "Example Domain",
            "resultID": "1775437183977.373-0",
            "startTime": 1_775_437_183_977.373_f64,
            "timestamp": 1_775_437_183_980.721_f64,
            "type": "evaluationResult"
        });
        let result = WebConsoleActor::parse_eval_result(&msg);
        assert_eq!(result.result, Grip::Value(json!("Example Domain")));
        assert!(result.exception.is_none());
        assert_eq!(result.timestamp, Some(1_775_437_183_980));
    }

    #[test]
    fn parse_eval_result_number() {
        let msg = json!({
            "from": "server1.conn0.child2/consoleActor3",
            "hasException": false,
            "input": "1 + 41",
            "result": 42,
            "resultID": "1775437183981.449-1",
            "startTime": 1_775_437_183_981.449_f64,
            "timestamp": 1_775_437_183_981.697_f64,
            "type": "evaluationResult"
        });
        let result = WebConsoleActor::parse_eval_result(&msg);
        assert_eq!(result.result, Grip::Value(json!(42)));
        assert!(result.exception.is_none());
    }

    #[test]
    fn parse_eval_result_undefined() {
        let msg = json!({
            "from": "server1.conn0.child2/consoleActor3",
            "hasException": false,
            "input": "undefined",
            "result": {"type": "undefined"},
            "resultID": "1775437183982.111-2",
            "startTime": 1_775_437_183_982.111_f64,
            "timestamp": 1_775_437_183_982.286_f64,
            "type": "evaluationResult"
        });
        let result = WebConsoleActor::parse_eval_result(&msg);
        assert_eq!(result.result, Grip::Undefined);
        assert!(result.exception.is_none());
    }

    #[test]
    fn parse_eval_result_object() {
        let msg = json!({
            "from": "server1.conn0.child2/consoleActor3",
            "hasException": false,
            "input": "({a: 1, b: [2,3]})",
            "result": {
                "type": "object",
                "actor": "server1.conn0.child2/obj19",
                "class": "Object",
                "extensible": true,
                "frozen": false,
                "isError": false,
                "ownPropertyLength": 2,
                "preview": {
                    "kind": "Object",
                    "ownProperties": {},
                    "ownPropertiesLength": 2
                },
                "sealed": false
            },
            "resultID": "1775437183982.764-3",
            "startTime": 1_775_437_183_982.764_f64,
            "timestamp": 1_775_437_183_987.678_f64,
            "type": "evaluationResult"
        });
        let result = WebConsoleActor::parse_eval_result(&msg);
        match &result.result {
            Grip::Object { class, .. } => assert_eq!(class, "Object"),
            other => panic!("expected Grip::Object, got {other:?}"),
        }
    }

    #[test]
    fn parse_eval_result_with_exception() {
        // Real Firefox format: exception at top level, result is {type: "undefined"}
        let msg = json!({
            "exception": {
                "actor": "server1.conn0.child2/obj21",
                "class": "Error",
                "extensible": true,
                "frozen": false,
                "isError": true,
                "ownPropertyLength": 4,
                "preview": {
                    "columnNumber": 7,
                    "fileName": "debugger eval code",
                    "kind": "Error",
                    "lineNumber": 1,
                    "message": "test error",
                    "name": "Error",
                    "stack": "@debugger eval code:1:7\n"
                },
                "sealed": false,
                "type": "object"
            },
            "exceptionMessage": "Error: test error",
            "from": "server1.conn0.child2/consoleActor3",
            "hasException": true,
            "input": "throw new Error('test error')",
            "result": {"type": "undefined"},
            "resultID": "1775437183988.612-4",
            "startTime": 1_775_437_183_988.612_f64,
            "timestamp": 1_775_437_183_990.629_f64,
            "type": "evaluationResult"
        });
        let result = WebConsoleActor::parse_eval_result(&msg);
        let exc = result.exception.as_ref().unwrap();
        assert_eq!(exc.message.as_deref(), Some("test error"));
        match &exc.value {
            Grip::Object { class, .. } => assert_eq!(class, "Error"),
            other => panic!("expected Grip::Object, got {other:?}"),
        }
    }

    #[test]
    fn parse_eval_result_long_string() {
        let msg = json!({
            "from": "server1.conn0.child2/consoleActor3",
            "hasException": false,
            "input": "'x'.repeat(50000)",
            "result": {
                "actor": "server1.conn0.child2/longstractor22",
                "initial": "xxxx",
                "length": 50000,
                "type": "longString"
            },
            "resultID": "1775437183991.461-5",
            "startTime": 1_775_437_183_991.461_f64,
            "timestamp": 1_775_437_183_991.851_f64,
            "type": "evaluationResult"
        });
        let result = WebConsoleActor::parse_eval_result(&msg);
        match &result.result {
            Grip::LongString {
                length, initial, ..
            } => {
                assert_eq!(*length, 50000);
                assert_eq!(initial, "xxxx");
            }
            other => panic!("expected Grip::LongString, got {other:?}"),
        }
    }

    #[test]
    fn parse_eval_result_string_exception() {
        // Edge case: some exceptions are plain strings, not objects.
        let msg = json!({
            "from": "console1",
            "type": "evaluationResult",
            "resultID": "id1",
            "result": {"type": "undefined"},
            "hasException": true,
            "exception": "uncaught string error"
        });
        let result = WebConsoleActor::parse_eval_result(&msg);
        let exc = result.exception.as_ref().unwrap();
        assert_eq!(exc.message.as_deref(), Some("uncaught string error"));
    }

    // --- Console message parsing tests (recorded from Firefox) ---

    #[test]
    fn parse_console_message_log() {
        let wrapper = json!({
            "message": {
                "arguments": ["hello from test"],
                "category": "webdev",
                "chromeContext": false,
                "columnNumber": 9,
                "filename": "debugger eval code",
                "innerWindowID": 21474836481_u64,
                "level": "log",
                "lineNumber": 1,
                "sourceId": null,
                "timeStamp": 1775439071165.699
            },
            "type": "consoleAPICall"
        });

        let msg = parse_console_message(&wrapper).unwrap();
        assert_eq!(msg.level, "log");
        assert_eq!(msg.message, "hello from test");
        assert_eq!(msg.source, "debugger eval code");
        assert_eq!(msg.line, 1);
        assert_eq!(msg.column, 9);
        assert!(msg.timestamp > 0.0);
    }

    #[test]
    fn parse_console_message_warn() {
        let wrapper = json!({
            "message": {
                "arguments": ["warning msg"],
                "category": "webdev",
                "columnNumber": 41,
                "filename": "https://example.com/app.js",
                "level": "warn",
                "lineNumber": 15,
                "timeStamp": 1775439071166.011
            },
            "type": "consoleAPICall"
        });

        let msg = parse_console_message(&wrapper).unwrap();
        assert_eq!(msg.level, "warn");
        assert_eq!(msg.message, "warning msg");
        assert_eq!(msg.source, "https://example.com/app.js");
        assert_eq!(msg.line, 15);
    }

    #[test]
    fn parse_console_message_error_with_stacktrace() {
        let wrapper = json!({
            "message": {
                "arguments": ["error msg"],
                "category": "webdev",
                "columnNumber": 70,
                "filename": "debugger eval code",
                "level": "error",
                "lineNumber": 1,
                "stacktrace": [{"columnNumber": 70, "filename": "debugger eval code", "functionName": "", "lineNumber": 1}],
                "timeStamp": 1775439071166.021
            },
            "type": "consoleAPICall"
        });

        let msg = parse_console_message(&wrapper).unwrap();
        assert_eq!(msg.level, "error");
        assert_eq!(msg.message, "error msg");
    }

    #[test]
    fn parse_console_message_multiple_arguments() {
        let wrapper = json!({
            "message": {
                "arguments": ["count:", 42, true],
                "level": "log",
                "filename": "test.js",
                "lineNumber": 5,
                "columnNumber": 1,
                "timeStamp": 1000.0
            },
            "type": "consoleAPICall"
        });

        let msg = parse_console_message(&wrapper).unwrap();
        assert_eq!(msg.message, "count: 42 true");
    }

    #[test]
    fn parse_console_message_returns_none_for_missing_message() {
        let wrapper = json!({"type": "consoleAPICall"});
        assert!(parse_console_message(&wrapper).is_none());
    }

    // --- pageError parsing tests ---

    #[test]
    fn parse_page_error_basic() {
        let wrapper = json!({
            "pageError": {
                "errorMessage": "ReferenceError: foo is not defined",
                "sourceName": "https://example.com/app.js",
                "lineNumber": 42,
                "columnNumber": 5,
                "timeStamp": 1775439071166.0
            },
            "type": "pageError"
        });
        let msg = parse_page_error(&wrapper).unwrap();
        assert_eq!(msg.level, "error");
        assert_eq!(msg.message, "ReferenceError: foo is not defined");
        assert_eq!(msg.source, "https://example.com/app.js");
        assert_eq!(msg.line, 42);
        assert_eq!(msg.column, 5);
    }

    #[test]
    fn parse_page_error_missing_fields() {
        let wrapper = json!({
            "pageError": {},
            "type": "pageError"
        });
        let msg = parse_page_error(&wrapper).unwrap();
        assert_eq!(msg.level, "error");
        assert_eq!(msg.message, "");
        assert_eq!(msg.source, "");
    }

    #[test]
    fn parse_page_error_returns_none_for_console_api() {
        let wrapper = json!({
            "message": {"level": "log", "arguments": ["test"]},
            "type": "consoleAPICall"
        });
        assert!(parse_page_error(&wrapper).is_none());
    }

    // --- parse_console_notification tests ---

    #[test]
    fn parse_console_notification_consoleapicall() {
        // Direct consoleAPICall push from Firefox with startListeners active.
        let msg = json!({
            "type": "consoleAPICall",
            "from": "server1.conn0.child2/consoleActor3",
            "message": {
                "arguments": ["hello from eval"],
                "level": "log",
                "filename": "debugger eval code",
                "lineNumber": 1,
                "columnNumber": 9,
                "timeStamp": 1775439071165.699_f64
            }
        });
        let result = super::parse_console_notification(&msg).unwrap();
        assert_eq!(result.level, "log");
        assert_eq!(result.message, "hello from eval");
        assert_eq!(result.source, "debugger eval code");
        assert_eq!(result.line, 1);
    }

    #[test]
    fn parse_console_notification_pageerror() {
        // Direct pageError push from Firefox.
        let msg = json!({
            "type": "pageError",
            "from": "server1.conn0.child2/consoleActor3",
            "pageError": {
                "errorMessage": "ReferenceError: x is not defined",
                "sourceName": "https://example.com/app.js",
                "lineNumber": 10,
                "columnNumber": 3,
                "timeStamp": 1775439071200.0_f64
            }
        });
        let result = super::parse_console_notification(&msg).unwrap();
        assert_eq!(result.level, "error");
        assert_eq!(result.message, "ReferenceError: x is not defined");
        assert_eq!(result.source, "https://example.com/app.js");
        assert_eq!(result.line, 10);
    }

    #[test]
    fn parse_console_notification_ignores_unrelated_type() {
        let msg = json!({
            "type": "evaluationResult",
            "from": "server1.conn0.child2/consoleActor3",
            "result": "some value"
        });
        assert!(super::parse_console_notification(&msg).is_none());
    }

    #[test]
    fn parse_console_notification_ignores_resources_available_array() {
        let msg = json!({
            "type": "resources-available-array",
            "from": "server1.conn0.watcher4",
            "array": []
        });
        assert!(super::parse_console_notification(&msg).is_none());
    }
}
