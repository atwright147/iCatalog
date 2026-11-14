#!/usr/bin/env bash
# Run only on a disposable native Linux systemd host (for example a hosted runner).
# Installs an isolated transient test unit/account; never touches icatalog.service.
set -euo pipefail

if [[ $# != 1 || ! -f $1 || ! -x $1 ]]; then
  echo 'Usage: sudo bash packaging/systemd/test.sh <built-native-icatalog>' >&2
  exit 2
fi
if [[ $(uname -s) != Linux || $EUID != 0 || ! -d /run/systemd/system ]]; then
  echo 'This check requires root on a disposable Linux host running systemd.' >&2
  exit 2
fi
for tool in systemctl systemd-analyze useradd userdel groupdel runuser getent od; do
  command -v "$tool" >/dev/null
done
systemctl show --property=Version >/dev/null

scratch=$(mktemp -d /run/icatalog-systemd-test.XXXXXX)
service_user="icatalog-test-$$"
service_name="$service_user.service"
unit="/run/systemd/system/$service_name"
created_user=0
created_unit=0
cleanup() {
  result=$?
  trap - EXIT
  if (( created_unit )); then
    if ! systemctl stop "$service_name"; then
      echo "Cannot stop test service; retained $unit and $scratch for inspection." >&2
      exit 1
    fi
    systemctl reset-failed "$service_name" >/dev/null 2>&1 || true
    rm -- "$unit"
    systemctl daemon-reload
  fi
  if (( created_user )); then
    userdel "$service_user"
    if getent group "$service_user" >/dev/null; then groupdel "$service_user"; fi
  fi
  rm -rf -- "$scratch"
  exit "$result"
}
trap cleanup EXIT
if [[ -e $unit || -e $unit.d ]] || getent passwd "$service_user" >/dev/null || getent group "$service_user" >/dev/null; then
  echo 'Refusing to reuse a pre-existing test service or account.' >&2
  exit 1
fi
useradd --system --user-group --home-dir "$scratch/catalogue" --no-create-home --shell /usr/sbin/nologin "$service_user"
created_user=1
chmod 0755 "$scratch"
install -d -m 0755 "$scratch/bin"
install -m 0755 "$1" "$scratch/bin/icatalog"
install -d -o "$service_user" -g "$service_user" -m 0700 "$scratch/catalogue"
install -d -o root -g "$service_user" -m 0750 "$scratch/security"
od -An -N32 -tx1 /dev/urandom | tr -d ' \n' > "$scratch/security/sync.token"
chown "root:$service_user" "$scratch/security/sync.token"
chmod 0640 "$scratch/security/sync.token"
cli="$scratch/bin/icatalog"
catalogue="$scratch/catalogue/catalogue.icatalog"
token="$scratch/security/sync.token"
port=$((20000 + $$ % 40000))
url="http://127.0.0.1:$port"

# Only identity, executable, paths and port differ from the shipped unit. Keep
# every sandbox, restart and stop directive intact for real systemd execution.
sed -e "s|/usr/local/bin/icatalog|$cli|g" \
    -e "s|/var/lib/icatalog|$scratch/catalogue|g" \
    -e "s|/etc/icatalog|$scratch/security|g" \
    -e "s|^User=icatalog$|User=$service_user|" \
    -e "s|^Group=icatalog$|Group=$service_user|" \
    -e "s|--port 8080|--port $port|" \
    packaging/systemd/icatalog.service > "$unit"
created_unit=1
systemd-analyze verify "$unit"
systemctl daemon-reload

# A missing catalogue must produce a failed start, never create a blank one.
systemctl start "$service_name" || true
for _ in {1..100}; do
  if [[ $(systemctl show -p ExecMainStatus --value "$service_name") != 0 ]]; then break; fi
  sleep 0.1
done
test "$(systemctl show -p ExecMainStatus --value "$service_name")" != 0
test ! -e "$catalogue"
systemctl stop "$service_name"
systemctl reset-failed "$service_name" >/dev/null 2>&1 || true

runuser -u "$service_user" -- "$cli" init "$catalogue"
systemctl start "$service_name"
ready=0
for _ in {1..100}; do
  if "$cli" clone "$scratch/client.icatalog" --url "$url" --token-file "$token" > "$scratch/clone.log" 2>&1; then
    ready=1
    break
  fi
  sleep 0.1
done
if (( ! ready )); then
  cat "$scratch/clone.log" >&2
  journalctl -u "$service_name" -n 30 --no-pager >&2
  exit 1
fi
systemctl is-active --quiet "$service_name"
if "$cli" clone "$scratch/denied.icatalog" --url "$url" > "$scratch/denied.log" 2>&1; then
  echo 'Service unexpectedly accepted an unauthenticated clone.' >&2
  exit 1
fi
test ! -e "$scratch/denied.icatalog"
"$cli" collection "$scratch/client.icatalog" create --name 'Survives systemd restart'
"$cli" sync "$scratch/client.icatalog" --url "$url" --token-file "$token"
systemctl stop "$service_name"
test "$(systemctl show -p ExecMainStatus --value "$service_name")" = 15
test "$(systemctl show -p Result --value "$service_name")" = success
runuser -u "$service_user" -- "$cli" open "$catalogue"
systemctl start "$service_name"
ready=0
for _ in {1..100}; do
  if "$cli" clone "$scratch/after-restart.icatalog" --url "$url" --token-file "$token" > "$scratch/clone.log" 2>&1; then
    ready=1
    break
  fi
  sleep 0.1
done
if (( ! ready )); then cat "$scratch/clone.log" >&2; exit 1; fi
"$cli" collection "$scratch/after-restart.icatalog" list > "$scratch/collections.txt"
grep -Fq 'Survives systemd restart' "$scratch/collections.txt"
echo 'systemd unit verified: missing-catalogue refusal, authenticated sync, SIGTERM stop and restart persistence passed.'
