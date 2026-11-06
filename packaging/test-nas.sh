#!/usr/bin/env bash
# Verify and execute the packaged CLI on a native Linux runner.
set -euo pipefail
target=${1:-}
case "$target" in
  x86_64-unknown-linux-musl|aarch64-unknown-linux-musl) ;;
  *) echo 'Expected an x86_64 or aarch64 Linux musl target.' >&2; exit 1 ;;
esac
version=$(bash packaging/version.sh)
archive="${ICATALOG_RELEASE_OUTPUT_DIR:-target/release-assets}/icatalog-$version-$target.tar.gz"
if [[ $(tar -tzf "$archive") != icatalog ]]; then
  echo 'NAS archive must contain only the icatalog executable.' >&2
  exit 1
fi
mkdir -p target
scratch=$(mktemp -d target/nas-check.XXXXXX)
trap 'rm -rf "$scratch"' EXIT
tar -xzf "$archive" -C "$scratch"
bash packaging/check-static-elf.sh "$target" "$scratch/icatalog"
test "$("$scratch/icatalog" --version)" = "icatalog $version"
"$scratch/icatalog" init "$scratch/smoke.sqlite"
"$scratch/icatalog" open "$scratch/smoke.sqlite"
"$scratch/icatalog" status "$scratch/smoke.sqlite"
