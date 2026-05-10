#!/usr/bin/env bash
set -euo pipefail

komac new ractive.ff-rdp \
    --version 0.2.0 \
    --urls "https://github.com/ractive/ff-rdp/releases/download/v0.2.0/ff-rdp-x86_64-pc-windows-msvc.zip" "https://github.com/ractive/ff-rdp/releases/download/v0.2.0/ff-rdp-aarch64-pc-windows-msvc.zip" \
    --package-name "ff-rdp" \
    --publisher "ractive" \
    --publisher-url "https://github.com/ractive" \
    --package-url "https://github.com/ractive/ff-rdp" \
    --license "MIT" \
    --license-url "https://github.com/ractive/ff-rdp/blob/main/LICENSE" \
    --short-description "Use Firefox's Remote Debugging Protocol via a CLI" \
    --description "CLI for Firefox Remote Debugging Protocol — inspect, test, and automate Firefox from the terminal. Supports navigation, DOM queries, screenshots, accessibility audits, performance analysis, network monitoring, and more. Designed for both human users and AI agents." \
    --moniker "ff-rdp" \
    --author "James Bergamin" \
    --release-notes-url "https://github.com/ractive/ff-rdp/releases/tag/v0.2.0" \
    --submit
