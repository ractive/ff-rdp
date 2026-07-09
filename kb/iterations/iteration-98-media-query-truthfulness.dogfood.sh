#!/usr/bin/env bash
# iter-98 dogfood gate — media-query truthfulness.
#
# Drives `responsive` at 390 and 1280 against a live media-query fixture,
# asserts the media_query_check self-check is present and truthful at both
# widths, then runs `cascade` on the media-overridden `width` property asserting
# the winner equals computed. Executed by the dogfood linter/runner.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-98-*.dogfood.sh
set -euo pipefail

SENTINEL=/tmp/ff-rdp-iter-98-dogfood-ok
rm -f "$SENTINEL"

# Fresh Firefox — avoid cross-run state pollution.
pkill -f 'firefox.*ff-rdp-profile' || true
sleep 1
ff-rdp launch --headless --port 6000
sleep 2

# A self-contained fixture: #probe is 390px by default and 980px inside
# @media (min-width: 1024px). Mirrors the field-report scenario.
FIXTURE="data:text/html;charset=utf-8,<!DOCTYPE html><html><head><style>#probe{width:390px}@media (min-width: 1024px){#probe{width:980px}}</style></head><body><div id='probe'>x</div></body></html>"
ff-rdp navigate "$FIXTURE"

# --- Theme A: responsive self-check present at 390 and 1280 ---
# The media_query_check.requested must echo the requested width at each
# breakpoint (the self-check ran). Over RDP the emulation is layout-only, so we
# do not assert `matches` — only that the truthful self-check object is present.
REQ_390=$(ff-rdp responsive '#probe' --widths 390 --jq '.results.breakpoints[0].media_query_check.requested')
test "$REQ_390" = "390" || { echo "FAIL Theme A: 390 media_query_check.requested=$REQ_390 (expected 390)" >&2; exit 1; }

REQ_1280=$(ff-rdp responsive '#probe' --widths 1280 --jq '.results.breakpoints[0].media_query_check.requested')
test "$REQ_1280" = "1280" || { echo "FAIL Theme A: 1280 media_query_check.requested=$REQ_1280 (expected 1280)" >&2; exit 1; }

# --- Theme B: cascade winner respects media context and equals computed ---
# At the headless-default (wide) viewport the (min-width: 1024px) block is
# active, so computed width is 980px and the media-active override must win.
COMPUTED=$(ff-rdp cascade '#probe' --prop width --jq '.results[0].computed')
test "$COMPUTED" = "980px" || { echo "FAIL Theme B: computed width=$COMPUTED (expected 980px)" >&2; exit 1; }

WINNER_VAL=$(ff-rdp cascade '#probe' --prop width --jq '.results[0].rules[] | select(.winner == true) | .value')
test "$WINNER_VAL" = "980px" || { echo "FAIL Theme B: winner value=$WINNER_VAL (expected 980px == computed)" >&2; exit 1; }

VERIFIED=$(ff-rdp cascade '#probe' --prop width --jq '.results[0].winner_verified')
test "$VERIFIED" = "true" || { echo "FAIL Theme B: winner_verified=$VERIFIED (expected true)" >&2; exit 1; }

pkill -f 'firefox.*ff-rdp-profile' || true

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-98 dogfood: responsive self-check + media-aware cascade winner verified — $SENTINEL"
