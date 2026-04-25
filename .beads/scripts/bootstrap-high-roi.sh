#!/usr/bin/env bash
set -euo pipefail

if ! command -v bd >/dev/null 2>&1; then
  echo "bd is not installed. Install Beads first." >&2
  exit 1
fi

if [ ! -d .beads/embeddeddolt ] && [ ! -d .beads/dolt ]; then
  bd init --quiet
fi

python3 .beads/scripts/seed_high_roi.py "$@"
