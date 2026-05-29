#!/usr/bin/env bash
# Fixture: triggers missing-sentinel-pattern rule (no sentinel at all).
set -euo pipefail

echo "doing something but never writing a sentinel"
