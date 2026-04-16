#!/usr/bin/env bash
cd "$(dirname "$0")"
exec python3 -m http.server 8787
