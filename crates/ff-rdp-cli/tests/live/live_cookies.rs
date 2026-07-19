//! iter-82 AC: `live_cookies_surfaces_js_readable_cookie`.
//! iter-83 AC: `live_cookies_default_surfaces_js_readable_cookie`.
//!
//! Serves a minimal HTML page on a localhost HTTP port, navigates Firefox to
//! it, then runs `ff-rdp cookies --include-document-cookie` and asserts the
//! cookie name `"probe"` appears in the results.
//!
//! A `data:` URL is used as a fallback fixture in many tests, but cookies set
//! via `document.cookie` on `data:` URLs do not persist — browsers treat them
//! as cookie-averse origins.  A real `http://127.0.0.1` origin is required.
//!
//! This validates Theme D: cookies set via JS without a `Domain=` attribute
//! are surfaced in `ff-rdp cookies` output — via `--include-document-cookie`
//! if the StorageActor enumeration misses them, or authoritatively by the
//! StorageActor itself when it doesn't.
//!
//! # iter-124 update
//!
//! Before iter-121, StorageActor cookie enumeration was dead on FF152, so
//! `--include-document-cookie` was the *only* path that ever surfaced this
//! probe cookie, always tagged `source: "document.cookie"`. iter-121 fixed
//! StorageActor enumeration, and it turns out FF152's StorageActor *does*
//! enumerate a JS-set cookie without a `Domain=` attribute — so this cookie
//! is now returned authoritatively by the StorageActor (no `source` field,
//! `isHttpOnly`/`isSecure`/`sameSite` flags populated), and the cookies
//! command's merge logic (`--include-document-cookie` only adds entries not
//! already present by name — see `crates/ff-rdp-cli/src/commands/cookies.rs`)
//! correctly drops the redundant `document.cookie` duplicate. This is
//! strictly better data, not a regression, so the assertion below now pins
//! the StorageActor-sourced shape instead of the old fallback tag.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_cookies -- --nocapture

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};

/// HTML body served by the local fixture server.
///
/// Sets `document.cookie = "probe=1"` so the cookie is JS-readable on
/// the `http://127.0.0.1` origin.  The `source: "document.cookie"` path
/// in ff-rdp cookies must surface this entry when
/// `--include-document-cookie` is passed.
const FIXTURE_BODY: &[u8] = b"<!DOCTYPE html><html><head></head><body>\
<script>document.cookie='probe=1';window.__cookieSet=true;</script>\
</body></html>";

/// Spawn a minimal single-shot HTTP server on a random port.
///
/// The server accepts connections in a background thread (bounded to 10
/// accepts so the thread exits after the test), serving `FIXTURE_BODY`
/// with `Content-Type: text/html` on any `GET` request.  Returns
/// `(port, join-handle)`.
fn spawn_fixture_server() -> Option<(u16, thread::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();

    let handle = thread::spawn(move || {
        listener.set_nonblocking(false).ok();
        for stream in listener.incoming().take(10) {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            // Drain the HTTP request so the browser doesn't see a reset.
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);

            let resp = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\n\
                 Cache-Control: no-store\r\n\
                 Connection: close\r\n\r\n",
                FIXTURE_BODY.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(FIXTURE_BODY);
        }
    });

    Some((port, handle))
}

/// `live_cookies_surfaces_js_readable_cookie`:
/// Serve a fixture page that sets `document.cookie = "probe=1"` from JS
/// on a real `http://127.0.0.1` origin, navigate Firefox to it, then
/// assert that `ff-rdp cookies --include-document-cookie` surfaces a
/// cookie named `"probe"` in its results — either as an authoritative
/// StorageActor entry (has `isHttpOnly`/`isSecure`/`sameSite` flags, no
/// `source` field) or, if the StorageActor happens to miss it, via the
/// `document.cookie` fallback (`source: "document.cookie"`). Since
/// iter-121 fixed StorageActor cookie enumeration on FF152, this probe
/// cookie is enumerated authoritatively — asserting a specific source
/// would pin an implementation detail rather than the actual contract
/// (`--include-document-cookie` must never lose the cookie or serve an
/// entry with neither shape).
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_cookies_surfaces_js_readable_cookie() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_cookies_surfaces_js_readable_cookie: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_cookies_surfaces_js_readable_cookie: Firefox not available — skipping");
        return;
    };

    let Some((http_port, _server)) = spawn_fixture_server() else {
        eprintln!(
            "live_cookies_surfaces_js_readable_cookie: could not bind HTTP server — skipping"
        );
        return;
    };

    let fixture_url = format!("http://127.0.0.1:{http_port}/");

    // Navigate to fixture so the JS cookie is set.
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", &fixture_url])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "live_cookies_surfaces_js_readable_cookie: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Run cookies with the document-cookie fallback enabled.
    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["cookies", "--include-document-cookie"])
        .output()
        .expect("ff-rdp cookies --include-document-cookie");
    assert!(
        out.status.success(),
        "live_cookies_surfaces_js_readable_cookie: cookies failed — stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "live_cookies_surfaces_js_readable_cookie: output is not valid JSON: {e}\n\
                 stdout={stdout}\nstderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    });

    let results = json["results"]
        .as_array()
        .expect("results must be an array");

    let has_probe = results
        .iter()
        .any(|c| c["name"].as_str().unwrap_or("") == "probe");

    assert!(
        has_probe,
        "live_cookies_surfaces_js_readable_cookie: cookie 'probe' not found in results; \
         results={results:?}"
    );

    // The probe cookie must be surfaced in one of two valid shapes:
    //   - StorageActor-authoritative: `source` absent, `isHttpOnly` present
    //     (iter-121 fixed FF152 StorageActor enumeration — this is now the
    //     common case for a JS-set cookie without a Domain= attribute).
    //   - document.cookie fallback: `source: "document.cookie"` (only if the
    //     StorageActor ever legitimately misses this cookie).
    // Either is acceptable; what matters is the cookie isn't lost and isn't
    // some malformed half-shape (e.g. `source: null` with no flags either).
    let probe = results
        .iter()
        .find(|c| c["name"].as_str().unwrap_or("") == "probe")
        .expect("probe cookie must exist");
    let source = probe["source"].as_str();
    let is_document_cookie_sourced = source == Some("document.cookie");
    let is_storage_actor_sourced = source.is_none() && probe["isHttpOnly"].is_boolean();
    assert!(
        is_document_cookie_sourced || is_storage_actor_sourced,
        "probe cookie must be either document.cookie-sourced (source: \"document.cookie\") \
         or StorageActor-sourced (no source field, isHttpOnly present); got {probe:?}"
    );

    eprintln!(
        "live_cookies_surfaces_js_readable_cookie: PASS — found 'probe' among {} cookies \
         (storage_actor_sourced={is_storage_actor_sourced})",
        results.len()
    );
}

/// `live_cookies_default_surfaces_js_readable_cookie` (iter-83 AC):
///
/// Same as `live_cookies_surfaces_js_readable_cookie` but calls `ff-rdp cookies`
/// WITHOUT any `--include-document-cookie` flag.  Verifies that the default
/// behavior (as of iter-83 Theme D) includes document.cookie evaluation, so the
/// `probe` cookie shows up without an explicit flag.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_cookies_default_surfaces_js_readable_cookie() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!(
            "live_cookies_default_surfaces_js_readable_cookie: set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_cookies_default_surfaces_js_readable_cookie: Firefox not available — skipping"
        );
        return;
    };

    let Some((http_port, _server)) = spawn_fixture_server() else {
        eprintln!(
            "live_cookies_default_surfaces_js_readable_cookie: could not bind HTTP server — skipping"
        );
        return;
    };

    let fixture_url = format!("http://127.0.0.1:{http_port}/");

    // Navigate to fixture so the JS cookie is set.
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", &fixture_url])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "live_cookies_default_surfaces_js_readable_cookie: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Run cookies WITHOUT --include-document-cookie — this is the key difference
    // from the iter-82 test: the default must now include document.cookie.
    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["cookies"])
        .output()
        .expect("ff-rdp cookies (no flags)");
    assert!(
        out.status.success(),
        "live_cookies_default_surfaces_js_readable_cookie: cookies failed — stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "live_cookies_default_surfaces_js_readable_cookie: output is not valid JSON: {e}\n\
             stdout={stdout}\nstderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    });

    let results = json["results"]
        .as_array()
        .expect("results must be an array");

    let has_probe = results
        .iter()
        .any(|c| c["name"].as_str().unwrap_or("") == "probe");

    assert!(
        has_probe,
        "live_cookies_default_surfaces_js_readable_cookie: cookie 'probe' not found in results; \
         results={results:?}"
    );

    eprintln!(
        "live_cookies_default_surfaces_js_readable_cookie: PASS — \
         found 'probe' among {} cookies (no --include-document-cookie flag)",
        results.len()
    );
}

/// HTML/headers body served by the httpOnly fixture server.
///
/// Sets two cookies via `Set-Cookie` response headers:
///   - `normal=vis123` (visible to `document.cookie`)
///   - `secret=hidden456; HttpOnly; Secure; SameSite=Strict` (invisible to
///     `document.cookie` — only the StorageActor can enumerate it)
const HTTPONLY_FIXTURE_BODY: &[u8] =
    b"<!DOCTYPE html><html><head></head><body>httpOnly cookie fixture</body></html>";

/// Spawn a single-shot HTTP server that sets a normal + an httpOnly cookie via
/// `Set-Cookie` response headers.  Returns `(port, join-handle)`.
fn spawn_httponly_fixture_server() -> Option<(u16, thread::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();

    let handle = thread::spawn(move || {
        listener.set_nonblocking(false).ok();
        for stream in listener.incoming().take(10) {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);

            let resp = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: text/html; charset=utf-8\r\n\
                 Set-Cookie: normal=vis123; Path=/; SameSite=Lax\r\n\
                 Set-Cookie: secret=hidden456; Path=/; HttpOnly; Secure; SameSite=Strict\r\n\
                 Content-Length: {}\r\n\
                 Cache-Control: no-store\r\n\
                 Connection: close\r\n\r\n",
                HTTPONLY_FIXTURE_BODY.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(HTTPONLY_FIXTURE_BODY);
        }
    });

    Some((port, handle))
}

/// `live_cookies_httponly_enumerated` (iter-121 AC):
///
/// After navigating to a page that sets an httpOnly cookie via a `Set-Cookie`
/// header, `ff-rdp cookies` must return that cookie with `isHttpOnly == true`
/// and non-null `isSecure`/`sameSite`, sourced from the StorageActor (NOT
/// `document.cookie`, which can never see httpOnly cookies).  This is the core
/// regression fix — on FF152 the StorageActor enumeration silently returned
/// empty and every cookie fell back to `document.cookie`, missing httpOnly
/// cookies entirely.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_cookies_httponly_enumerated() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_cookies_httponly_enumerated: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_cookies_httponly_enumerated: Firefox not available — skipping");
        return;
    };

    let Some((http_port, _server)) = spawn_httponly_fixture_server() else {
        eprintln!("live_cookies_httponly_enumerated: could not bind HTTP server — skipping");
        return;
    };

    let fixture_url = format!("http://127.0.0.1:{http_port}/");

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", &fixture_url])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "live_cookies_httponly_enumerated: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["cookies"])
        .output()
        .expect("ff-rdp cookies");
    assert!(
        out.status.success(),
        "live_cookies_httponly_enumerated: cookies failed — stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("live_cookies_httponly_enumerated: output is not valid JSON: {e}\nstdout={stdout}")
    });

    let results = json["results"]
        .as_array()
        .expect("results must be an array");

    let secret = results
        .iter()
        .find(|c| c["name"].as_str() == Some("secret"))
        .unwrap_or_else(|| {
            panic!(
                "live_cookies_httponly_enumerated: httpOnly cookie 'secret' not found; \
                 results={results:?}"
            )
        });

    assert_eq!(
        secret["isHttpOnly"].as_bool(),
        Some(true),
        "secret cookie must have isHttpOnly=true; got {:?}",
        secret["isHttpOnly"]
    );
    // Flags must be present (non-null) — the document.cookie fallback would omit
    // these entirely, so their presence proves the StorageActor path fired.
    assert!(
        secret["isSecure"].is_boolean(),
        "secret cookie must have a non-null isSecure flag; got {:?}",
        secret["isSecure"]
    );
    assert!(
        secret["sameSite"].is_string(),
        "secret cookie must have a non-null sameSite flag; got {:?}",
        secret["sameSite"]
    );
    // Must NOT be sourced from document.cookie.
    assert_ne!(
        secret["source"].as_str(),
        Some("document.cookie"),
        "secret cookie must come from the StorageActor, not document.cookie"
    );

    eprintln!(
        "live_cookies_httponly_enumerated: PASS — 'secret' httpOnly cookie enumerated \
         (isSecure={:?}, sameSite={:?})",
        secret["isSecure"], secret["sameSite"]
    );
}

/// `live_cookies_storage_only_nonempty` (iter-121 AC):
///
/// `ff-rdp cookies --storage-only` must return `>= 1` entry on a page that set
/// a normal cookie — proving the StorageActor enumeration path (not just the
/// document.cookie merge) is non-empty on FF152.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_cookies_storage_only_nonempty() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_cookies_storage_only_nonempty: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_cookies_storage_only_nonempty: Firefox not available — skipping");
        return;
    };

    let Some((http_port, _server)) = spawn_httponly_fixture_server() else {
        eprintln!("live_cookies_storage_only_nonempty: could not bind HTTP server — skipping");
        return;
    };

    let fixture_url = format!("http://127.0.0.1:{http_port}/");

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", &fixture_url])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "live_cookies_storage_only_nonempty: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["cookies", "--storage-only"])
        .output()
        .expect("ff-rdp cookies --storage-only");
    assert!(
        out.status.success(),
        "live_cookies_storage_only_nonempty: cookies failed — stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("live_cookies_storage_only_nonempty: output is not valid JSON: {e}\nstdout={stdout}")
    });

    let results = json["results"]
        .as_array()
        .expect("results must be an array");

    assert!(
        !results.is_empty(),
        "live_cookies_storage_only_nonempty: --storage-only returned 0 entries; \
         StorageActor enumeration must be non-empty on FF152. results={results:?}"
    );

    // Every entry must come from the StorageActor (no document.cookie merge).
    for c in results {
        assert_ne!(
            c["source"].as_str(),
            Some("document.cookie"),
            "--storage-only entries must not be sourced from document.cookie: {c:?}"
        );
    }

    eprintln!(
        "live_cookies_storage_only_nonempty: PASS — StorageActor returned {} cookie(s)",
        results.len()
    );
}
