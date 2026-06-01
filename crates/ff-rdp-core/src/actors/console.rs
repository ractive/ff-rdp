use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::{RdpTransport, recv_event_from, recv_reply_from};
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

/// Optional scoping for [`WebConsoleActor::evaluate_js_async_scoped`].
///
/// Mirrors the spec-declared `evaluateJSAsync` request fields at
/// `devtools/shared/specs/webconsole.js:149-164` (Firefox 149+).  The
/// server consults these to scope the eval against a specific iframe
/// (`frame_actor`), pre-bind `$0` to a selected DOM node
/// (`selected_node_actor`), or pin the inner-window ID when multiple
/// windows share the same actor — see
/// `devtools/server/actors/webconsole.js:761-870` for the consuming code.
#[derive(Debug, Default, Clone)]
pub struct EvaluateScope {
    /// Frame actor — eval inside this iframe rather than the top-level
    /// document.
    pub frame_actor: Option<ActorId>,
    /// Pre-bind `$0` to the given DOM node actor.
    pub selected_node_actor: Option<ActorId>,
    /// Constrain the eval to a specific inner window.
    pub inner_window_id: Option<u64>,
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
    ///
    /// Per the Firefox RDP protocol, replies have no `type` field while push
    /// events (consoleAPICall, pageError, tabNavigated, …) always carry one.
    /// The two loops below use this invariant to skip push events cleanly
    /// without naming specific event types.
    ///
    /// Returns [`ProtocolError::EvalNavigatedDuringEval`] immediately if a
    /// `tabNavigated` or `willNavigate` event is received from the evaluated
    /// console actor while waiting for the eval result — indicating the page
    /// navigated away and the result will never arrive.  Navigation events
    /// originating from other actors on the same connection are ignored.
    pub fn evaluate_js_async(
        transport: &mut RdpTransport,
        console_actor: &ActorId,
        text: &str,
    ) -> Result<EvalResult, ProtocolError> {
        Self::evaluate_js_async_scoped(transport, console_actor, text, None)
    }

    /// Like [`evaluate_js_async`](Self::evaluate_js_async) but accepts an
    /// optional [`EvaluateScope`] carrying the spec-declared
    /// `frameActor` / `selectedNodeActor` / `innerWindowID` fields
    /// (`devtools/shared/specs/webconsole.js:149-164`).
    ///
    /// # CSP bypass — iter-93 finding
    ///
    /// This method is the "console sandbox" path described in iter-93.
    /// Firefox routes the text through `Debugger.evalInGlobal` inside
    /// `devtools/server/actors/webconsole/eval-with-debugger.js:119-247`,
    /// which is **not** subject to the page's Content Security Policy.
    ///
    /// Page CSP restricts `eval()` when called from within a page script, but
    /// `Debugger.evalInGlobal` operates at the Debugger API level — outside the
    /// page's scripting environment — so it bypasses that restriction entirely.
    ///
    /// The consuming server code that dispatches to `eval-with-debugger.js` is
    /// in `devtools/server/actors/webconsole.js:761-900`.  The important
    /// consequence for ff-rdp: the text sent in `evaluateJSAsync.text` must
    /// NOT itself call `eval()`, because a page-`eval()` call from within the
    /// evaluated script IS subject to page CSP.  `build_script` in
    /// `crates/ff-rdp-cli/src/commands/eval.rs` is responsible for ensuring
    /// no `eval()` wrapper is emitted.
    pub fn evaluate_js_async_scoped(
        transport: &mut RdpTransport,
        console_actor: &ActorId,
        text: &str,
        scope: Option<&EvaluateScope>,
    ) -> Result<EvalResult, ProtocolError> {
        let mut request = json!({
            "to": console_actor.as_ref(),
            "type": "evaluateJSAsync",
            "text": text,
            "eager": false,
            // mapped.await instructs Firefox to await any Promise returned by the
            // evaluated expression before surfacing the result. Without this flag,
            // async expressions resolve to a Promise grip rather than the awaited
            // value, and CSP-restricted pages may return a pending-promise error
            // instead of the actual JS result.
            "mapped": { "await": true },
        });
        if let Some(scope) = scope
            && let Some(obj) = request.as_object_mut()
        {
            if let Some(ref frame) = scope.frame_actor {
                obj.insert("frameActor".to_owned(), json!(frame.as_ref()));
            }
            if let Some(ref node) = scope.selected_node_actor {
                obj.insert("selectedNodeActor".to_owned(), json!(node.as_ref()));
            }
            if let Some(iwid) = scope.inner_window_id {
                obj.insert("innerWindowID".to_owned(), json!(iwid));
            }
        }
        transport.send(&request)?;

        // The immediate ack is a reply (no `type` field) from the console
        // actor; push events arriving in the gap are forwarded to the
        // transport's event sink by `recv_reply_from`.
        let immediate = recv_reply_from(transport, console_actor.as_ref())?;

        let result_id = immediate
            .get("resultID")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProtocolError::InvalidPacket(
                    "evaluateJSAsync response missing 'resultID' field".into(),
                )
            })?
            .to_owned();

        // Wait for the matching `evaluationResult`. The predicate also fires
        // [`ProtocolError::EvalNavigatedDuringEval`] via a sentinel return when
        // the page navigates away — we use a local control-flow flag because
        // `recv_event_from` only signals "found it" via the predicate.
        //
        // Safety: the underlying socket has a read timeout (set during connect),
        // so the helper will fail with a timeout if Firefox stops responding.
        let mut nav_aborted = false;
        let msg = recv_event_from(transport, console_actor.as_ref(), |m| {
            let msg_type = m.get("type").and_then(Value::as_str).unwrap_or_default();
            if msg_type == "tabNavigated" || msg_type == "willNavigate" {
                nav_aborted = true;
                return true;
            }
            let msg_result_id = m
                .get("resultID")
                .and_then(Value::as_str)
                .unwrap_or_default();
            msg_type == "evaluationResult" && msg_result_id == result_id
        })?;

        if nav_aborted {
            return Err(ProtocolError::EvalNavigatedDuringEval);
        }
        Ok(Self::parse_eval_result(&msg))
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

    // --- evaluate_js_async navigation-abort tests ---

    use crate::transport::{RdpTransport, encode_frame, recv_from as transport_recv_from};
    use std::io::{BufReader, Write};
    use std::net::{TcpListener, TcpStream};

    fn make_transport_pair() -> (RdpTransport, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server_stream, _) = listener.accept().unwrap();
        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        let transport = RdpTransport::from_parts(reader, writer);
        (transport, server_stream)
    }

    fn send_frame(stream: &TcpStream, msg: &serde_json::Value) {
        let json = serde_json::to_string(msg).unwrap();
        stream
            .try_clone()
            .unwrap()
            .write_all(encode_frame(&json).as_bytes())
            .unwrap();
    }

    #[test]
    fn evaluate_js_async_aborts_on_tab_navigated() {
        let (mut transport, server) = make_transport_pair();
        let console_actor: ActorId = "server1.conn0.child2/consoleActor3".into();

        // Server thread: read the eval request, send ack, then a tabNavigated event.
        let actor_str = console_actor.as_ref().to_owned();
        let srv = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            // Consume the evaluateJSAsync request.
            let _req = transport_recv_from(&mut reader).unwrap();

            // Send the ack (no `type` field — it's a direct reply).
            let ack = json!({"from": &actor_str, "resultID": "test-result-id-1"});
            send_frame(&server, &ack);

            // Send a tabNavigated push event.
            let nav =
                json!({"from": &actor_str, "type": "tabNavigated", "url": "https://example.com/"});
            send_frame(&server, &nav);
        });

        let err = WebConsoleActor::evaluate_js_async(&mut transport, &console_actor, "1 + 1")
            .unwrap_err();

        assert!(
            matches!(err, crate::error::ProtocolError::EvalNavigatedDuringEval),
            "expected EvalNavigatedDuringEval, got {err:?}"
        );

        srv.join().unwrap();
    }

    #[test]
    fn evaluate_js_async_aborts_on_will_navigate() {
        let (mut transport, server) = make_transport_pair();
        let console_actor: ActorId = "server1.conn0.child2/consoleActor3".into();

        let actor_str = console_actor.as_ref().to_owned();
        let srv = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let _req = transport_recv_from(&mut reader).unwrap();

            let ack = json!({"from": &actor_str, "resultID": "test-result-id-2"});
            send_frame(&server, &ack);

            // willNavigate is also a navigation signal.
            let nav = json!({"from": &actor_str, "type": "willNavigate", "url": "about:blank"});
            send_frame(&server, &nav);
        });

        let err = WebConsoleActor::evaluate_js_async(&mut transport, &console_actor, "1 + 1")
            .unwrap_err();

        assert!(
            matches!(err, crate::error::ProtocolError::EvalNavigatedDuringEval),
            "expected EvalNavigatedDuringEval, got {err:?}"
        );

        srv.join().unwrap();
    }

    #[test]
    fn evaluate_js_async_skips_push_events_before_result() {
        let (mut transport, server) = make_transport_pair();
        let console_actor: ActorId = "server1.conn0.child2/consoleActor3".into();

        let actor_str = console_actor.as_ref().to_owned();
        let srv = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let _req = transport_recv_from(&mut reader).unwrap();

            // Ack with no `type`.
            let ack = json!({"from": &actor_str, "resultID": "test-result-id-3"});
            send_frame(&server, &ack);

            // Several push events that must be skipped.
            let push1 = json!({"from": &actor_str, "type": "consoleAPICall", "message": {}});
            send_frame(&server, &push1);
            let push2 = json!({"from": &actor_str, "type": "pageError", "pageError": {}});
            send_frame(&server, &push2);

            // The actual eval result (has `type` — it's a server push, not a reply).
            let result = json!({
                "from": &actor_str,
                "type": "evaluationResult",
                "resultID": "test-result-id-3",
                "result": 42,
                "hasException": false
            });
            send_frame(&server, &result);
        });

        let eval_result =
            WebConsoleActor::evaluate_js_async(&mut transport, &console_actor, "1 + 41").unwrap();

        assert_eq!(eval_result.result, Grip::Value(json!(42)));
        srv.join().unwrap();
    }

    /// AC: `evaluate_scope_serializes_fields` — each EvaluateScope field
    /// appears in the outbound `evaluateJSAsync` request body.
    #[test]
    fn evaluate_scope_serializes_fields() {
        let (mut transport, server) = make_transport_pair();
        let console_actor: ActorId = "server1.conn0.child2/consoleActor3".into();

        let actor_str = console_actor.as_ref().to_owned();
        let srv = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let req = transport_recv_from(&mut reader).unwrap();
            assert_eq!(req["frameActor"], "conn0/frame17");
            assert_eq!(req["selectedNodeActor"], "conn0/node42");
            assert_eq!(req["innerWindowID"], 21_474_836_481_u64);

            let ack = json!({"from": &actor_str, "resultID": "scope-test"});
            send_frame(&server, &ack);
            let result = json!({
                "from": &actor_str,
                "type": "evaluationResult",
                "resultID": "scope-test",
                "result": "ok",
                "hasException": false
            });
            send_frame(&server, &result);
        });

        let scope = EvaluateScope {
            frame_actor: Some("conn0/frame17".into()),
            selected_node_actor: Some("conn0/node42".into()),
            inner_window_id: Some(21_474_836_481),
        };
        let eval_result = WebConsoleActor::evaluate_js_async_scoped(
            &mut transport,
            &console_actor,
            "location.href",
            Some(&scope),
        )
        .unwrap();
        assert_eq!(eval_result.result, Grip::Value(json!("ok")));
        srv.join().unwrap();
    }

    // ── iter-93: console sandbox request shape ───────────────────────────────

    /// AC: `unit_evaluate_js_async_console_scope_request_shape` — the outbound
    /// `evaluateJSAsync` request body must be exactly the field set that lands
    /// in Firefox's Debugger.evalInGlobal path (no `chromeContext`, no
    /// `frameActor`, no `innerWindowID`).
    ///
    /// This test pins the wire format.  If Firefox renames a field, this test
    /// catches the divergence loudly.
    ///
    /// Expected shape (iter-93):
    /// ```json
    /// {"to":"<conn0.console1>","type":"evaluateJSAsync","text":"document.title",
    ///  "eager":false,"mapped":{"await":true}}
    /// ```
    #[test]
    fn unit_evaluate_js_async_console_scope_request_shape() {
        let (mut transport, server) = make_transport_pair();
        let console_actor: ActorId = "conn0.console1".into();

        let actor_str = console_actor.as_ref().to_owned();
        let srv = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            // Capture the outbound request and assert its exact shape.
            let req = transport_recv_from(&mut reader).unwrap();

            // Required fields that land in Debugger.evalInGlobal (iter-93).
            assert_eq!(req["to"], actor_str, "to must be the console actor");
            assert_eq!(req["type"], "evaluateJSAsync");
            assert_eq!(req["text"], "document.title");
            assert_eq!(req["eager"], false);
            assert_eq!(
                req["mapped"]["await"],
                serde_json::Value::Bool(true),
                "mapped.await must be true"
            );
            // No chromeContext field — removed in iter-93 (was deprecated).
            assert!(
                req.get("chromeContext").is_none(),
                "chromeContext must not be present"
            );
            // No scope fields when None is passed.
            assert!(req.get("frameActor").is_none());
            assert!(req.get("innerWindowID").is_none());
            assert!(req.get("selectedNodeActor").is_none());

            let ack = json!({"from": &actor_str, "resultID": "iter93-shape-test"});
            send_frame(&server, &ack);
            let result = json!({
                "from": &actor_str,
                "type": "evaluationResult",
                "resultID": "iter93-shape-test",
                "result": "iter93-csp-fixture",
                "hasException": false
            });
            send_frame(&server, &result);
        });

        let eval_result = WebConsoleActor::evaluate_js_async_scoped(
            &mut transport,
            &console_actor,
            "document.title",
            None,
        )
        .unwrap();
        assert_eq!(eval_result.result, Grip::Value(json!("iter93-csp-fixture")));
        srv.join().unwrap();
    }

    // ── mapped.await field tests ─────────────────────────────────────────────

    /// Verify that `evaluate_js_async` sends `mapped: { await: true }` so that
    /// Firefox resolves Promises before returning the result (Theme C, iter-61r).
    #[test]
    fn evaluate_js_async_sends_mapped_await_true() {
        let (mut transport, server) = make_transport_pair();
        let console_actor: ActorId = "server1.conn0.child2/consoleActor3".into();

        let actor_str = console_actor.as_ref().to_owned();
        let srv = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            // Read the evaluateJSAsync request and verify `mapped.await`.
            let req = transport_recv_from(&mut reader).unwrap();
            assert_eq!(
                req["mapped"]["await"],
                serde_json::Value::Bool(true),
                "evaluateJSAsync must include mapped.await=true"
            );

            let ack = json!({"from": &actor_str, "resultID": "mapped-await-test"});
            send_frame(&server, &ack);

            let result = json!({
                "from": &actor_str,
                "type": "evaluationResult",
                "resultID": "mapped-await-test",
                "result": "resolved",
                "hasException": false
            });
            send_frame(&server, &result);
        });

        let eval_result = WebConsoleActor::evaluate_js_async(
            &mut transport,
            &console_actor,
            "Promise.resolve('resolved')",
        )
        .unwrap();

        assert_eq!(eval_result.result, Grip::Value(json!("resolved")),);
        srv.join().unwrap();
    }
}
