#!/bin/sh
# Optional re-fetch/verifier for the 16 legacy catalogue oracle samples (CC0), pulled from a
# pinned upstream commit. The corpus is committed; run this only if it is missing or to verify.
# Usage: ./fetch.sh [--force]
set -eu

OWNER=thorsted
REPO=PRONOM_Research
REF=2c26686a0577028783b2f24977cdaf78e6ca39fb
SUBDIR=Submissions/iView/Samples
BASE="https://raw.githubusercontent.com/$OWNER/$REPO/$REF/$SUBDIR"

force=0
case "${1-}" in
    --force) force=1 ;;
    "") ;;
    *) echo "usage: $0 [--force]" >&2; exit 2 ;;
esac

cd -- "$(dirname -- "$0")"
[ -f SHA256SUMS ] || { echo "error: SHA256SUMS not found" >&2; exit 1; }

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum -- "$1" | cut -d ' ' -f 1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 -- "$1" | cut -d ' ' -f 1
    elif command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 -- "$1" | sed 's/^.*= *//'
    else
        echo "error: no SHA-256 tool (sha256sum, shasum, openssl) found" >&2
        exit 1
    fi
}

download() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$2" "$1"
    else
        echo "error: neither curl nor wget found" >&2
        exit 1
    fi
}

fetched=0
ok=0
failed=0

while read -r want name; do
    [ -n "$name" ] || continue
    if [ "$force" -eq 0 ] && [ -f "$name" ] && [ "$(sha256_of "$name")" = "$want" ]; then
        echo "ok      $name"
        ok=$((ok + 1))
        continue
    fi
    part="$name.part"
    if ! download "$BASE/$name" "$part"; then
        rm -f -- "$part"
        echo "FAILED  $name (download error)" >&2
        failed=$((failed + 1))
        continue
    fi
    got=$(sha256_of "$part")
    if [ "$got" != "$want" ]; then
        rm -f -- "$part"
        echo "FAILED  $name (sha256 mismatch: got $got, want $want)" >&2
        failed=$((failed + 1))
        continue
    fi
    mv -f -- "$part" "$name"
    echo "fetched $name"
    fetched=$((fetched + 1))
done < SHA256SUMS

echo "summary: fetched=$fetched ok=$ok failed=$failed"
[ "$failed" -eq 0 ]
