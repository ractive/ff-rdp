use std::collections::VecDeque;
use std::io::{BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use ff_rdp_core::transport::{encode_frame, recv_from};
use serde_json::Value;

/// One entry in a sequence handler: `(immediate_response, followup_messages)`.
type SeqEntry = (Value, Vec<Value>);

/// A single-response or sequence-of-responses handler entry.
enum HandlerKind {
    /// Always returns the same (response, followups).
    Fixed(Value, Vec<Value>),
    /// Pops the front item for each invocation; when the queue is exhausted,
    /// repeats the last item forever.
    Sequence {
        queue: Arc<Mutex<VecDeque<SeqEntry>>>,
        last: Arc<Mutex<SeqEntry>>,
    },
}

/// A minimal mock RDP server for CLI end-to-end testing.
///
/// Binds to a random local port, sends a configurable greeting on connect,
/// and responds to incoming requests matched by their `"type"` field.
///
/// # Example
///
/// ```rust,no_run
/// let server = MockRdpServer::new()
///     .on("listTabs", serde_json::json!({"from":"root","tabs":[]}));
///
/// let port = server.port();
/// let handle = std::thread::spawn(move || server.serve_one());
/// // ... connect client here ...
/// handle.join().unwrap();
/// ```
pub struct MockRdpServer {
    listener: TcpListener,
    greeting: Value,
    /// Registered handlers, matched in insertion order.  The first match wins.
    handlers: Vec<(String, HandlerKind)>,
}

impl MockRdpServer {
    /// Bind to `127.0.0.1:0` and return a new server ready to be configured.
    pub fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind random port");
        Self {
            listener,
            greeting: serde_json::json!({
                "from": "root",
                "applicationType": "browser",
                "traits": {}
            }),
            handlers: Vec::new(),
        }
    }

    /// Return the local port the server is listening on.
    pub fn port(&self) -> u16 {
        self.listener.local_addr().expect("local_addr").port()
    }

    /// Register a handler: when a request arrives with `"type": method`,
    /// respond with `response`. Handlers are matched in insertion order;
    /// the first match wins.
    pub fn on(mut self, method: &str, response: Value) -> Self {
        self.handlers
            .push((method.to_owned(), HandlerKind::Fixed(response, Vec::new())));
        self
    }

    /// Register a handler with a follow-up message sent after the response.
    ///
    /// This is used for async patterns like `evaluateJSAsync` where an
    /// immediate response is sent, followed by an `evaluationResult` event.
    pub fn on_with_followup(mut self, method: &str, response: Value, followup: Value) -> Self {
        self.handlers.push((
            method.to_owned(),
            HandlerKind::Fixed(response, vec![followup]),
        ));
        self
    }

    /// Register a handler with multiple follow-up messages sent after the response.
    pub fn on_with_followups(
        mut self,
        method: &str,
        response: Value,
        followups: Vec<Value>,
    ) -> Self {
        self.handlers
            .push((method.to_owned(), HandlerKind::Fixed(response, followups)));
        self
    }

    /// Register a sequence handler: successive calls to `method` consume
    /// successive entries from `responses`.  Once all entries are exhausted,
    /// the last entry is repeated indefinitely.
    ///
    /// Each entry is `(immediate_response, followup_messages)`.
    ///
    /// This is useful for commands that issue the same RDP request type
    /// multiple times and need different replies for each call (e.g. the
    /// `responsive` command issues several `evaluateJSAsync` calls that must
    /// return different values).
    pub fn on_sequence(mut self, method: &str, responses: Vec<(Value, Vec<Value>)>) -> Self {
        assert!(
            !responses.is_empty(),
            "on_sequence requires at least one response"
        );

        let last = responses.last().expect("checked non-empty").clone();
        let queue: VecDeque<(Value, Vec<Value>)> = responses.into();

        self.handlers.push((
            method.to_owned(),
            HandlerKind::Sequence {
                queue: Arc::new(Mutex::new(queue)),
                last: Arc::new(Mutex::new(last)),
            },
        ));
        self
    }

    /// Accept one TCP connection, serve it, and return all requests received.
    ///
    /// This method consumes `self` and is intended to be run in a
    /// `std::thread::spawn` closure. It returns when the client disconnects
    /// (EOF) or when an unrecoverable error occurs.
    pub fn serve_one(self) {
        let (stream, _peer) = self.listener.accept().expect("accept");

        let mut writer = stream.try_clone().expect("try_clone");
        let mut reader = BufReader::new(stream);

        // Send the greeting immediately after accepting.
        let greeting_json = serde_json::to_string(&self.greeting).expect("greeting encode");
        writer
            .write_all(encode_frame(&greeting_json).as_bytes())
            .expect("greeting write");

        loop {
            // Read the next request. EOF / connection reset is a clean client disconnect.
            let request = match recv_from(&mut reader) {
                Ok(v) => v,
                Err(ff_rdp_core::ProtocolError::RecvFailed(io_err))
                    if io_err.kind() == std::io::ErrorKind::UnexpectedEof
                        || io_err.kind() == std::io::ErrorKind::ConnectionReset =>
                {
                    break;
                }
                Err(_) => break,
            };

            // Match by the "type" field.
            let method = request
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();

            let handler = self.handlers.iter().find(|(m, _)| m == method);

            let (reply, followups) = if let Some((_, kind)) = handler {
                match kind {
                    HandlerKind::Fixed(resp, follows) => (resp.clone(), follows.clone()),
                    HandlerKind::Sequence { queue, last } => {
                        let mut q = queue.lock().expect("sequence queue lock");
                        if let Some(entry) = q.pop_front() {
                            // Update last so the final item repeats correctly.
                            *last.lock().expect("sequence last lock") = entry.clone();
                            entry
                        } else {
                            last.lock().expect("sequence last lock").clone()
                        }
                    }
                }
            } else {
                // No handler matched — send a generic actor error so the
                // client gets a reply and doesn't hang.
                (
                    serde_json::json!({
                        "from": "root",
                        "error": "unknownMethod",
                        "message": format!("no handler for type={method:?}")
                    }),
                    Vec::new(),
                )
            };

            let json = serde_json::to_string(&reply).expect("response encode");
            if writer.write_all(encode_frame(&json).as_bytes()).is_err() {
                break;
            }

            // Send follow-up messages if registered (e.g., evaluationResult event).
            for followup_msg in followups {
                let followup_json = serde_json::to_string(&followup_msg).expect("followup encode");
                if writer
                    .write_all(encode_frame(&followup_json).as_bytes())
                    .is_err()
                {
                    break;
                }
            }
        }
    }

    /// A variant of `serve_one` that accepts a connection but never sends the
    /// greeting — useful for testing timeout behaviour.
    pub fn serve_one_silent(self) {
        let (stream, _peer) = self.listener.accept().expect("accept");
        // Block until the client disconnects (EOF), so the thread exits cleanly
        // rather than leaking a sleep for 30 seconds.
        let mut buf = [0u8; 1];
        let _ = std::io::Read::read(&mut &stream, &mut buf);
    }
}
