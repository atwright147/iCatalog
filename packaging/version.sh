#!/usr/bin/env bash
# Run from the repository root. An optional release tag must match both binaries.
set -euo pipefail

version=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' crates/icatalog-cli/Cargo.toml)
ui_version=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' crates/icatalog-ui/Cargo.toml)
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ || "$version" != "$ui_version" ]]; then
  echo 'CLI and UI versions must match (X.Y.Z or X.Y.Z-prerelease, without build metadata).' >&2
  exit 1
fi
if [[ $# -gt 0 && "${1#v}" != "$version" ]]; then
  echo "Release tag must be v$version or $version; update both Cargo manifests before tagging." >&2
  exit 1
fi
printf '%s\n' "$version"
