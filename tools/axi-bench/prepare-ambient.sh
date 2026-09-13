#!/bin/bash
# Install only in a new private project; retain the product's actual hook shape.
set -eu
umask 077
binary="$1"
destination="$2"
mkdir "$destination"
destination="$(cd "$destination" && pwd -P)"
mkdir "$destination/project"
git -C "$destination/project" init -q
PATH="$(dirname "$binary"):$PATH"
export PATH
[ "$(command -v ff-rdp)" = "$binary" ]
cd "$destination/project"
"$binary" install-hook --claude --project > "$destination/install.json"
cp .claude/settings.json "$destination/installed-settings.json"
# Reject a changed product contract instead of silently measuring an imitation.
jq -e '.hooks.SessionStart | length == 1 and .[0].ff_rdp_managed == true and
  (.[0].hooks | length == 1 and .[0].type == "command" and .[0].command == "ff-rdp home --hook")' \
  "$destination/installed-settings.json" >/dev/null
shasum -a 256 "$destination/installed-settings.json" "$binary" > "$destination/identity.sha256"
