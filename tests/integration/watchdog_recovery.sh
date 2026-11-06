#!/usr/bin/env bash
# Recovery watchdog integration test on a disposable GitHub Actions runner.
# Deliberately corrupts a temporary test installation; NEVER run on production.
set -Eeuo pipefail
[[ $(id -u) -eq 0 ]] || { echo 'run only as root in ephemeral CI' >&2; exit 1; }
[[ "${GITHUB_ACTIONS:-}" == true ]] || { echo 'this destructive test is CI-only' >&2; exit 1; }
if grep -q 'pam_saspam.so' /etc/pam.d/sshd; then
  echo 'Refusing to simulate crash with active SSH PAM gate' >&2; exit 1;
fi
source_id=$(cat /var/lib/saspam/last-committed-tx)
[[ "$source_id" =~ ^tx-[a-zA-Z0-9]{8,16}$ ]] || exit 1
source="/var/lib/saspam/transactions/$source_id"
[[ -f "$source/backup/saspam" && -d "$source/backup/venv" ]] || exit 1
TX=$(mktemp -d /var/lib/saspam/transactions/tx-XXXXXXXX)
id=$(basename "$TX")
cp -a "$source/backup" "$TX/backup"
cp -a "$source/was-active-daemon" "$TX/was-active-daemon"
cp -a "$source/was-active-bot" "$TX/was-active-bot"
printf 'pending\n' > "$TX/state"
# Register an independent systemd timer, then simulate a partially installed
# binary while no updater process is running.
systemd-run --unit="saspam-crash-test-$id" --on-active=5s \
  /usr/local/lib/saspam/rollback-upgrade.sh "$id" >/dev/null
install -m 0755 /bin/false /usr/local/bin/saspam.corrupt
mv -f /usr/local/bin/saspam.corrupt /usr/local/bin/saspam
if /usr/local/bin/saspam help >/dev/null 2>&1; then
  echo 'Expected the deliberately corrupted CLI to fail' >&2; exit 1;
fi
restored=0
for _ in $(seq 1 60); do
  if [[ $(cat "$TX/state") == rolled-back ]] && /usr/local/bin/saspam help >/dev/null 2>&1; then
    restored=1
    break
  fi
  sleep .5
done
[[ "$restored" == 1 ]] || {
  journalctl -u "saspam-crash-test-$id.service" --no-pager -n 100 || true
  echo 'systemd watchdog did not restore a working CLI' >&2; exit 1;
}
[[ -f /usr/local/lib/security/pam_saspam.so ]] || exit 1
if grep -q 'pam_saspam.so' /etc/pam.d/sshd; then
  echo 'Recovery changed sshd PAM configuration unexpectedly' >&2; exit 1;
fi
echo "Watchdog restored the previous version from a simulated interrupted transaction: $id"
