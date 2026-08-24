#!/usr/bin/env bash
# iter-93 dogfood gate — eval survives strict Content Security Policy sites.
#
# Verifies that `eval 'document.title'` works on a page that sets a strict CSP
# that would have blocked the old eval() isolation wrapper.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-93-eval-via-debugger-csp-bypass.dogfood.sh
set -euo pipefail

# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/dogfood-lib.sh"

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

dogfood_init
WORK="$(dogfood_workdir)"
PORT="$(dogfood_free_port)"

FIXTURE_PORT_FILE="$WORK/fixture-port.txt"
SERVER_PID_FILE="$WORK/fixture-server.pid"

# Extra cleanup for the fixture HTTP server. Registered with dogfood_on_exit
# rather than a second `trap … EXIT`, which would silently replace the
# teardown that stops this run's Firefox.
stop_fixture_server() {
  if [ -f "$SERVER_PID_FILE" ]; then
    kill "$(cat "$SERVER_PID_FILE")" 2>/dev/null || true
  fi
}
dogfood_on_exit stop_fixture_server

dogfood_launch "$PORT"
sleep 2

# Spin up a minimal Python HTTP server that serves a strict-CSP page on a
# random port.  We use a heredoc to pass the server script inline; this avoids
# any dependency on axum/hyper and uses only the Python stdlib.
FF_RDP_ITER93_PORT_FILE="$FIXTURE_PORT_FILE" \
FF_RDP_ITER93_PID_FILE="$SERVER_PID_FILE" \
python3 - <<'PYEOF' &
import http.server
import os

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

server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
port = server.server_address[1]

with open(os.environ["FF_RDP_ITER93_PID_FILE"], "w") as f:
    f.write(str(os.getpid()))
# Written last: the shell polls for this file as the readiness signal.
with open(os.environ["FF_RDP_ITER93_PORT_FILE"], "w") as f:
    f.write(str(port))

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

# Navigate to the CSP fixture.
ffrdp --port "$PORT" navigate "$FIXTURE_URL" \
  || { echo "FAIL: navigate to CSP fixture failed" >&2; exit 1; }

# Evaluate document.title — must exit 0 on this branch.
EVAL_OUT=$(ffrdp --port "$PORT" eval 'document.title') \
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
SCROLL_OUT=$(ffrdp --port "$PORT" eval 'window.scrollTo(0, 100); window.scrollY') \
  || { echo "FAIL: scrollY eval exited non-zero" >&2; exit 1; }
SCROLL_Y=$(python3 -c "import sys,json; d=json.loads(sys.stdin.read()); print(d.get('results',0))" <<< "$SCROLL_OUT" 2>/dev/null || echo "0")
if python3 -c "import sys; sys.exit(0 if float('$SCROLL_Y') >= 1 else 1)" 2>/dev/null; then
  echo "Theme B OK: window.scrollY = $SCROLL_Y (>= 1 after scrollTo)"
else
  echo "FAIL: scrollY=$SCROLL_Y — expected >= 1 after scrollTo(0, 100)" >&2
  exit 1
fi

# Verify script errors still surface.
if ffrdp --port "$PORT" eval 'throw new Error("boom")' 2>/dev/null; then
  echo "FAIL: eval of throw must exit non-zero" >&2
  exit 1
fi
echo "Theme C OK: script errors still surface (eval 'throw new Error' exited non-zero)"

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-93 dogfood: CSP bypass verified — $SENTINEL"
