#!/usr/bin/env bash
# Package an existing native musl build. No repository/media enters the build context.
set -euo pipefail
target=${1:-}
image=${2:-icatalog:local}
case "$target" in
  x86_64-unknown-linux-musl) platform=linux/amd64 ;;
  aarch64-unknown-linux-musl) platform=linux/arm64 ;;
  *) echo 'Usage: build.sh x86_64-unknown-linux-musl|aarch64-unknown-linux-musl [image]' >&2; exit 1 ;;
esac
if [[ $# -gt 2 || -z "$image" || "$image" == -* ]]; then
  echo 'Expected a nonempty image tag (for example icatalog:local).' >&2
  exit 1
fi
cd "$(dirname "$0")/../.."
binary=${ICATALOG_DOCKER_BINARY:-${CARGO_TARGET_DIR:-target}/$target/release/icatalog}
command -v readelf >/dev/null || { echo 'Install binutils (readelf) to verify the static Linux CLI before packaging.' >&2; exit 1; }
bash packaging/check-static-elf.sh "$target" "$binary"
engine=${ICATALOG_CONTAINER_ENGINE:-docker}
case "$engine" in
  docker|podman) ;;
  *) echo 'ICATALOG_CONTAINER_ENGINE must be docker or podman.' >&2; exit 1 ;;
esac
command -v "$engine" >/dev/null || { echo "Install $engine before building the image." >&2; exit 1; }
version=$(bash packaging/version.sh)
context=$(mktemp -d "${TMPDIR:-/tmp}/icatalog-docker.XXXXXX")
trap 'rm -rf "$context"' EXIT
cp "$binary" "$context/icatalog"
cp packaging/docker/Dockerfile "$context/Dockerfile"
mkdir "$context/catalogue" "$context/media"
chmod 755 "$context/icatalog" "$context/catalogue" "$context/media"
"$engine" build --platform "$platform" --build-arg "VERSION=$version" --tag "$image" "$context"
