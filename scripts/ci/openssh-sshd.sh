#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 STATE_DIR PORT" >&2
  exit 64
}

[[ $# -eq 2 ]] || usage
STATE_DIR=$1
PORT=$2
[[ -n $STATE_DIR && $STATE_DIR != *$'\r'* && $STATE_DIR != *$'\n'* ]] || usage
[[ $PORT =~ ^[0-9]+$ ]] && ((PORT > 0 && PORT < 65536)) || usage

SSHD=$(command -v sshd) || {
  echo "required tool not found: sshd" >&2
  exit 69
}
SSH_KEYGEN=$(command -v ssh-keygen) || {
  echo "required tool not found: ssh-keygen" >&2
  exit 69
}
USER_NAME=$(id -un)

umask 077
mkdir -p "$STATE_DIR"
STATE_DIR=$(cd -P "$STATE_DIR" && pwd)
[[ -n $STATE_DIR && $STATE_DIR != *$'\r'* && $STATE_DIR != *$'\n'* ]] || usage

quote_sshd_value() {
  local value=$1
  value=${value//\\/\\\\}
  value=${value//\"/\\\"}
  printf '"%s"' "$value"
}

HOST_KEY_CONFIG=$(quote_sshd_value "$STATE_DIR/host_key")
PID_FILE_CONFIG=$(quote_sshd_value "$STATE_DIR/sshd.pid")
AUTHORIZED_KEYS_CONFIG=$(quote_sshd_value "$STATE_DIR/authorized_keys")

cleanup() {
  rm -f "$STATE_DIR/sshd.pid"
}
trap cleanup EXIT INT TERM

"$SSH_KEYGEN" -q -t ed25519 -N '' -f "$STATE_DIR/host_key"
"$SSH_KEYGEN" -q -t ed25519 -N '' -f "$STATE_DIR/client_key"
cp "$STATE_DIR/client_key.pub" "$STATE_DIR/authorized_keys"
printf '%s\n' "$USER_NAME" >"$STATE_DIR/user"
printf '[127.0.0.1]:%s %s\n' "$PORT" "$(cat "$STATE_DIR/host_key.pub")" \
  >"$STATE_DIR/known_hosts"

cat >"$STATE_DIR/sshd_config" <<EOF
Port $PORT
ListenAddress 127.0.0.1
AddressFamily inet
HostKey $HOST_KEY_CONFIG
PidFile $PID_FILE_CONFIG
AuthorizedKeysFile $AUTHORIZED_KEYS_CONFIG
StrictModes no
PasswordAuthentication no
KbdInteractiveAuthentication no
ChallengeResponseAuthentication no
UsePAM no
PermitRootLogin prohibit-password
AllowUsers $USER_NAME
PermitUserRC no
PermitUserEnvironment no
DisableForwarding yes
X11Forwarding no
PermitTunnel no
Subsystem sftp internal-sftp
LogLevel ERROR
EOF

"$SSHD" -t -f "$STATE_DIR/sshd_config"
EFFECTIVE_CONFIG=$("$SSHD" -T -f "$STATE_DIR/sshd_config" \
  -C "user=$USER_NAME,host=localhost,addr=127.0.0.1")
grep -qx 'permituserrc no' <<<"$EFFECTIVE_CONFIG"
grep -qx 'disableforwarding yes' <<<"$EFFECTIVE_CONFIG"
grep -qx 'passwordauthentication no' <<<"$EFFECTIVE_CONFIG"
grep -qx 'kbdinteractiveauthentication no' <<<"$EFFECTIVE_CONFIG"

# The shell intentionally leaves no daemon child behind: the guarded process
# becomes sshd itself, so killing and waiting for the guard also reaps sshd.
trap - EXIT INT TERM
exec "$SSHD" -D -e -f "$STATE_DIR/sshd_config"
