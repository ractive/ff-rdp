#!/usr/bin/env bash
# iter-93 dogfood gate — eval survives strict Content Security Policy sites.
#
# Verifies that `ff-rdp eval 'document.title'` works on a page that sets a
# strict CSP that would have blocked the old eval() isolation wrapper.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-93-eval-via-debugger-csp-bypass.dogfood.sh
set -euo pipefail

SENTINEL=/tmp/ff-rdp-iter-93-dogfood-ok
rm -f "$SENTINEL"

# Kill any stale Firefox launched by ff-rdp.
pkill -f 'firefox.*ff-rdp-profile' || true
sleep 1

# Launch headless Firefox on the default port.
ff-rdp launch --headless --port 6000
sleep 2

# Write a CSP fixture HTML file to /tmp.
# The <meta> tag enforces CSP even from file:// URLs.
FIXTURE_HTML=/tmp/ff-rdp-iter93-csp-fixture.html
cat > "$FIXTURE_HTML" <<'HTMLEOF'
<!DOCTYPE html>
<html>
<head>
  <meta http-equiv="Content-Security-Policy" content="script-src 'self'; object-src 'none'">
  <title>iter93-csp-fixture</title>
</head>
<body><div style="height:5000px">x</div></body>
</html>
HTMLEOF

FIXTURE_URL="file://${FIXTURE_HTML}"

# Navigate to the CSP fixture.
ff-rdp navigate --allow-unsafe-urls "$FIXTURE_URL" \
  || { echo "FAIL: navigate to CSP fixture failed" >&2; exit 1; }

# Evaluate document.title — must exit 0 on this branch.
EVAL_OUT=$(ff-rdp eval 'document.title') \
  || { echo "FAIL: eval 'document.title' exited non-zero (CSP still blocking?): $EVAL_OUT" >&2; exit 1; }

# Parse the result with Python (available everywhere; avoids a jq dep).
RESULT=$(python3 -c "import sys,json; d=json.loads('$EVAL_OUT'); print(d.get('results',''))" 2>/dev/null || echo "")
if [ "$RESULT" != "iter93-csp-fixture" ]; then
  echo "FAIL: eval result '$RESULT' != 'iter93-csp-fixture'" >&2
  echo "Full output: $EVAL_OUT" >&2
  exit 1
fi
echo "Theme A OK: eval 'document.title' = '$RESULT' on strict-CSP page"

# Verify scrollY eval also works.
SCROLL_OUT=$(ff-rdp eval 'window.scrollTo(0, 100); window.scrollY') \
  || { echo "FAIL: scrollY eval exited non-zero" >&2; exit 1; }
SCROLL_Y=$(python3 -c "import sys,json; d=json.loads('$SCROLL_OUT'); print(d.get('results',0))" 2>/dev/null || echo "0")
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

pkill -f 'firefox.*ff-rdp-profile' || true

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-93 dogfood: CSP bypass verified — $SENTINEL"
