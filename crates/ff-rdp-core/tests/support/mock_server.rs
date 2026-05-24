use std::io::{BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

use ff_rdp_core::transport::{encode_frame, recv_from};
use serde_json::Value;

/// A minimal mock RDP server for integration testing.
///
/// Binds to a random local port, sends a configurable greeting on connect,
/// and responds to incoming requests matched by their `"type"` field.
///
/// # Example
///
/// ```rust,no_run
/// # use serde_json::json;
/// let server = MockRdpServer::new()
///     .with_greeting(json!({"from":"root","applicationType":"browser","traits":{}}))
///     .on("listTabs", json!({"from":"root","tabs":[]}));
///
/// let port = server.port();
/// let handle = std::thread::spawn(move || server.serve_one());
/// // ... connect client here ...
/// let received = handle.join().unwrap();
/// ```
pub struct MockRdpServer {
    listener: TcpListener,
    greeting: Value,
    /// Registered (method_type, response) pairs, matched in insertion order.
    handlers: Vec<(String, Value)>,
    /// Optional follow-up packets queued per method; sent immediately after
    /// the matching reply. One entry per `on_with_followup` registration; if
    /// a method has multiple matches the followups are popped in order.
    followups: Vec<(String, Value)>,
    /// Receiver end of the injection channel.  Packets placed here are written
    /// to the connected client between request polls.
    inject_rx: mpsc::Receiver<Value>,
    /// Sender end kept so we can hand out a `MockServerHandle`.
    inject_tx: mpsc::Sender<Value>,
}

impl MockRdpServer {
    /// Bind to `127.0.0.1:0` and return a new server ready to be configured.
    pub fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind random port");
        let (inject_tx, inject_rx) = mpsc::channel();
        Self {
            listener,
            greeting: serde_json::json!({
                "from": "root",
                "applicationType": "browser",
                "traits": {}
            }),
            handlers: Vec::new(),
            followups: Vec::new(),
            inject_rx,
            inject_tx,
        }
    }

    /// Return the local port the server is listening on.
    pub fn port(&self) -> u16 {
        self.listener.local_addr().expect("local_addr").port()
    }

    /// Override the greeting sent immediately after a client connects.
    pub fn with_greeting(mut self, greeting: Value) -> Self {
        self.greeting = greeting;
        self
    }

    /// Register a handler: when a request arrives with `"type": method`,
    /// respond with `response`. Handlers are matched in insertion order;
    /// the first match wins.
    pub fn on(mut self, method: &str, response: Value) -> Self {
        self.handlers.push((method.to_owned(), response));
        self
    }

    /// Register a handler whose reply is `response`, followed by an additional
    /// push packet sent immediately after the reply.
    ///
    /// Useful for actors that reply with a typed-less ack and then emit a
    /// typed event (e.g. `watchResources` → `resources-available-array`).
    pub fn on_with_followup(mut self, method: &str, response: Value, followup: Value) -> Self {
        self.handlers.push((method.to_owned(), response));
        self.followups.push((method.to_owned(), followup));
        self
    }

    /// Return a handle that can inject events into the connected client while
    /// `serve_one` is running in another thread.
    ///
    /// Call this *before* moving the server into a thread:
    ///
    /// ```rust,no_run
    /// # use serde_json::json;
    /// # let server = ff_rdp_core::tests::support::MockRdpServer::new();
    /// let handle = server.handle();
    /// let thread = std::thread::spawn(move || server.serve_one());
    /// handle.inject_event(json!({"from": "root", "type": "tabNavigated"}));
    /// ```
    pub fn handle(&self) -> MockServerHandle {
        MockServerHandle {
            tx: self.inject_tx.clone(),
            port: self.port(),
        }
    }

    /// Accept one TCP connection, serve it, and return all requests received.
    ///
    /// This method consumes `self` and is intended to be run in a
    /// `std::thread::spawn` closure. It returns when the client disconnects
    /// (EOF) or when an unrecoverable error occurs.
    ///
    /// Between request polls the server drains any packets queued via
    /// `MockServerHandle::inject_event` and writes them to the client.
    pub fn serve_one(mut self) -> Vec<Value> {
        let (stream, _peer) = self.listener.accept().expect("accept");

        let mut writer = stream.try_clone().expect("try_clone");
        let mut reader = BufReader::new(stream);

        // Use a short read timeout so we can interleave injection draining
        // without busy-waiting.
        reader
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(50)))
            .expect("set_read_timeout");

        // Send the greeting immediately after accepting.
        let greeting_json = serde_json::to_string(&self.greeting).expect("greeting encode");
        writer
            .write_all(encode_frame(&greeting_json).as_bytes())
            .expect("greeting write");

        let mut received: Vec<Value> = Vec::new();

        loop {
            // (a) Try to read the next request.
            match recv_from(&mut reader) {
                Ok(request) => {
                    received.push(request.clone());

                    // Match by the "type" field.
                    let method = request
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or_default();

                    let response = self
                        .handlers
                        .iter()
                        .find(|(m, _)| m == method)
                        .map(|(_, r)| r.clone());

                    let reply = if let Some(resp) = response {
                        resp
                    } else {
                        // No handler matched — send a generic actor error so the
                        // client gets a reply and doesn't hang.
                        serde_json::json!({
                            "from": "root",
                            "error": "unknownMethod",
                            "message": format!("no handler for type={method:?}")
                        })
                    };

                    let json = serde_json::to_string(&reply).expect("response encode");
                    if writer.write_all(encode_frame(&json).as_bytes()).is_err() {
                        break;
                    }

                    // Pop & send a followup packet for this method, if any.
                    if let Some(idx) = self.followups.iter().position(|(m, _)| m == method) {
                        let (_, followup) = self.followups.remove(idx);
                        let follow_json =
                            serde_json::to_string(&followup).expect("followup encode");
                        if writer
                            .write_all(encode_frame(&follow_json).as_bytes())
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                Err(ff_rdp_core::ProtocolError::RecvFailed(io_err))
                    if io_err.kind() == std::io::ErrorKind::UnexpectedEof
                        || io_err.kind() == std::io::ErrorKind::ConnectionReset =>
                {
                    // Drain any remaining injections before the client disconnects.
                    while let Ok(packet) = self.inject_rx.try_recv() {
                        let json = serde_json::to_string(&packet).expect("inject encode");
                        let _ = writer.write_all(encode_frame(&json).as_bytes());
                    }
                    break;
                }
                // Timeout — no data yet; fall through to drain injections.
                Err(ff_rdp_core::ProtocolError::Timeout) => {
                    // nothing — just drain injections below
                }
                Err(_) => break,
            }

            // (b) Drain any pending injections and write them to the client.
            while let Ok(packet) = self.inject_rx.try_recv() {
                let json = serde_json::to_string(&packet).expect("inject encode");
                if writer.write_all(encode_frame(&json).as_bytes()).is_err() {
                    return received;
                }
            }
        }

        received
    }
}

// ---------------------------------------------------------------------------
// MockServerHandle — owned by the driving thread; sends injected events.
// ---------------------------------------------------------------------------

/// A cloneable handle to a running `MockRdpServer` that allows the driving
/// thread to push unsolicited packets to the connected client.
#[derive(Clone)]
pub struct MockServerHandle {
    tx: mpsc::Sender<Value>,
    pub port: u16,
}

impl MockServerHandle {
    /// Inject an arbitrary JSON packet.  The packet is written to the client
    /// on the next `serve_one` poll cycle (within ~50 ms).
    ///
    /// The `from` field must be set by the caller.
    pub fn inject_event(&self, packet: Value) {
        // Silently ignore send errors — the server thread may have already exited.
        let _ = self.tx.send(packet);
    }

    /// Inject a `resources-available-array` watcher notification containing
    /// `payloads` for the given `resource_type`, sent as if from `watcher_actor`.
    ///
    /// This matches the Firefox RDP watcher envelope:
    /// ```json
    /// {"from": "<watcher>", "type": "resources-available-array",
    ///  "array": [["<resource_type>", [<payloads...>]]]}
    /// ```
    pub fn inject_watcher_resource(
        &self,
        watcher_actor: &str,
        resource_type: &str,
        payloads: &[Value],
    ) {
        let packet = serde_json::json!({
            "from": watcher_actor,
            "type": "resources-available-array",
            "array": [[resource_type, payloads]]
        });
        self.inject_event(packet);
    }
}
