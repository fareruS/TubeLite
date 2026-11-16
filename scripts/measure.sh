#!/usr/bin/env bash
# Measurement protocol (Linux). Windows equivalent: scripts/measure.ps1
# 1. Launch a release build. 2. Sample sum of Private Bytes / PSS of app +
# all child processes every 5 s. 3. Run scripted scenarios.
# 4. Write CSV + summary to docs/measurements/<date>-<commit>.md
# This script is the protocol, not a claim: do not edit its output into a
# report as if measured until it has actually run against a release build.
set -euo pipefail
OUT="${1:-docs/measurements/$(date +%F)-$(git rev-parse --short HEAD 2>/dev/null || echo unknown).csv}"
mkdir -p "$(dirname "$OUT")"
echo "timestamp,scenario,rss_kb,pss_kb" > "$OUT"
echo "No release build exists yet (Phase 1). Run after Phase 2 playback lands."
echo "CSV header written to $OUT"
