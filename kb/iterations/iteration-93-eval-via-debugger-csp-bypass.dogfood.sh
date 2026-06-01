#!/usr/bin/env bash
# iter-93 dogfood gate — eval survives strict Content Security Policy sites.
#
# Verifies that `ff-rdp eval 'document.title'` works on a page that sets a
# strict CSP that would have blocked the old eval() isolation wrapper.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-93-eval-via-debugger-csp-bypass.dogfood.sh
set -euo pipefail

# Prefer the development build over any installed ff-rdp binary, so the
# dogfood script exercises the actual branch code.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
for candidate in "$REPO_ROOT/target/debug/ff-rdp" "$REPO_ROOT/target/release/ff-rdp"; do
  if [ -x "$candidate" ]; then
    export PATH="$(dirname "$candidate"):$PATH"
    echo "using ff-rdp: $candidate"
    break
  fi
done
unset candidate SCRIPT_DIR

SENTINEL=/tmp/ff-rdp-iter-93-dogfood-ok
rm -f "$SENTINEL"

# Kill any stale Firefox launched by ff-rdp.
pkill -f 'firefox.*ff-rdp-profile' || true
sleep 1

# Launch headless Firefox on the default port.
ff-rdp launch --headless --port 6000
sleep 2

# Spin up a minimal Python HTTP server that serves a strict-CSP page on a
# random port.  We use a heredoc to pass the server script inline; this avoids
# any dependency on axum/hyper and uses only the Python stdlib.
FIXTURE_PORT_FILE=/tmp/ff-rdp-iter93-port.txt
SERVER_PID_FILE=/tmp/ff-rdp-iter93-server.pid
rm -f "$FIXTURE_PORT_FILE" "$SERVER_PID_FILE"

python3 - <<'PYEOF' &
import http.server
import socket

BODY = b"""<!DOCTYPE html>
<html>
<head><title>iter93-csp-fixture</title></head>
<body><div style="height:5000px">x</div></body>
</html>"""

CSP = "script-src 'self'; object-src 'none'; base-uri 'self'"

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(BODY)))
        self.send_header("Content-Security-Policy", CSP)
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(BODY)
    def log_message(self, *args):
        pass  # suppress access log

sock = socket.socket()
sock.bind(("127.0.0.1", 0))
port = sock.getsockname()[1]
sock.close()

with open("/tmp/ff-rdp-iter93-port.txt", "w") as f:
    f.write(str(port))
with open("/tmp/ff-rdp-iter93-server.pid", "w") as f:
    import os; f.write(str(os.getpid()))

server = http.server.HTTPServer(("127.0.0.1", port), Handler)
server.serve_forever()
PYEOF

# Wait for the server to write its port (up to 5 s).
for i in $(seq 1 50); do
  [ -f "$FIXTURE_PORT_FILE" ] && break
  sleep 0.1
done
[ -f "$FIXTURE_PORT_FILE" ] || { echo "FAIL: fixture server did not start" >&2; exit 1; }

FIXTURE_PORT=$(cat "$FIXTURE_PORT_FILE")
FIXTURE_URL="http://127.0.0.1:${FIXTURE_PORT}/"
echo "fixture server: $FIXTURE_URL"

# Cleanup trap.
cleanup() {
  pkill -f 'firefox.*ff-rdp-profile' || true
  if [ -f "$SERVER_PID_FILE" ]; then
    kill "$(cat "$SERVER_PID_FILE")" 2>/dev/null || true
  fi
}
trap cleanup EXIT

# Navigate to the CSP fixture.
ff-rdp navigate "$FIXTURE_URL" \
  || { echo "FAIL: navigate to CSP fixture failed" >&2; exit 1; }

# Evaluate document.title — must exit 0 on this branch.
EVAL_OUT=$(ff-rdp eval 'document.title') \
  || { echo "FAIL: eval 'document.title' exited non-zero (CSP still blocking?): $EVAL_OUT" >&2; exit 1; }

# Parse the result with Python (available everywhere; avoids a jq dep).
RESULT=$(python3 -c "import sys,json; d=json.loads(sys.stdin.read()); print(d.get('results',''))" <<< "$EVAL_OUT" 2>/dev/null || echo "")
if [ "$RESULT" != "iter93-csp-fixture" ]; then
  echo "FAIL: eval result '$RESULT' != 'iter93-csp-fixture'" >&2
  echo "Full output: $EVAL_OUT" >&2
  exit 1
fi
echo "Theme A OK: eval 'document.title' = '$RESULT' on strict-CSP page"

# Verify scrollY eval also works.
SCROLL_OUT=$(ff-rdp eval 'window.scrollTo(0, 100); window.scrollY') \
  || { echo "FAIL: scrollY eval exited non-zero" >&2; exit 1; }
SCROLL_Y=$(python3 -c "import sys,json; d=json.loads(sys.stdin.read()); print(d.get('results',0))" <<< "$SCROLL_OUT" 2>/dev/null || echo "0")
if python3 -c "import sys; sys.exit(0 if float('$SCROLL_Y') >= 1 else 1)" 2>/dev/null; then
  echo "Theme B OK: window.scrollY = $SCROLL_Y (>= 1 after scrollTo)"
else
  echo "FAIL: scrollY=$SCROLL_Y — expected >= 1 after scrollTo(0, 100)" >&2
  exit 1
fi

# Verify script errors still surface.
if ff-rdp eval 'throw new Error("boom")' 2>/dev/null; then
  echo "FAIL: eval of throw must exit non-zero" >&2
  exit 1
fi
echo "Theme C OK: script errors still surface (eval 'throw new Error' exited non-zero)"

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-93 dogfood: CSP bypass verified — $SENTINEL"
