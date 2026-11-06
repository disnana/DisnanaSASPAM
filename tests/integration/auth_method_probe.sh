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
sudo bash -c 'exec "$1" --check-config --config /etc/saspam/config.toml' bash "$ROOT/target/release/saspamd"
# A disabled ntfy provider must be rejected without sending a notification.
if sudo /usr/local/bin/saspam notify test ntfy >/dev/null 2>&1; then
  echo 'Disabled notification provider was accepted' >&2; exit 1
fi
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
LogLevel INFO
LogVerbose auth2-pubkey.c:userauth_pubkey:*
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


echo '== Research: verified SSH auth debug log available before PAM approval? =='
echo 'The key used here is a temporary CI-only public test key'
(
  timeout 20 "${SSH_BASE[@]}" 'printf SSH-PROBE-SUCCESS'
) >"$TMP/ssh-probe-out" 2>"$TMP/ssh-probe-err" &
SSH_PID=$!
echo "SSH client pid=$SSH_PID"
# A successful primary public-key auth will wait on the real PAM account gate.
for i in $(seq 1 30); do
  if sudo python3 - <<'PY' 2>/dev/null
import sys
sys.path.insert(0,'tests/integration')
from decision_bot import call
r = call({'action':'list'})
sys.exit(0 if any(x.get('user')=='saspam-ci' for x in r.get('requests',[])) else 1)
PY
  then break; fi
  sleep .2
done
echo '--- BEFORE APPROVAL: sanitized sshd log lines ---'
LOG=$(sudo journalctl -u saspam-sshd-ci --no-pager -o cat --since '2 minutes ago')
printf '%s\n' "$LOG" | grep -E 'authenticated [01] pkalg|Accepted (publickey|password)|Postponed publickey|PAM:' | tail -n 30 || :
if printf '%s\n' "$LOG" | grep -q 'authenticated 1 pkalg'; then
  echo 'OBSERVATION: verified pubkey result appears in pre-approval logs'
else
  echo 'OBSERVATION: verified pubkey result is NOT visible before PAM approval'
fi
echo '--- Approving the independent PAM gate ---'
sudo python3 tests/integration/decision_bot.py approve --timeout 6
wait "$SSH_PID"
cat "$TMP/ssh-probe-out"
echo '--- AFTER APPROVAL: sanitized sshd log lines ---'
sudo journalctl -u saspam-sshd-ci --no-pager -o cat --since '2 minutes ago' |
  grep -E 'authenticated [01] pkalg|Accepted (publickey|password)|Postponed publickey|PAM:' | tail -n 30 || :
