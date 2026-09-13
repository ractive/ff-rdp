#!/bin/bash
# A transparent recorder around the installed product command. No payload injection.
set -eu
umask 077
binary="$1"
audit="$2"
mkdir "$audit/hook" # A duplicate SessionStart must fail visibly, not overwrite evidence.
trap 'printf "%s\n" "$?" > "$audit/hook/exit"' EXIT
cat > "$audit/hook/input.json"
jq -e '.hook_event_name == "SessionStart"' "$audit/hook/input.json" >/dev/null
[ "$(command -v ff-rdp)" = "$binary" ]
jq -n --arg binary "$binary" --arg sha256 "$(shasum -a 256 "$binary" | cut -d' ' -f1)" \
  '{$binary,$sha256,argv:["home","--hook"]}' > "$audit/hook/identity.json"
set +e
"$binary" home --hook > "$audit/hook/stdout.txt" 2> "$audit/hook/stderr.txt"
status=$?
set -e
cat "$audit/hook/stdout.txt"
cat "$audit/hook/stderr.txt" >&2
shasum -a 256 "$audit/hook/stdout.txt" > "$audit/hook/stdout.sha256"
exit "$status"
