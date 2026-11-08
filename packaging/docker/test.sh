#!/usr/bin/env bash
# Disposable real-container checks; no user data. Optional Compose checks publish
# only a temporary automatically allocated loopback port (ICATALOG_TEST_COMPOSE=1).
set -euo pipefail
if [[ $# -gt 1 ]]; then
  echo 'Usage: test.sh [image]' >&2
  exit 1
fi
cd "$(dirname "$0")/../.."
export ICATALOG_DOCKER_IMAGE=${1:-icatalog:local}
if [[ -z "$ICATALOG_DOCKER_IMAGE" || "$ICATALOG_DOCKER_IMAGE" == -* ]]; then
  echo 'Expected a nonempty image tag (for example icatalog:local).' >&2
  exit 1
fi
case "${ICATALOG_CONTAINER_ENGINE:-docker}" in
  docker|podman) ;;
  *) echo 'ICATALOG_CONTAINER_ENGINE must be docker or podman.' >&2; exit 1 ;;
esac
cargo test --locked -p icatalog-cli --test docker_packaging container_round_trip -- --ignored --nocapture
