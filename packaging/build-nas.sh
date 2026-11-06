#!/usr/bin/env bash
# Build/test only the CLI on a Linux host of the requested architecture.
set -euo pipefail
action=${1:-}
target=${2:-}
case "$action:$target" in
  build:x86_64-unknown-linux-musl|test:x86_64-unknown-linux-musl) arch=x86_64 ;;
  build:aarch64-unknown-linux-musl|test:aarch64-unknown-linux-musl) arch=aarch64 ;;
  *) echo 'Usage: build-nas.sh build|test x86_64-unknown-linux-musl|aarch64-unknown-linux-musl' >&2; exit 1 ;;
esac
if [[ $(uname -s) != Linux || $(uname -m) != "$arch" ]]; then
  echo "NAS builds require a native Linux $arch host with musl-tools; use the NAS workflow on other hosts." >&2
  exit 1
fi
# Scope the C compiler/linker and static runtime to this target, including bundled SQLite.
# Older musl-gcc specs can add an interpreter even for Rust's static PIE output.
target_key=${target//-/_}
linker_key=$(printf '%s' "$target_key" | tr '[:lower:]' '[:upper:]')
env "CC_$target_key=musl-gcc" \
  "CARGO_TARGET_${linker_key}_LINKER=musl-gcc" \
  "CARGO_TARGET_${linker_key}_RUSTFLAGS=-C target-feature=+crt-static -C link-arg=-Wl,--no-dynamic-linker" \
  cargo "$action" --locked --release -p icatalog-cli --target "$target"
if [[ "$action" == build ]]; then
  bash packaging/check-static-elf.sh "$target" "${CARGO_TARGET_DIR:-target}/$target/release/icatalog"
fi
