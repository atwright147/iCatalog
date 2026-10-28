#!/usr/bin/env bash
# Runs as root in a disposable distribution container, with the repo at /work.
set -euo pipefail
case "${1:-}" in
  deb)
    apt-get update
    DEBIAN_FRONTEND=noninteractive apt-get install -y /work/target/release-assets/*.deb
    ;;
  rpm) dnf install -y /work/target/release-assets/*.rpm ;;
  arch)
    pacman -Syu --noconfirm
    pacman -U --noconfirm /work/target/release-assets/*.pkg.tar.zst
    ;;
  *) echo 'Usage: test-linux-install.sh deb|rpm|arch' >&2; exit 1 ;;
esac
test -x /usr/bin/icatalog-ui
test -f /usr/share/applications/icatalog.desktop
# Check direct ELF dependencies of both executables, even without a display.
for binary in /usr/bin/icatalog /usr/bin/icatalog-ui; do
  links=$(ldd "$binary")
  if [[ "$links" == *'not found'* ]]; then
    printf '%s\n' "$links" >&2
    exit 1
  fi
done
icatalog --version
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
icatalog init "$scratch/smoke.sqlite"
icatalog open "$scratch/smoke.sqlite"
