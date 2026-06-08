#!/usr/bin/env bash
#
# Single source of truth for the project version.
#
# The workspace version in the root Cargo.toml ([workspace.package] version) is
# authoritative. The Tauri app derives its version from it (tauri.conf.json has
# no `version` key, so Tauri falls back to CARGO_PKG_VERSION), which is what the
# installers, bundle metadata and the in-app About screen (getVersion()) show.
#
# This script keeps the remaining, non-derivable copies in lockstep:
#   * root Cargo.toml      — the source value
#   * Cargo.lock           — workspace member entries
#   * app/package.json     — npm metadata
#   * app/package-lock.json
#
# Usage:
#   scripts/bump-version.sh <X.Y.Z>
#
# After it runs, review `git diff`, then commit and tag, e.g.:
#   git commit -am "release: vX.Y.Z" && git tag -a vX.Y.Z -m "release: vX.Y.Z"
#   git push origin main && git push origin vX.Y.Z   # the tag triggers the release workflow
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <X.Y.Z>" >&2
  exit 2
fi

new="$1"
if [[ ! "$new" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: '$new' is not a valid X.Y.Z semver" >&2
  exit 2
fi

# Resolve repo root from this script's location so it works from anywhere.
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

old="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if [[ -z "$old" ]]; then
  echo "error: could not read current version from Cargo.toml" >&2
  exit 1
fi
echo "bumping $old -> $new"

# 1) Source of truth: root Cargo.toml [workspace.package] version (the only
#    top-of-line `version = ` in this file; dependency versions are inline).
sed -i.bak 's/^version = ".*"/version = "'"$new"'"/' Cargo.toml && rm -f Cargo.toml.bak

# 2) npm metadata: npm rewrites both package.json and package-lock.json
#    consistently. --no-git-tag-version keeps git untouched; --allow-same-version
#    makes re-runs idempotent.
( cd app && npm version "$new" --no-git-tag-version --allow-same-version >/dev/null )

# 3) Cargo.lock: refresh the workspace member version entries from Cargo.toml.
cargo update --workspace --quiet

echo "done. Files changed:"
git --no-pager diff --name-only
echo
echo "Next: git commit -am \"release: v$new\" && git tag -a v$new -m \"release: v$new\""
echo "      git push origin main && git push origin v$new"
