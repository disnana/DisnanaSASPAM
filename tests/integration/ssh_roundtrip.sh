#!/usr/bin/env bash
# Real local-only SSH/PAM tests on an ephemeral GitHub Actions Ubuntu runner.
set -Eeuo pipefail
ROOT=$(pwd)
TMP=$(mktemp -d)
PORT=22387
cleanup() {
  sudo systemctl stop saspam-sshd-ci.service saspamd-ci.service saspam-discord.service 2>/dev/null || :
  if [[ -f "$TMP/pam-original" ]]; then sudo cp "$TMP/pam-original" /etc/pam.d/sshd; fi
  sudo userdel -r saspam-ci 2>/dev/null || :
  rm -rf "$TMP"
}
trap cleanup EXIT

sudo groupadd --system -f saspam-bot
id saspam-bot >/dev/null 2>&1 || sudo useradd --system --gid saspam-bot --home-dir /var/lib/saspam-discord --shell /usr/sbin/nologin saspam-bot
sudo install -d -m 0700 /etc/saspam
sudo install -d -m 0755 /usr/local/lib/security
sudo install -m 0644 target/release/libpam_saspam.so /usr/local/lib/security/pam_saspam.so
sudo tee /etc/saspam/config.toml >/dev/null <<'CONF'
[general]
server_name = "CI-SSH-POSTAUTH"
timeout_seconds = 10
[discord]
enabled = true
guild_id = "111111111111111111"
channel_id = "222222222222222222"
allowed_role_ids = ["333333333333333333", "444444444444444444"]
CONF
sudo chmod 0600 /etc/saspam/config.toml
sudo systemd-run --unit=saspamd-ci --property=Restart=no "$ROOT/target/release/saspamd" --config /etc/saspam/config.toml
for _ in $(seq 1 60); do
  if sudo test -S /run/saspam/pam.sock && sudo test -S /run/saspam/bot.sock; then break; fi
  sleep 0.3
done
sudo test -S /run/saspam/pam.sock && sudo test -S /run/saspam/bot.sock || {
  sudo journalctl -u saspamd-ci --no-pager -n 50
  echo 'Daemon IPC unavailable' >&2; exit 1;
}

sudo useradd -m -s /bin/bash saspam-ci
echo 'saspam-ci:only-CI-test-password-123' | sudo chpasswd
ssh-keygen -q -t ed25519 -N '' -f "$TMP/key" >/dev/null
sudo install -d -m 0700 -o saspam-ci /home/saspam-ci/.ssh
sudo install -m 0600 -o saspam-ci "$TMP/key.pub" /home/saspam-ci/.ssh/authorized_keys
sudo ssh-keygen -A >/dev/null
sudo install -d -m 0755 /run/sshd
sudo cp /etc/pam.d/sshd "$TMP/pam-original"
sudo install -m 0755 target/release/saspam /usr/local/bin/saspam
sudo tee /etc/saspam/discord.env >/dev/null <<'ENV'
DISCORD_TOKEN=CI-only-not-a-real-Discord-token
DISCORD_GUILD_ID=111111111111111111
DISCORD_CHANNEL_ID=222222222222222222
DISCORD_APPROVAL_ROLE_IDS=333333333333333333,444444444444444444
ENV
sudo chmod 0600 /etc/saspam/discord.env
# The live Discord API is not contacted by this test; its non-root adapter is
# replaced by a harmless active service, and a mock socket client casts votes.
sudo systemd-run --unit=saspam-discord --property=Restart=no /bin/sleep 180
sudo saspam doctor
echo '== CLI approval test BEFORE any PAM modification =='
sudo python3 tests/integration/decision_bot.py approve --user "$USER" --timeout 15 &
BOT_PID=$!
sudo saspam test
wait "$BOT_PID"
sudo saspam enable --ack-recovery --ack-tested
grep -Fq 'account required /usr/local/lib/security/pam_saspam.so' /etc/pam.d/sshd

cat >"$TMP/sshd.conf" <<EOF
Port $PORT
ListenAddress 127.0.0.1
HostKey /etc/ssh/ssh_host_ed25519_key
PidFile /run/sshd/saspam-ci.pid
AuthorizedKeysFile .ssh/authorized_keys
UsePAM yes
PasswordAuthentication yes
PubkeyAuthentication yes
KbdInteractiveAuthentication no
PermitRootLogin no
AllowUsers saspam-ci
MaxAuthTries 3
LogLevel VERBOSE
EOF
sudo /usr/sbin/sshd -t -f "$TMP/sshd.conf"
sudo systemd-run --unit=saspam-sshd-ci --property=Restart=no /usr/sbin/sshd -D -e -f "$TMP/sshd.conf"
for _ in $(seq 1 50); do
  if python3 - "$PORT" <<'PY' >/dev/null 2>&1
import socket,sys
with socket.create_connection(("127.0.0.1",int(sys.argv[1])),timeout=.25): pass
PY
  then break; fi
  sleep 0.2
done

SSH_BASE=(ssh -p "$PORT" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null
  -o ConnectTimeout=5 -o ServerAliveInterval=5 -o ServerAliveCountMax=3
  -o BatchMode=yes -i "$TMP/key" saspam-ci@127.0.0.1)
PW_BASE=(sshpass -p 'only-CI-test-password-123' ssh -p "$PORT"
  -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null
  -o ConnectTimeout=5 -o PubkeyAuthentication=no -o PreferredAuthentications=password
  -o NumberOfPasswordPrompts=1 saspam-ci@127.0.0.1)

echo '== Invalid primary credential MUST NOT reach post-auth PAM =='
START=$(date +%s)
if timeout 12 sshpass -p 'invalid-test-only' ssh -p "$PORT" \
  -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
  -o ConnectTimeout=5 -o PubkeyAuthentication=no -o PreferredAuthentications=password \
  -o NumberOfPasswordPrompts=1 saspam-ci@127.0.0.1 'printf BAD-ACCESS'; then
  echo 'Invalid SSH password was accepted' >&2; exit 1
fi
ELAPSED=$(( $(date +%s) - START ))
[[ "$ELAPSED" -lt 9 ]] || { echo "Failed credential unexpectedly waited for PAM approval ($ELAPSED s)" >&2; exit 1; }
sudo python3 - <<'PY'
import sys
sys.path.insert(0,"tests/integration")
from decision_bot import call
reply=call({"action":"list"})
assert reply["ok"] and not reply["requests"], "Failed primary auth incorrectly created a pending request"
PY

echo '== Public key needs approval =='
sudo python3 tests/integration/decision_bot.py approve --timeout 15 &
BOT_PID=$!
OUT=$(timeout 26 "${SSH_BASE[@]}" 'printf APPROVED')
wait "$BOT_PID"
[[ "$OUT" == "APPROVED" ]] || { echo "Unexpected key SSH output: $OUT" >&2; exit 1; }
echo '== Password needs approval =='
sudo python3 tests/integration/decision_bot.py approve --timeout 15 &
BOT_PID=$!
OUT=$(timeout 26 "${PW_BASE[@]}" 'printf APPROVED-PASSWORD')
wait "$BOT_PID"
[[ "$OUT" == "APPROVED-PASSWORD" ]] || { echo "Unexpected password SSH output: $OUT" >&2; exit 1; }
echo '== Deny prevents session =='
sudo python3 tests/integration/decision_bot.py deny --timeout 15 &
BOT_PID=$!
if timeout 26 "${SSH_BASE[@]}" 'printf BAD-ACCESS'; then echo 'SSH connected after DENY' >&2; exit 1; fi
wait "$BOT_PID"
echo '== Timeout denies session =='
START=$(date +%s)
if timeout 27 "${SSH_BASE[@]}" 'printf BAD-ACCESS'; then echo 'SSH connected without approval' >&2; exit 1; fi
ELAPSED=$(( $(date +%s) - START ))
[[ "$ELAPSED" -ge 8 && "$ELAPSED" -le 26 ]] || { echo "Bad expiration: $ELAPSED" >&2; exit 1; }
echo '== Daemon offline denies session =='
sudo systemctl stop saspamd-ci.service
if timeout 16 "${SSH_BASE[@]}" 'printf BAD-ACCESS'; then echo 'SSH connected without daemon' >&2; exit 1; fi
echo '== CLI disable removes only SASPAM hook even while daemon is down =='
sudo saspam disable
if grep -Fq 'pam_saspam.so' /etc/pam.d/sshd; then echo 'CLI failed to remove PAM hook' >&2; exit 1; fi

echo '== Local PAM stacks untouched =='
if grep -q 'pam_saspam' /etc/pam.d/login /etc/pam.d/sudo /etc/pam.d/common-account; then echo 'Unexpected local PAM changes' >&2; exit 1; fi
echo 'Live SSH/PAM tests passed'
