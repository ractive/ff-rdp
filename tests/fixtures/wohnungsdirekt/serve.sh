#!/usr/bin/env bash
cd "$(dirname "$0")"
exec python3 -m http.server --bind 127.0.0.1 8787
