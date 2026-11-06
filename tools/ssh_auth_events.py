#!/usr/bin/env python3
"""Read-only OpenSSH Accepted-event monitor (not a PAM authentication source).

Based on the operator's 2025 auth.log Discord notifier. The default
source is the system journal; --file is for offline log inspection.
Nothing in this module can approve SASPAM requests.
"""
from __future__ import annotations

import argparse
from collections import deque
import ipaddress
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from typing import Iterator

# Matches a genuine Accepted record, not a failed or offered key. An
# optional syslog prefix is present in /var/log/auth.log and absent in
# journalctl --output=cat and the journal JSON MESSAGE field.
ACCEPTED = re.compile(
    r"^(?:[^\r\n]{0,160}\bsshd(?:-session)?\[\d+\]:\s+)?"
    r"Accepted\s+(?P<method>password|publickey|keyboard-interactive(?:/pam)?|hostbased)"
    r"\s+for\s+(?P<user>[^\s]{1,128})\s+from\s+(?P<ip>[^\s]{1,128})"
    r"\s+port\s+(?P<port>\d{1,5})\s+ssh2(?:\s*:\s*(?P<detail>[^\r\n]{1,256}))?\s*$"
)
FIDO_KEY_TYPES = {
    "ECDSA-SK": "fido_ecdsa_sk",
    "sk-ecdsa-sha2-nistp256@openssh.com": "fido_ecdsa_sk",
    "ED25519-SK": "fido_ed25519_sk",
    "sk-ssh-ed25519@openssh.com": "fido_ed25519_sk",
}
SUPPORTED_IDENTIFIERS = frozenset(("sshd", "sshd-session"))
# The journal _EXE metadata is supplied by systemd (unlike SYSLOG_IDENTIFIER,
# which another process could forge). Accept only known OpenSSH executable paths.
TRUSTED_EXECUTABLES = frozenset((
    "/usr/sbin/sshd", "/usr/local/sbin/sshd",
    "/usr/lib/openssh/sshd-auth", "/usr/lib/openssh/sshd-session",
))


def classify(method: str, detail: str | None) -> str:
    """Classify only the authenticated method/type shown in Accepted logs."""
    if method != "publickey":
        return "keyboard_interactive" if method.startswith("keyboard-interactive") else method
    key_type = (detail or "").split(maxsplit=1)[0] if detail else ""
    return FIDO_KEY_TYPES.get(key_type, "public_key")


def parse_line(raw: str) -> dict[str, object] | None:
    if len(raw) > 8192 or any(ch == "\x00" for ch in raw):
        return None
    line = raw.rstrip("\n")
    pid = None
    if line.startswith("{"):
        try:
            source = json.loads(line)
        except (ValueError, TypeError):
            return None
        if (not isinstance(source, dict)
                or source.get("SYSLOG_IDENTIFIER") not in SUPPORTED_IDENTIFIERS
                or source.get("_EXE") not in TRUSTED_EXECUTABLES):
            return None
        line = source.get("MESSAGE", "")
        if not isinstance(line, str):
            return None
        if (str(source.get("_PID", "")).isdigit()
                and len(str(source.get("_PID"))) <= 10):
            pid = int(source["_PID"])
    match = ACCEPTED.fullmatch(line)
    if not match:
        return None
    user, remote_ip, method, detail = (
        match.group("user"), match.group("ip"), match.group("method"), match.group("detail")
    )
    try:
        # OpenSSH emits IP literals in the from field for Accepted logs.
        ip = str(ipaddress.ip_address(remote_ip))
        port = int(match.group("port"))
    except ValueError:
        return None
    if port == 0 or any(ord(c) < 32 or ord(c) == 127 for c in user):
        return None
    result: dict[str, object] = {
        "event": "openssh_accepted_log",
        "user": user,
        "source_ip": ip,
        "source_port": port,
        "ssh_method": method.split("/", 1)[0],
        "method": classify(method, detail),
    }
    # No fingerprint, key bytes, credential, or raw log line is forwarded.
    if pid is not None:
        result["journal_pid"] = pid
    return result


def journal_lines() -> Iterator[str]:
    # Only new events. Do not replay old logins after restart.
    command = ["journalctl", "--no-pager", "--follow", "-n", "0", "--output=json",
               "--identifier=sshd", "--identifier=sshd-session"]
    try:
        with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                              encoding="utf-8", errors="replace") as proc:
            assert proc.stdout is not None
            yield from proc.stdout
            if proc.wait() != 0:
                raise RuntimeError("journalctl stopped unexpectedly")
    except FileNotFoundError as exc:
        raise RuntimeError("journalctl is not installed") from exc


def clean(value: object) -> str:
    """Neutralize Discord formatting even with allowed_mentions disabled."""
    return str(value).replace(chr(96), "′").replace("@", "＠").replace("\n", " ").replace("\r", " ")[:128]


def discord_message(event: dict[str, object], server_name: str) -> bytes:
    labels = {
        "password": "Password",
        "public_key": "Public key",
        "fido_ecdsa_sk": "Public key (FIDO / ECDSA-SK)",
        "fido_ed25519_sk": "Public key (FIDO / Ed25519-SK)",
        "keyboard_interactive": "Keyboard-interactive",
        "hostbased": "Hostbased",
    }
    label = labels.get(str(event["method"]), "Unknown")
    payload = {
        "username": "SASPAM SSH audit",
        "allowed_mentions": {"parse": []},
        "embeds": [{
            "title": "SSH認証ログを確認しました / OpenSSH Accepted event",
            "description": "監査通知 / Audit only — not a Discord approval decision",
            "color": 3066993,
            "fields": [
                {"name": "Server", "value": clean(server_name), "inline": True},
                {"name": "User", "value": clean(event["user"]), "inline": True},
                {"name": "Source", "value": f"{clean(event['source_ip'])}:{event['source_port']}", "inline": False},
                {"name": "Method", "value": label, "inline": False},
            ],
        }],
    }
    return json.dumps(payload, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def send_discord(event: dict[str, object], url: str, server_name: str) -> bool:
    body = discord_message(event, server_name)
    req = urllib.request.Request(
        url, data=body,
        headers={"Content-Type": "application/json", "User-Agent": "DisnanaSASPAM-AuthAudit/0.1"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=4) as resp:
            return resp.status in (200, 204)
    except (OSError, urllib.error.URLError, ValueError):
        # Never print the URL containing the secret webhook token.
        print("SSH audit Discord delivery failed", file=sys.stderr)
        return False


def run(lines: Iterator[str], *, webhook: str | None = None, server: str = "unknown") -> int:
    # Observation-only deduplication and rate cap. Never influences SSH access.
    recent: dict[tuple[str, str, int, str], float] = {}
    delivery: deque[float] = deque()
    for raw in lines:
        evt = parse_line(raw)
        if evt is None:
            continue
        now = time.monotonic()
        identity = (str(evt["user"]), str(evt["source_ip"]), int(evt["source_port"]), str(evt["method"]))
        if recent.get(identity, float("-inf")) > now - 90:
            continue
        if len(recent) >= 256:
            recent = {k: t for k, t in recent.items() if t > now - 90}
            if len(recent) >= 256:
                recent.pop(next(iter(recent)))
        recent[identity] = now
        print(json.dumps(evt, ensure_ascii=False, separators=(",", ":")), flush=True)
        if webhook:
            while delivery and delivery[0] <= now - 60:
                delivery.popleft()
            if len(delivery) < 30:
                delivery.append(now)
                send_discord(evt, webhook, server)
            else:
                print("SSH audit Discord rate cap reached", file=sys.stderr)
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Read-only OpenSSH auth audit; not a login approval source")
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--journal-follow", action="store_true", help="follow new sshd journal events")
    source.add_argument("--file", type=Path, help="inspect an existing text or journal JSON log file")
    parser.add_argument("--discord", action="store_true", help="also post separate audit cards to Discord")
    args = parser.parse_args(argv)
    webhook = None
    if args.discord:
        webhook = os.environ.get("SASPAM_AUDIT_DISCORD_WEBHOOK", "")
        if not re.fullmatch(r"https://(?:discord\.com|discordapp\.com)/api/webhooks/\d+/[A-Za-z0-9_-]+", webhook):
            parser.error("set a valid SASPAM_AUDIT_DISCORD_WEBHOOK environment variable")
    try:
        if args.file is not None:
            with args.file.open(encoding="utf-8", errors="replace") as input_file:
                return run(input_file, webhook=webhook, server=socket.gethostname())
        return run(journal_lines(), webhook=webhook, server=socket.gethostname())
    except (OSError, RuntimeError) as exc:
        print(f"SSH audit read failed: {type(exc).__name__}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
