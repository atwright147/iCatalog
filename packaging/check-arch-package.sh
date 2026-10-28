#!/usr/bin/env bash
# Validate generated pacman metadata before uploading the package.
set -euo pipefail
if [[ $# -ne 1 ]]; then
  echo 'Usage: check-arch-package.sh <package.pkg.tar.zst>' >&2
  exit 1
fi
metadata=$(tar -xOf "$1" .PKGINFO)
printf '%s\n' "$metadata" | awk -F ' = ' '
  $1 == "depend" {
    if ($2 == "" || $2 ~ /[[:space:]]/) {
      print "Invalid Arch dependency: " $2 > "/dev/stderr"
      invalid = 1
    }
    if ($2 == "glibc>=2.35") has_glibc = 1
  }
  END {
    if (!has_glibc) print "Missing Arch dependency: glibc>=2.35" > "/dev/stderr"
    exit (invalid || !has_glibc)
  }
'
