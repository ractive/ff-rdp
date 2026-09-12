#!/usr/bin/env bash
set -euo pipefail
# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/dogfood-lib.sh"
SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?run via check-dogfood-script}"
rm -f "$SENTINEL"
dogfood_init

# Per-command environment assignments leave the real user configuration alone.
hook_home="$FF_RDP_DOGFOOD_WORKDIR/hook-home"
hook_codex="$FF_RDP_DOGFOOD_WORKDIR/custom-codex"
mkdir -p "$hook_home/.codex" "$hook_codex"
printf '[features]\nhooks = false\n' > "$hook_home/.codex/config.toml"
printf '[features]\nhooks = true\n' > "$hook_codex/config.toml"
hook_cli() {
    HOME="$hook_home" USERPROFILE="$hook_home" CODEX_HOME="$hook_codex" ffrdp install-hook "$@"
}
hook_cli --codex --dry-run | jq -e '.results.dry_run == true'
[ ! -e "$hook_codex/hooks.json" ]
hook_cli --codex | jq -e '.results.action == "installed" and (.results.next_step | contains("/hooks"))'
cp "$hook_codex/hooks.json" "$FF_RDP_DOGFOOD_WORKDIR/first.json"
hook_cli --codex | jq -e '.results.action == "no-op"'
cmp "$hook_codex/hooks.json" "$FF_RDP_DOGFOOD_WORKDIR/first.json"
printf '[features]\nhooks = false\n' > "$hook_codex/config.toml"
if hook_cli --codex > "$FF_RDP_DOGFOOD_WORKDIR/refused.json"; then
    dogfood_die "Codex installer accepted an explicit false gate"
fi
cmp "$hook_codex/hooks.json" "$FF_RDP_DOGFOOD_WORKDIR/first.json"
hook_cli --codex --uninstall | jq -e '.results.action == "uninstalled"'
[ ! -e "$hook_home/.codex/hooks.json" ]
if hook_cli --opencode --dry-run; then
    dogfood_die "OpenCode must retain its module-contract refusal"
fi

# Exercise the actual hook payload against a browser this dogfood run owns.
PORT="$(dogfood_free_port)"
dogfood_launch "$PORT"
printf '%s\n' "$DOGFOOD_LAUNCH_JSON"
ffrdp --port "$PORT" navigate --allow-unsafe-urls 'data:text/html,<h1>Hook target dogfood</h1><button>Continue</button>'
ffrdp --port "$PORT" home --hook | tee "$FF_RDP_DOGFOOD_WORKDIR/payload.txt"
grep -q 'Hook target dogfood' "$FF_RDP_DOGFOOD_WORKDIR/payload.txt"
grep -q 'Continue' "$FF_RDP_DOGFOOD_WORKDIR/payload.txt"
date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
