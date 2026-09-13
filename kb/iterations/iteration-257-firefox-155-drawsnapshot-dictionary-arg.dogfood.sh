#!/usr/bin/env bash
set -euo pipefail
# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/dogfood-lib.sh"
SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?run through check-dogfood-script}"
rm -f "$SENTINEL"
dogfood_init
PORT="$(dogfood_free_port)"
WORK="$(dogfood_workdir)"
RAW_PID=""
save_evidence() {
  if [ -n "${FF_RDP_257_EVIDENCE_DIR:-}" ]; then
    mkdir -p "$FF_RDP_257_EVIDENCE_DIR"
    for file in "$WORK"/*.json "$WORK"/*.png "$WORK"/*.trace "$WORK"/*.log; do
      [ ! -f "$file" ] || cp "$file" "$FF_RDP_257_EVIDENCE_DIR/"
    done
  fi
}
dogfood_on_exit save_evidence
cleanup_raw() {
  if [ -n "$RAW_PID" ]; then
    kill -TERM "$RAW_PID" 2>/dev/null || true
    wait "$RAW_PID" || true
  fi
}
dogfood_on_exit cleanup_raw
if [ -n "${FF_RDP_257_FIREFOX:-}" ]; then
  mkdir "$WORK/raw-profile"
  cat > "$WORK/raw-profile/user.js" <<'PREFS'
user_pref("devtools.debugger.remote-enabled", true);
user_pref("devtools.chrome.enabled", true);
user_pref("devtools.debugger.prompt-connection", false);
PREFS
  "$FF_RDP_257_FIREFOX" --version
  "$FF_RDP_257_FIREFOX" -no-remote -profile "$WORK/raw-profile" --start-debugger-server "$PORT" --headless > "$WORK/firefox.log" 2>&1 &
  RAW_PID=$!
  printf 'runtime=%s pid=%s port=%s profile=%s\n' "$FF_RDP_257_FIREFOX" "$RAW_PID" "$PORT" "$WORK/raw-profile"
  ready=0
  for attempt in $(seq 1 150); do
    if nc -z 127.0.0.1 "$PORT"; then ready=1; break; fi
    sleep 0.2
  done
  [ "$ready" = 1 ] || dogfood_die "raw Firefox debugger did not become ready"
else
  if ! dogfood_launch "$PORT"; then
    printf '%s\n' "$DOGFOOD_LAUNCH_JSON" >&2
    dogfood_die "managed Firefox launch failed"
  fi
fi
tabs_ready=0
for attempt in $(seq 1 100); do
  if ffrdp --no-daemon --port "$PORT" tabs > "$WORK/tabs.json" && jq -e '.results | length > 0' "$WORK/tabs.json" >/dev/null; then
    tabs_ready=1
    break
  fi
  sleep 0.1
done
[ "$tabs_ready" = 1 ] || dogfood_die "Firefox exposed no tab within the setup bound"
ffrdp --no-daemon --port "$PORT" navigate --allow-unsafe-urls "data:text/html,<body style='margin:0;height:4000px;background:linear-gradient(red,blue)'><header style='position:fixed;top:0'>header</header>iter257</body>" > "$WORK/navigate.json"
ffrdp --no-daemon --port "$PORT" eval 'window.scrollTo(0,500)' > "$WORK/scroll.json"
ffrdp --no-daemon --port "$PORT" eval 'JSON.stringify({y:scrollY,w:innerWidth,h:innerHeight,style:document.querySelector("header").getAttribute("style")})' > "$WORK/before.json"
for mode in viewport fullpage; do
  extra=()
  [ "$mode" != fullpage ] || extra=(--full-page)
  RUST_LOG=ff_rdp_cli::screenshot=debug ffrdp --no-daemon --port "$PORT" screenshot ${extra[@]+"${extra[@]}"} --output "$WORK/$mode.png" > "$WORK/$mode.json" 2> "$WORK/$mode.trace"
done
ffrdp --no-daemon --port "$PORT" eval 'JSON.stringify({y:scrollY,w:innerWidth,h:innerHeight,style:document.querySelector("header").getAttribute("style")})' > "$WORK/after.json"
jq -e -s '.[0].results == .[1].results' "$WORK/before.json" "$WORK/after.json"
jq -e -s '.[1].results.height == 4000 and .[1].results.height > .[0].results.height' "$WORK/viewport.json" "$WORK/fullpage.json"
for mode in viewport fullpage; do
  printf '%s IHDR width/height bytes: ' "$mode"
  od -An -tu1 -j16 -N8 "$WORK/$mode.png"
  jq '.results | {width,height,path}' "$WORK/$mode.json"
done
save_evidence
date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
