#!/usr/bin/env bash
# Check the actual ELF, rather than assuming the musl target implies static linkage.
set -euo pipefail
target=${1:-}
binary=${2:-}
case "$target" in
  x86_64-unknown-linux-musl) machine='Advanced Micro Devices X86-64' ;;
  aarch64-unknown-linux-musl) machine=AArch64 ;;
  *) echo 'Expected an x86_64 or aarch64 Linux musl target.' >&2; exit 1 ;;
esac
if [[ ! -f "$binary" || ! -x "$binary" ]]; then
  echo "Missing executable: $binary" >&2
  exit 1
fi
export LC_ALL=C
header=$(readelf -hW -- "$binary")
programs=$(readelf -lW -- "$binary")
dynamic=$(readelf -dW -- "$binary")
if ! grep -Eq 'Class:[[:space:]]+ELF64' <<< "$header" ||
   ! grep -Eq "Machine:[[:space:]]+$machine[[:space:]]*$" <<< "$header" ||
   ! grep -Eq 'Type:[[:space:]]+(EXEC|DYN)[[:space:]]' <<< "$header" ||
   ! grep -Eq '^[[:space:]]+LOAD[[:space:]]' <<< "$programs"; then
  echo "Expected a Linux ELF64 executable for $target: $binary" >&2
  exit 1
fi
if grep -Eq '^[[:space:]]+INTERP[[:space:]]' <<< "$programs" ||
   grep -Fq '(NEEDED)' <<< "$dynamic"; then
  echo "Executable requires a dynamic loader or shared library: $binary" >&2
  exit 1
fi
