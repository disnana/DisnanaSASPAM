#!/usr/bin/env bash
# Developer-only: build as an unprivileged user first, then sudo this script.
# Does not enable PAM. For audited production install, use a signed Release.
set -Eeuo pipefail
[[ $(id -u) -eq 0 ]] || { echo 'Run with sudo after cargo build --release --workspace' >&2; exit 1; }
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
for file in "$ROOT/target/release/saspam" "$ROOT/target/release/saspamd" "$ROOT/target/release/libpam_saspam.so"; do
  [[ -f "$file" ]] || { echo "Not built: $file" >&2; exit 1; }
done
if grep -q 'pam_saspam.so' /etc/pam.d/sshd 2>/dev/null; then
  echo 'Existing PAM gate detected; refusing unsafe overwrite' >&2; exit 1
fi
export DEBIAN_FRONTEND=noninteractive
apt-get install -y python3-venv python3-pip libpam0g >/dev/null
getent group saspam-bot >/dev/null || groupadd --system saspam-bot
id saspam-bot >/dev/null 2>&1 || useradd --system --gid saspam-bot --home-dir /var/lib/saspam-discord --create-home --shell /usr/sbin/nologin saspam-bot
install -d -m 0700 -o root -g root /etc/saspam
install -d -m 0755 -o root -g root /usr/local/lib/security /opt/saspam/discord /usr/local/lib/saspam
install -d -m 0700 -o saspam-bot -g saspam-bot /var/lib/saspam-discord
install -m 0755 "$ROOT/target/release/saspam" /usr/local/bin/saspam
install -m 0755 "$ROOT/target/release/saspamd" /usr/local/bin/saspamd
install -m 0644 "$ROOT/target/release/libpam_saspam.so" /usr/local/lib/security/pam_saspam.so
install -m 0644 "$ROOT/integrations/discord/bot.py" /opt/saspam/discord/bot.py
install -m 0644 "$ROOT/integrations/discord/requirements.txt" /opt/saspam/discord/requirements.txt
install -m 0755 "$ROOT/scripts/uninstall.sh" /usr/local/lib/saspam/uninstall.sh
install -m 0755 "$ROOT/scripts/update.sh" /usr/local/lib/saspam/update.sh
install -m 0755 "$ROOT/scripts/rollback-upgrade.sh" /usr/local/lib/saspam/rollback-upgrade.sh
install -d -m 0700 -o root -g root /var/lib/saspam
printf '%s\n' "$(tr -d '\n' < "$ROOT/VERSION")" > /var/lib/saspam/installed-version
chmod 0600 /var/lib/saspam/installed-version
python3 -m venv /opt/saspam/venv
/opt/saspam/venv/bin/python -m pip install --disable-pip-version-check --no-input -r /opt/saspam/discord/requirements.txt
# umask 077 protects secrets, but the isolated Bot needs to run its venv.
# Share only the interpreter/dependencies with the Bot group; never /etc/saspam.
chown -R root:saspam-bot /opt/saspam/venv
chmod -R u=rwX,g=rX,o= /opt/saspam/venv
runuser -u saspam-bot -- /opt/saspam/venv/bin/python -c 'import discord; print("SASPAM Bot Python OK:", discord.__version__)'
install -m 0644 "$ROOT/systemd/saspamd.service" /etc/systemd/system/saspamd.service
install -m 0644 "$ROOT/systemd/saspam-discord.service" /etc/systemd/system/saspam-discord.service
systemctl daemon-reload
echo 'Developer build installed, PAM unchanged. Next: sudo saspam setup && sudo saspam doctor && sudo saspam test'
