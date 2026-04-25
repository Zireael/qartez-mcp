$ErrorActionPreference = "Stop"

if (-not (Get-Command bd -ErrorAction SilentlyContinue)) {
  throw "bd is not installed. Install Beads first."
}

if (-not (Test-Path ".beads/embeddeddolt") -and -not (Test-Path ".beads/dolt")) {
  bd init --quiet
}

python .beads/scripts/seed_high_roi.py @args
