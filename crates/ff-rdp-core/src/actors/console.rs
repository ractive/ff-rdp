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

/// Operations on a WebConsole actor (JavaScript evaluation).
pub struct WebConsoleActor;

impl WebConsoleActor {
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

#[cfg(test)]
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
}
