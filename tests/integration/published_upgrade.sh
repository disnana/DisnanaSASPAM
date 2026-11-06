#!/usr/bin/env bash
# Run ONLY on a fresh disposable Ubuntu GitHub Actions runner, after Release publishing.
# The PAM gate stays absent here; real SSH/PAM approval tests are separate.
set -Eeuo pipefail
umask 077
TAG=$(tr -d '\n' < VERSION)
sudo install -d -m 0700 /etc/saspam
cat <<'CONF' | sudo tee /etc/saspam/config.toml >/dev/null
[general]
server_name = "CI-UPDATER-TEST"
timeout_seconds = 60
language = "ja"
[discord]
enabled = true
guild_id = "111111111111111111"
channel_id = "222222222222222222"
allowed_role_ids = ["333333333333333333"]
[ntfy]
enabled = false
server = "https://ntfy.sh"
topic = ""
[bark]
enabled = false
server = "https://api.day.app"
device_key = ""
CONF
cat <<'ENV' | sudo tee /etc/saspam/discord.env >/dev/null
DISCORD_TOKEN=CI-fake-unused-token
DISCORD_GUILD_ID=111111111111111111
DISCORD_CHANNEL_ID=222222222222222222
DISCORD_APPROVAL_ROLE_IDS=333333333333333333
DISCORD_MENTION_MODE=none
DISCORD_LANGUAGE=ja
ENV
sudo chmod 600 /etc/saspam/config.toml /etc/saspam/discord.env
# Only change one field. Verify it persists in both config and Bot env.
sudo saspam config set language en
sudo saspam config show | grep -q 'Language: en'
sudo grep -qx 'DISCORD_LANGUAGE=en' /etc/saspam/discord.env
sudo saspam config set language ja
sudo saspam config set ntfy topic --generate
sudo saspam config show | grep -q 'ntfy enabled: false (topic: '
# Disabled notification provider must never send a test push.
if sudo saspam notify test ntfy >/dev/null 2>&1; then
  echo 'Disabled ntfy notification unexpectedly succeeded' >&2; exit 1;
fi
# The TestDisk-style menu has a deterministic numbered fallback for CI.
printf '2\n0\n' | sudo saspam menu
[[ $(sudo cat /var/lib/saspam/installed-version) == "$TAG" ]] || {
  echo 'Installed version is not recorded' >&2; exit 1;
}
# Basic semantic version gate: do not permit alpha.3 downgrade without explicit consent.
if sudo saspam update --version v0.1.0-alpha.3 --dry-run >/tmp/saspam-downgrade-test.log 2>&1; then
  echo 'Downgrade was accepted without --allow-downgrade' >&2; exit 1;
fi
grep -q 'Downgrade requires --allow-downgrade' /tmp/saspam-downgrade-test.log
# Real published archive + transaction + watchdog + manual recovery.
sudo saspam update --version "$TAG" --allow-reinstall --skip-approval-test
[[ $(sudo cat /var/lib/saspam/installed-version) == "$TAG" ]] || exit 1
[[ -x /usr/local/lib/saspam/rollback-upgrade.sh ]] || exit 1
sudo saspam update --rollback
[[ $(sudo cat /var/lib/saspam/installed-version) == "$TAG" ]] || exit 1
sudo runuser -u saspam-bot -- /opt/saspam/venv/bin/python -c 'import discord; print(discord.__version__)'
if grep -q pam_saspam.so /etc/pam.d/sshd; then
  echo 'An updater unexpectedly changed the PAM stack' >&2; exit 1;
fi
echo "Published updater and recovery transaction succeeded: $TAG"
