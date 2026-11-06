#!/usr/bin/env bash
# Internal helper called after saspam uninstall --yes disables the PAM hook.
set -Eeuo pipefail
[[ $(id -u) -eq 0 ]] || { echo 'root only' >&2; exit 1; }
if grep -q 'pam_saspam.so' /etc/pam.d/sshd; then
  echo 'PAM still references SASPAM; refusing to uninstall the module' >&2; exit 1
fi
systemctl disable --now saspam-discord.service saspamd.service 2>/dev/null || :
rm -f /etc/systemd/system/saspamd.service /etc/systemd/system/saspam-discord.service
systemctl daemon-reload
rm -f /usr/local/bin/saspam /usr/local/bin/saspamd /usr/local/lib/security/pam_saspam.so
rm -f /opt/saspam/discord/bot.py /opt/saspam/discord/requirements.txt
rm -f /usr/local/lib/saspam/uninstall.sh /usr/local/lib/saspam/update.sh /usr/local/lib/saspam/rollback-upgrade.sh
echo 'Binaries and services removed. /etc/saspam and journal audit logs intentionally retained.'
echo 'To purge credentials after reviewing: sudo rm -f /etc/saspam/config.toml /etc/saspam/discord.env'
