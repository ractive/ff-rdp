//! Full-payload collection while a destination has not committed yet.
//! Both routes must either return the destination or label the outgoing view
//! unready. The six-second response delay exceeds the bounded settle wait.
//!
//! daemon-parity: live_253_outgoing_page_daemon covers the same fixture matrix.

use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

fn exercise(direct: bool) {
    assert!(live_tests_enabled());
    let ff = LiveFirefox::headless_on_random_port();
    if !direct {
        assert!(ff.with_daemon().is_some());
    }
    let run = |args: &[&str]| {
        let mut cmd = Command::new(ff_rdp_bin());
        cmd.args([
            "--host",
            "127.0.0.1",
            "--port",
            &ff.port().to_string(),
            "--timeout",
            "20000",
        ]);
        if direct {
            cmd.arg("--no-daemon");
        }
        let out = cmd.args(args).output().expect("run this checkout's ff-rdp");
        assert!(out.status.success(), "{args:?}: {out:?}");
        let value = serde_json::from_slice::<Value>(&out.stdout).expect("JSON envelope");
        assert_eq!(
            value["meta"]["route"],
            if direct { "direct" } else { "daemon" },
            "{value}"
        );
        value
    };
    let mut unexpected = Vec::new();
    for delay_ms in [50, 700, 3_200, 6_000] {
        let server = FixtureServer::start(HashMap::from([
            (
                "/".into(),
                FixtureRoute::html(
                    "<!doctype html><title>t253 origin</title><h1>Outgoing document</h1>\
                 <article><p>A small real article about computing machines and their history.</p>\
                 <a href='/destination'>Read the destination</a></article>",
                ),
            ),
            (
                "/destination".into(),
                FixtureRoute::html(
                    "<!doctype html><title>t253 destination</title><h1>Destination document</h1>\
                 <article><p>The destination has now committed.</p></article>",
                )
                .with_delay(Duration::from_millis(delay_ms)),
            ),
        ]))
        .expect("fixture server");
        // No --with-page here: every sample must inject the full Readability
        // bundle on a fresh document, then run headings + interactive + reader.
        run(&["navigate", &server.base_url()]);
        let started = Instant::now();
        let view = run(&["click", "a", "--no-wait", "--with-page"]);
        let elapsed = started.elapsed();
        let heading = &view["results"]["page"]["headings"][0]["text"];
        let ready = &view["meta"]["page_ready"];
        eprintln!(
            "ITER253 direct={direct} delay_ms={delay_ms} elapsed_ms={} heading={heading} ready={ready} injected={} parse_ms={} attempts={}",
            elapsed.as_millis(),
            view["meta"]["page_readability_injected"],
            view["meta"]["page_parse_ms"],
            view["meta"]["page_attempts"]
        );
        assert_eq!(view["meta"]["page_readability_injected"], true);
        assert!(view["results"]["page"]["interactive"].is_array());
        assert!(view["meta"]["page_parse_ms"].is_number());
        if heading == "Outgoing document" {
            if ready != false {
                unexpected.push(view);
            }
        } else {
            assert_eq!(heading, "Destination document");
            assert_eq!(ready, true);
        }
    }
    assert!(
        unexpected.is_empty(),
        "outgoing views reported confidently ready: {unexpected:?}"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_253_outgoing_page_direct() {
    exercise(true);
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_253_outgoing_page_daemon() {
    exercise(false);
}

struct SameUrlFixture {
    url: String,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl SameUrlFixture {
    fn start() -> Self {
        use std::io::{BufRead as _, BufReader, Write as _};
        use std::net::TcpListener;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://127.0.0.1:{}/same",
            listener.local_addr().unwrap().port()
        );
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let generation = Arc::new(AtomicUsize::new(0));
        let worker = std::thread::spawn(move || {
            let mut requests = Vec::new();
            while !worker_stop.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                let generation = Arc::clone(&generation);
                requests.push(std::thread::spawn(move || {
                    stream.set_nonblocking(false).unwrap();
                    stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                    let mut line = String::new();
                    if BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).is_err() {
                        // Firefox can preconnect without sending an HTTP request.
                        return;
                    }
                    let body = if line.starts_with("GET /same ") {
                        let generation = generation.fetch_add(1, Ordering::Relaxed) + 1;
                        if generation > 1 {
                            std::thread::sleep(Duration::from_secs(6));
                        }
                        format!("<!doctype html><title>generation {generation}</title>\
                            <h1>Generation {generation}</h1><article><p>A complete article on this generation.</p>\
                            <a href='/same'>Replace with the same URL</a></article>")
                    } else {
                        String::new()
                    };
                    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    let _ = stream.write_all(response.as_bytes());
                }));
            }
            for request in requests {
                request.join().unwrap();
            }
        });
        Self {
            url,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for SameUrlFixture {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn exercise_same_url(direct: bool) {
    assert!(live_tests_enabled());
    let ff = LiveFirefox::headless_on_random_port();
    if !direct {
        assert!(ff.with_daemon().is_some());
    }
    let server = SameUrlFixture::start();
    let run = |args: &[&str]| {
        let mut cmd = Command::new(ff_rdp_bin());
        cmd.args([
            "--host",
            "127.0.0.1",
            "--port",
            &ff.port().to_string(),
            "--timeout",
            "20000",
        ]);
        if direct {
            cmd.arg("--no-daemon");
        }
        let out = cmd.args(args).output().unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
        let value = serde_json::from_slice::<Value>(&out.stdout).unwrap();
        assert_eq!(
            value["meta"]["route"],
            if direct { "direct" } else { "daemon" },
            "{value}"
        );
        value
    };
    run(&["navigate", &server.url]);
    let before = run(&[
        "eval",
        "JSON.stringify({url:location.href,heading:document.querySelector('h1').textContent})",
    ]);
    eprintln!("ITER253 SAME_URL direct={direct} before={before}");
    let before: Value = serde_json::from_str(before["results"].as_str().unwrap()).unwrap();
    assert_eq!(before["url"], server.url);
    assert_eq!(before["heading"], "Generation 1");
    let started = Instant::now();
    let view = run(&["click", "a", "--no-wait", "--with-page"]);
    eprintln!(
        "ITER253 SAME_URL direct={direct} elapsed_ms={} view={view}",
        started.elapsed().as_millis()
    );
    // Verify this really was a cross-document replacement at the identical URL.
    std::thread::sleep(Duration::from_secs(6));
    let after = run(&[
        "eval",
        "JSON.stringify({url:location.href,heading:document.querySelector('h1').textContent})",
    ]);
    eprintln!("ITER253 SAME_URL direct={direct} after={after}");
    let after: Value = serde_json::from_str(after["results"].as_str().unwrap()).unwrap();
    assert_eq!(after["url"], server.url);
    assert_eq!(after["heading"], "Generation 2");
    assert_eq!(view["meta"]["page_readability_injected"], true);
    if view["results"]["page"]["headings"][0]["text"] == "Generation 1" {
        assert_eq!(
            view["meta"]["page_ready"], false,
            "the unchanged URL did not prove a new document: {view}"
        );
    } else {
        assert_eq!(
            view["results"]["page"]["headings"][0]["text"],
            "Generation 2"
        );
    }

    // A fragment really changes the previous URL, unlike the replacement
    // above. Keep a generation marker to prove it stayed in the same document.
    run(&[
        "eval",
        "document.querySelector('a').setAttribute('href', '#here')",
    ]);
    eprintln!("ITER253 FRAGMENT direct={direct} before={after}");
    let started = Instant::now();
    let fragment = run(&["click", "a", "--no-wait", "--with-page"]);
    let elapsed = started.elapsed();
    let fragment_url = run(&["eval", "location.href"]);
    eprintln!(
        "ITER253 FRAGMENT direct={direct} elapsed_ms={} after={fragment_url} view={fragment}",
        elapsed.as_millis()
    );
    assert_eq!(fragment_url["results"], format!("{}#here", server.url));
    assert_eq!(
        fragment["results"]["page"]["headings"][0]["text"],
        "Generation 2"
    );
    assert_eq!(fragment["meta"]["page_ready"], true);
    assert!(
        elapsed < Duration::from_secs(2),
        "fragment burned settle budget: {elapsed:?}"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_253_same_url_replacement_direct() {
    exercise_same_url(true);
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_253_same_url_replacement_daemon() {
    exercise_same_url(false);
}
