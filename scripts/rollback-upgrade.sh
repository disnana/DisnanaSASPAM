#!/usr/bin/env bash
# DisnanaSASPAM privileged transaction rollback.
# Never removes the PAM gate. The system remains fail closed if recovery fails.
set -Eeuo pipefail
umask 077
[[ $(id -u) -eq 0 ]] || { echo 'rollback requires root' >&2; exit 1; }
id="${1:-}"
mode="${2:-}"
[[ "$id" =~ ^tx-[a-zA-Z0-9]{8,16}$ ]] || { echo 'invalid transaction ID' >&2; exit 2; }
TX="/var/lib/saspam/transactions/$id"
[[ -d "$TX" && ! -L "$TX" ]] || { echo "transaction not found: $id" >&2; exit 1; }
[[ $(stat -c '%u' "$TX") == 0 ]] || { echo 'transaction directory is not owned by root' >&2; exit 1; }
exec 9>/run/lock/saspam-upgrade.lock
flock -w 90 9 || { echo 'cannot obtain upgrade recovery lock' >&2; exit 1; }
state=$(cat "$TX/state" 2>/dev/null || :)
case "$state:$mode" in
  pending:*|rolling-back:*|committed:--manual) ;;
  committed:*) exit 0 ;; # Timer runs after successful commit; do nothing.
  rolled-back:*) exit 0 ;;
  *) echo "transaction $id is not eligible for rollback ($state)" >&2; exit 1 ;;
esac
BACKUP="$TX/backup"
for file in saspam saspamd pam_saspam.so bot.py requirements.txt saspamd.service saspam-discord.service; do
  [[ -f "$BACKUP/$file" && ! -L "$BACKUP/$file" ]] || { echo "backup missing: $file" >&2; exit 1; }
done
[[ -d "$BACKUP/venv" && -f "$BACKUP/config.toml" && -f "$BACKUP/discord.env" ]] || {
  echo 'backup missing venv or configuration' >&2; exit 1;
}
printf 'rolling-back\n' > "$TX/state"
# Keep the watchdog service resumable if this rollback process is interrupted.
systemctl stop saspam-discord.service saspamd.service 2>/dev/null || :
# Stage replacement files, then atomic rename; never truncate live ELF objects.
install -m 0755 "$BACKUP/saspam" /usr/local/bin/saspam.rollback
mv -f /usr/local/bin/saspam.rollback /usr/local/bin/saspam
install -m 0755 "$BACKUP/saspamd" /usr/local/bin/saspamd.rollback
mv -f /usr/local/bin/saspamd.rollback /usr/local/bin/saspamd
install -m 0644 "$BACKUP/pam_saspam.so" /usr/local/lib/security/pam_saspam.so.rollback
mv -f /usr/local/lib/security/pam_saspam.so.rollback /usr/local/lib/security/pam_saspam.so
install -m 0644 "$BACKUP/bot.py" /opt/saspam/discord/bot.py
install -m 0644 "$BACKUP/requirements.txt" /opt/saspam/discord/requirements.txt
install -m 0644 "$BACKUP/saspamd.service" /etc/systemd/system/saspamd.service
install -m 0644 "$BACKUP/saspam-discord.service" /etc/systemd/system/saspam-discord.service
if [[ -d /opt/saspam/venv ]]; then
  mv /opt/saspam/venv "$TX/failed-venv-$(date +%s)"
fi
cp -a "$BACKUP/venv" /opt/saspam/venv
install -m 0600 "$BACKUP/config.toml" /etc/saspam/config.toml
install -m 0600 "$BACKUP/discord.env" /etc/saspam/discord.env
if [[ -f "$BACKUP/installed-version" ]]; then
  install -m 0600 "$BACKUP/installed-version" /var/lib/saspam/installed-version
else
  rm -f /var/lib/saspam/installed-version
fi
systemctl daemon-reload
failure=0
if [[ $(cat "$TX/was-active-daemon") == 1 ]]; then
  systemctl restart saspamd.service || failure=1
else
  systemctl stop saspamd.service 2>/dev/null || :
fi
if [[ $(cat "$TX/was-active-bot") == 1 ]]; then
  systemctl restart saspam-discord.service || failure=1
else
  systemctl stop saspam-discord.service 2>/dev/null || :
fi
if [[ "$failure" != 0 ]]; then
  echo "Recovery could not restart old services; SSH remains Fail Closed. Use the console." >&2
  exit 1
fi
printf 'rolled-back\n' > "$TX/state"
echo "DisnanaSASPAM restored previous transaction $id (PAM gate was never disabled)"
