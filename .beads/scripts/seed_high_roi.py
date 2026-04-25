#!/usr/bin/env python3
"""Seed the Qartez highest-ROI indexing improvements plan into Beads.

Usage:
  python3 .beads/scripts/seed_high_roi.py [--manifest path]

This script intentionally uses only very common `bd` commands:
- `bd init --quiet`
- `bd create ... --json`
- `bd dep add <child> <parent>`

It avoids trying to synthesize a Dolt database in advance.
"""
from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path


def run(cmd, cwd: Path) -> str:
    proc = subprocess.run(cmd, cwd=cwd, text=True, capture_output=True)
    if proc.returncode != 0:
        raise RuntimeError(
            f"command failed: {' '.join(cmd)}\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    return proc.stdout.strip()


def ensure_bd(cwd: Path) -> None:
    if shutil.which("bd") is None:
        raise SystemExit("`bd` was not found in PATH. Install Beads first.")
    runtime_a = cwd / ".beads" / "embeddeddolt"
    runtime_b = cwd / ".beads" / "dolt"
    if not runtime_a.exists() and not runtime_b.exists():
        print("Initializing Beads in embedded mode...", file=sys.stderr)
        run(["bd", "init", "--quiet"], cwd)


def create_issue(issue: dict, cwd: Path) -> str:
    cmd = [
        "bd",
        "create",
        issue["title"],
        "-p",
        str(issue["priority"]),
        "-t",
        issue.get("type", "task"),
        "--description",
        issue["description"],
        "--json",
    ]
    out = run(cmd, cwd)
    data = json.loads(out)
    issue_id = data.get("id")
    if not issue_id:
        raise RuntimeError(f"Could not find issue id in output: {out}")
    return issue_id


def add_dep(child_id: str, parent_id: str, cwd: Path) -> None:
    run(["bd", "dep", "add", child_id, parent_id], cwd)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--manifest",
        default=".beads/seeds/qartez-indexing-high-roi.json",
        help="path to seed manifest",
    )
    args = parser.parse_args()

    cwd = Path.cwd()
    manifest_path = cwd / args.manifest
    if not manifest_path.exists():
        raise SystemExit(f"Manifest not found: {manifest_path}")

    manifest = json.loads(manifest_path.read_text())
    ensure_bd(cwd)

    print(f"Seeding plan: {manifest['plan']['name']}")
    ids = {}
    for issue in manifest["issues"]:
        issue_id = create_issue(issue, cwd)
        ids[issue["key"]] = issue_id
        print(f"  created {issue['key']}: {issue_id}")

    for child_key, parent_key in manifest.get("dependencies", []):
        add_dep(ids[child_key], ids[parent_key], cwd)
        print(f"  dep: {ids[child_key]} blocked by {ids[parent_key]}")

    print("\nDone. Next steps:")
    print("  bd ready")
    print("  bd show <id>")
    print("  bd update <id> --claim")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
