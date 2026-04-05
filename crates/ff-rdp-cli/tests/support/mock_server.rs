use std::io::{BufReader, Write};
use std::net::TcpListener;

use ff_rdp_core::transport::{encode_frame, recv_from};
use serde_json::Value;

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
    /// Registered (method_type, response) pairs, matched in insertion order.
    handlers: Vec<(String, Value)>,
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
        self.handlers.push((method.to_owned(), response));
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
        }
    }

    /// A variant of `serve_one` that accepts a connection but never sends the
    /// greeting — useful for testing timeout behaviour.
    pub fn serve_one_silent(self) {
        let (_stream, _peer) = self.listener.accept().expect("accept");
        // Hold the connection open but write nothing, causing the client to
        // time out waiting for the greeting.
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
}
