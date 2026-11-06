#!/usr/bin/env bash
# DisnanaSASPAM staged update / downgrade. Never edits /etc/pam.d/sshd.
# No unverified firewall rules: --block-new-ssh is intentionally unsupported.
set -Eeuo pipefail
umask 077
[[ $(id -u) -eq 0 ]] || { echo 'Run saspam update as root' >&2; exit 1; }
TAG=""
DOWNGRADE=0
REINSTALL=0
SKIP_TEST=0
DRY_RUN=0
MANUAL_ROLLBACK=0
while (( $# > 0 )); do
  case "$1" in
    --version) TAG="${2:?missing release tag}"; shift 2 ;;
    --allow-downgrade) DOWNGRADE=1; shift ;;
    --allow-reinstall) REINSTALL=1; shift ;;
    --skip-approval-test) SKIP_TEST=1; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    --rollback) MANUAL_ROLLBACK=1; shift ;;
    --block-new-ssh) echo 'SSH blocking is not yet verified: refusing unsafe firewall changes' >&2; exit 2 ;;
    *) echo "Unknown upgrade option: $1" >&2; exit 2 ;;
  esac
done
HELPER=/usr/local/lib/saspam/rollback-upgrade.sh
[[ -x "$HELPER" ]] || { echo 'Recovery helper is missing; refusing update' >&2; exit 1; }
command -v flock >/dev/null || { echo 'flock is required' >&2; exit 1; }
install -d -m 0700 /var/lib/saspam/transactions
# Reject concurrent transactions. The recovery timer uses a separate lock.
exec 8>/run/lock/saspam-updater-singleton.lock
flock -n 8 || { echo 'Another SASPAM update is already running' >&2; exit 1; }
if (( MANUAL_ROLLBACK )); then
  LAST=$(cat /var/lib/saspam/last-committed-tx 2>/dev/null || :)
  [[ "$LAST" =~ ^tx-[a-zA-Z0-9]{8,16}$ ]] || { echo 'No committed update to roll back' >&2; exit 1; }
  exec "$HELPER" "$LAST" --manual
fi
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.-]+)?$ ]] || {
  echo 'Usage: sudo saspam update --version vX.Y.Z[-alpha.N] [--allow-downgrade|--dry-run]' >&2; exit 2;
}
case "$(uname -m)" in
  x86_64) ARCH=amd64 ;;
  aarch64) ARCH=arm64 ;;
  *) echo 'Only amd64 and arm64 are supported' >&2; exit 1 ;;
esac
CURRENT=$(cat /var/lib/saspam/installed-version 2>/dev/null || :)
if [[ -n "$CURRENT" && ! "$CURRENT" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.-]+)?$ ]]; then
  echo 'Unrecognized installed-version; cannot safely order releases' >&2; exit 1
fi
if [[ -n "$CURRENT" ]]; then
  if [[ "$TAG" == "$CURRENT" && $REINSTALL -ne 1 ]]; then
    echo 'Already on this version; use --allow-reinstall if intended' >&2; exit 1;
  fi
  # GNU sort -V by itself puts 1.0.0 before 1.0.0-alpha.1.
  # Normalize hyphens to tildes for correct SemVer prerelease precedence.
  TARGET_SEMVER="${TAG/-/~}"
  CURRENT_SEMVER="${CURRENT/-/~}"
  FIRST=$(printf '%s\n' "$TARGET_SEMVER" "$CURRENT_SEMVER" | sort -V | head -n 1)
  if [[ "$FIRST" == "$TARGET_SEMVER" && "$TAG" != "$CURRENT" && $DOWNGRADE -ne 1 ]]; then
    echo 'Downgrade requires --allow-downgrade' >&2; exit 1;
  fi
fi
PAM_GATED=0
if grep -Fqx 'account required /usr/local/lib/security/pam_saspam.so # DisnanaSASPAM-managed' /etc/pam.d/sshd 2>/dev/null; then
  PAM_GATED=1
fi
if (( PAM_GATED && SKIP_TEST )); then
  echo 'Cannot skip Discord approval test while PAM is enabled' >&2; exit 1
fi
if (( PAM_GATED )); then
  systemctl is-active --quiet saspamd.service && systemctl is-active --quiet saspam-discord.service || {
    echo 'Existing PAM gate is enabled but services are unhealthy. Use a recovery console; refusing update.' >&2
    exit 1
  }
fi
for program in curl tar sha256sum python3 systemd-run systemctl timeout; do
  command -v "$program" >/dev/null || { echo "Missing dependency: $program" >&2; exit 1; }
done
ASSET="saspam-$TAG-linux-$ARCH.tar.gz"
BASE="https://github.com/disnana/DisnanaSASPAM/releases/download/$TAG"
TX=$(mktemp -d /var/lib/saspam/transactions/tx-XXXXXXXX)
id=$(basename "$TX")
BACKUP="$TX/backup"
mkdir -m 0700 "$BACKUP" "$TX/bundle"
echo "$TAG" > "$TX/target"
ROLLBACK_NEEDED=0
on_exit() {
  status=$?
  trap - EXIT
  if (( status != 0 && ROLLBACK_NEEDED )); then
    exec 9>&-  # Release the lock before running the recovery helper.
    "$HELPER" "$id" || echo "CRITICAL: rollback failed, use the recovery console (transaction $id)" >&2
  elif (( status != 0 )); then
    # No live files have changed; discard temporary staging only.
    if [[ -n "${NEW_VENV:-}" && "$NEW_VENV" == /opt/saspam/.venv.next-tx-* ]]; then
      rm -rf -- "$NEW_VENV"
    fi
  fi
  exit "$status"
}
trap on_exit EXIT
cd "$TX"
curl --proto '=https' --tlsv1.2 -fsSL --retry 2 -o SHA256SUMS "$BASE/SHA256SUMS"
curl --proto '=https' --tlsv1.2 -fsSL --retry 2 -o "$ASSET" "$BASE/$ASSET"
SUM=$(awk -v file="$ASSET" '$2==file {print $1}' SHA256SUMS)
[[ "$SUM" =~ ^[0-9a-fA-F]{64}$ ]] || { echo 'Missing valid checksum' >&2; exit 1; }
printf '%s  %s\n' "$SUM" "$ASSET" | sha256sum -c -
while IFS= read -r member; do
  case "$member" in
    saspam|saspamd|pam_saspam.so|bot.py|requirements.txt|saspamd.service|saspam-discord.service|uninstall.sh|update.sh|rollback-upgrade.sh) ;;
    *) echo "Unexpected archive member: $member" >&2; exit 1 ;;
  esac
done < <(tar -tzf "$ASSET")
tar -tvzf "$ASSET" | awk '$1 !~ /^-/ {exit 1}' || {
  echo 'Release archive contains non-regular files' >&2; exit 1;
}
tar -xzf "$ASSET" --no-same-owner -C "$TX/bundle"
for part in saspam saspamd pam_saspam.so bot.py requirements.txt saspamd.service saspam-discord.service; do
  [[ -f "$TX/bundle/$part" && ! -L "$TX/bundle/$part" ]] || { echo "Missing $part" >&2; exit 1; }
done
# Refuse incompatible versions before stopping either service.
bash -c 'exec "$1" --check-config --config /etc/saspam/config.toml' bash "$TX/bundle/saspamd"
python3 -m py_compile "$TX/bundle/bot.py"
if (( DRY_RUN )); then
  echo "Release $TAG / $ARCH checksum and archive validation passed. No changes made."
  exit 0
fi
for part in /usr/local/bin/saspam /usr/local/bin/saspamd \
  /usr/local/lib/security/pam_saspam.so /opt/saspam/discord/bot.py \
  /opt/saspam/discord/requirements.txt /etc/systemd/system/saspamd.service \
  /etc/systemd/system/saspam-discord.service /etc/saspam/config.toml \
  /etc/saspam/discord.env; do
  [[ -f "$part" && ! -L "$part" ]] || { echo "Live file missing: $part; configure SASPAM first" >&2; exit 1; }
done
[[ -d /opt/saspam/venv ]] || { echo 'Live Python environment missing' >&2; exit 1; }
# Prepare and validate Python requirements before touching the live services.
install -d -m 0755 /opt/saspam
NEW_VENV="/opt/saspam/.venv.next-$id"
[[ ! -e "$NEW_VENV" ]] || exit 1
python3 -m venv "$NEW_VENV"
"$NEW_VENV/bin/python" -m pip install --disable-pip-version-check --no-input -r "$TX/bundle/requirements.txt"
chown -R root:saspam-bot "$NEW_VENV"
chmod -R u=rwX,g=rX,o= "$NEW_VENV"
runuser -u saspam-bot -- "$NEW_VENV/bin/python" -c 'import discord; print(discord.__version__)'
# All staging work finished. Take a global lock and make an immutable recovery snapshot.
exec 9>/run/lock/saspam-upgrade.lock
flock -w 30 9
cp -a /usr/local/bin/saspam "$BACKUP/saspam"
cp -a /usr/local/bin/saspamd "$BACKUP/saspamd"
cp -a /usr/local/lib/security/pam_saspam.so "$BACKUP/pam_saspam.so"
cp -a /opt/saspam/discord/bot.py "$BACKUP/bot.py"
cp -a /opt/saspam/discord/requirements.txt "$BACKUP/requirements.txt"
cp -a /etc/systemd/system/saspamd.service "$BACKUP/saspamd.service"
cp -a /etc/systemd/system/saspam-discord.service "$BACKUP/saspam-discord.service"
cp -a /etc/saspam/config.toml "$BACKUP/config.toml"
cp -a /etc/saspam/discord.env "$BACKUP/discord.env"
cp -a /opt/saspam/venv "$BACKUP/venv"
if [[ -f /var/lib/saspam/installed-version ]]; then cp -a /var/lib/saspam/installed-version "$BACKUP/installed-version"; fi
if systemctl is-active --quiet saspamd.service; then echo 1 > "$TX/was-active-daemon"; else echo 0 > "$TX/was-active-daemon"; fi
if systemctl is-active --quiet saspam-discord.service; then echo 1 > "$TX/was-active-bot"; else echo 0 > "$TX/was-active-bot"; fi
printf 'pending\n' > "$TX/state"
ROLLBACK_NEEDED=1
# Survives parent SIGKILL, broken terminal and lost SSH sessions.
systemd-run --unit="saspam-upgrade-$id" --on-active=240s \
  --timer-property=AccuracySec=1s "$HELPER" "$id" >/dev/null
# PAM hook remains in place: this window fails closed instead of bypassing approval.
systemctl stop saspam-discord.service saspamd.service || :
install -m 0755 "$TX/bundle/saspam" /usr/local/bin/saspam.next
mv -f /usr/local/bin/saspam.next /usr/local/bin/saspam
install -m 0755 "$TX/bundle/saspamd" /usr/local/bin/saspamd.next
mv -f /usr/local/bin/saspamd.next /usr/local/bin/saspamd
install -m 0644 "$TX/bundle/pam_saspam.so" /usr/local/lib/security/pam_saspam.so.next
mv -f /usr/local/lib/security/pam_saspam.so.next /usr/local/lib/security/pam_saspam.so
install -m 0644 "$TX/bundle/bot.py" /opt/saspam/discord/bot.py
install -m 0644 "$TX/bundle/requirements.txt" /opt/saspam/discord/requirements.txt
install -m 0644 "$TX/bundle/saspamd.service" /etc/systemd/system/saspamd.service
install -m 0644 "$TX/bundle/saspam-discord.service" /etc/systemd/system/saspam-discord.service
mv /opt/saspam/venv "$TX/old-live-venv"
mv "$NEW_VENV" /opt/saspam/venv
systemctl daemon-reload
/usr/local/bin/saspamd --check-config --config /etc/saspam/config.toml
if [[ $(cat "$TX/was-active-daemon") == 1 ]]; then systemctl restart saspamd.service; fi
if [[ $(cat "$TX/was-active-bot") == 1 ]]; then systemctl restart saspam-discord.service; fi
sleep 2
if [[ $(cat "$TX/was-active-daemon") == 1 ]]; then systemctl is-active --quiet saspamd.service; fi
if [[ $(cat "$TX/was-active-bot") == 1 ]]; then systemctl is-active --quiet saspam-discord.service; fi
if (( SKIP_TEST == 0 )); then
  echo 'The new version needs a Discord approval test; this is not an SSH bypass.'
  timeout -k 5 200 /usr/local/bin/saspam test || {
    echo 'New version was not approved. Restoring previous version.' >&2; exit 1;
  }
fi
printf '%s\n' "$TAG" > /var/lib/saspam/installed-version
printf 'committed\n' > "$TX/state"
printf '%s\n' "$id" > /var/lib/saspam/last-committed-tx
ROLLBACK_NEEDED=0
systemctl stop "saspam-upgrade-$id.timer" 2>/dev/null || :
if [[ -f "$TX/bundle/update.sh" ]]; then install -m 0755 "$TX/bundle/update.sh" /usr/local/lib/saspam/update.sh; fi
if [[ -f "$TX/bundle/rollback-upgrade.sh" ]]; then install -m 0755 "$TX/bundle/rollback-upgrade.sh" "$HELPER"; fi
echo "SASPAM upgrade committed: $CURRENT -> $TAG (PAM hook remained installed)"
echo "To revert manually: sudo saspam update --rollback"
