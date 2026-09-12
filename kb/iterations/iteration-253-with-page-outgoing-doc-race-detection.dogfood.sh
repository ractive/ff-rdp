#!/usr/bin/env bash
set -euo pipefail
# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/dogfood-lib.sh"
SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?run via check-dogfood-script}"
rm -f "$SENTINEL"
dogfood_init
PORT="$(dogfood_free_port)"
dogfood_launch "$PORT"
ffrdp --port "$PORT" eval 'document.body.innerHTML = "<h1>Control</h1><button>Stay</button>"'
CONTROL="$(ffrdp --port "$PORT" click button --with-page --jq '.meta.page_ready')"
[ "$CONTROL" = true ] || dogfood_die "non-navigating control was not ready: $CONTROL"
# These fixture routes exercise the same real CLI with the full reader payload,
# on both direct and daemon connections, before and beyond the settle bound.
FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_253_outgoing_page \
  -- --include-ignored --nocapture --test-threads=1
date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
