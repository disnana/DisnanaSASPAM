#!/usr/bin/env bash
# Official GitHub Release installer. Does NOT enable PAM or alter SSH config.
set -Eeuo pipefail
umask 077
REPO="disnana/DisnanaSASPAM"
TAG=""
while (( $# > 0 )); do
  case "$1" in
    --version) TAG="${2:?expected tag after --version}"; shift 2 ;;
    --help|-h) echo 'Usage: curl -fsSL https://raw.githubusercontent.com/disnana/DisnanaSASPAM/main/scripts/install.sh | sudo bash [-s -- --version v0.1.0]'; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done
[[ $(id -u) -eq 0 ]] || { echo 'Run the installer as root (sudo)' >&2; exit 1; }
[[ -f /etc/os-release ]] || { echo 'Unsupported OS' >&2; exit 1; }
. /etc/os-release
[[ "${ID:-}" == "ubuntu" ]] || { echo "Only Ubuntu is supported (found ${ID:-unknown})" >&2; exit 1; }
command -v systemctl >/dev/null || { echo 'systemd is required' >&2; exit 1; }
for tool in curl sha256sum tar awk python3 install; do
  command -v "$tool" >/dev/null || { echo "Missing prerequisite: $tool" >&2; exit 1; }
done
case "$(uname -m)" in
  x86_64) ARCH=amd64 ;;
  aarch64) ARCH=arm64 ;;
  *) echo 'Only amd64 / arm64 are supported' >&2; exit 1 ;;
esac
if grep -q 'pam_saspam.so' /etc/pam.d/sshd 2>/dev/null; then
  echo 'SASPAM PAM gate is present. Refusing in-place upgrade; plan recovery first.' >&2
  exit 1
fi
if [[ -z "$TAG" ]]; then
  TAG=$(curl --proto '=https' --tlsv1.2 -fsSL --retry 2 --connect-timeout 10 \
    "https://api.github.com/repos/${REPO}/releases/latest" | \
    python3 -c 'import sys,json; print(json.load(sys.stdin)["tag_name"])') || {
      echo 'No stable GitHub Release yet. Alpha is source-only; see README.' >&2
      exit 1
    }
fi
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.-]+)?$ ]] || { echo 'Invalid release tag' >&2; exit 1; }
ASSET="saspam-${TAG}-linux-${ARCH}.tar.gz"
BASE="https://github.com/${REPO}/releases/download/${TAG}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
cd "$TMP"
echo "Downloading SASPAM bundle $TAG / $ARCH"
curl --proto '=https' --tlsv1.2 -fLsS --retry 2 --connect-timeout 10 -o SHA256SUMS "${BASE}/SHA256SUMS"
curl --proto '=https' --tlsv1.2 -fLsS --retry 2 --connect-timeout 10 -o "$ASSET" "${BASE}/${ASSET}"
EXPECTED=$(awk -v name="$ASSET" '$2==name {print $1}' SHA256SUMS)
[[ "$EXPECTED" =~ ^[0-9a-fA-F]{64}$ ]] || { echo 'Invalid SHA256SUMS or missing entry' >&2; exit 1; }
printf '%s  %s\n' "$EXPECTED" "$ASSET" | sha256sum -c -
# Reject absolute paths, traversal, symlinks, devices and unexpected members.
while IFS= read -r path; do
  case "$path" in
    saspam|saspamd|pam_saspam.so|bot.py|requirements.txt|saspamd.service|saspam-discord.service|uninstall.sh) ;;
    *) echo "Unexpected bundle entry: $path" >&2; exit 1 ;;
  esac
done < <(tar -tzf "$ASSET")
if tar -tvzf "$ASSET" | awk '$1 !~ /^-/ {exit 1}'; then :; else echo 'Unsafe archive entry type' >&2; exit 1; fi
tar -xzf "$ASSET" --no-same-owner
for file in saspam saspamd pam_saspam.so bot.py requirements.txt saspamd.service saspam-discord.service uninstall.sh; do
  [[ -f "$file" && ! -L "$file" ]] || { echo "Missing bundle file: $file" >&2; exit 1; }
done
export DEBIAN_FRONTEND=noninteractive
apt-get install -y python3-venv python3-pip libpam0g >/dev/null
getent group saspam-bot >/dev/null || groupadd --system saspam-bot
id saspam-bot >/dev/null 2>&1 || useradd --system --gid saspam-bot --home-dir /var/lib/saspam-discord --create-home --shell /usr/sbin/nologin saspam-bot
install -d -m 0700 -o root -g root /etc/saspam
install -d -m 0755 -o root -g root /usr/local/lib/security /opt/saspam/discord /usr/local/lib/saspam
install -d -m 0700 -o saspam-bot -g saspam-bot /var/lib/saspam-discord
install -m 0755 saspam /usr/local/bin/saspam.new
mv -f /usr/local/bin/saspam.new /usr/local/bin/saspam
install -m 0755 saspamd /usr/local/bin/saspamd.new
mv -f /usr/local/bin/saspamd.new /usr/local/bin/saspamd
install -m 0644 pam_saspam.so /usr/local/lib/security/pam_saspam.so.new
mv -f /usr/local/lib/security/pam_saspam.so.new /usr/local/lib/security/pam_saspam.so
install -m 0644 bot.py /opt/saspam/discord/bot.py
install -m 0644 requirements.txt /opt/saspam/discord/requirements.txt
install -m 0755 uninstall.sh /usr/local/lib/saspam/uninstall.sh
python3 -m venv /opt/saspam/venv
/opt/saspam/venv/bin/python -m pip install --disable-pip-version-check --no-input -r /opt/saspam/discord/requirements.txt
install -m 0644 saspamd.service /etc/systemd/system/saspamd.service
install -m 0644 saspam-discord.service /etc/systemd/system/saspam-discord.service
systemctl daemon-reload
printf '\nSASPAM installed, but SSH is NOT protected yet.\n\nNext:\n  sudo saspam setup\n  sudo saspam doctor\n  sudo saspam test\n  sudo saspam enable --ack-recovery --ack-tested\n'
