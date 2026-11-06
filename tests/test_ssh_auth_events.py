"""No network, Discord, PAM or root privileges needed for these tests."""
from __future__ import annotations

import contextlib
from io import StringIO
import importlib.util
import json
from pathlib import Path
import unittest
from unittest.mock import patch

SOURCE = Path(__file__).resolve().parents[1] / "tools" / "ssh_auth_events.py"
spec = importlib.util.spec_from_file_location("ssh_auth_events", SOURCE)
assert spec and spec.loader
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)


class OpenSSHLogParsing(unittest.TestCase):
    def test_known_methods_and_secrets_redacted(self):
        cases = [
            ("Accepted password for root from 100.92.24.113 port 45678 ssh2", "password"),
            ("Nov  6 18:02:03 vps sshd[123]: Accepted publickey for ubuntu from 192.0.2.2 port 12345 ssh2: ECDSA-SK SHA256:secret", "fido_ecdsa_sk"),
            ("Accepted publickey for admin from 2001:db8::42 port 45678 ssh2: ED25519-SK SHA256:secret", "fido_ed25519_sk"),
            ("Accepted publickey for admin from 192.0.2.3 port 40000 ssh2: ED25519 SHA256:secret", "public_key"),
            ("Accepted publickey for admin from 192.0.2.4 port 40000 ssh2", "public_key"),
            ("Accepted keyboard-interactive/pam for tester from 192.0.2.5 port 21212 ssh2", "keyboard_interactive"),
            ("Accepted hostbased for svc from 192.0.2.6 port 33333 ssh2", "hostbased"),
        ]
        for raw, expected in cases:
            with self.subTest(raw=raw):
                event = mod.parse_line(raw)
                self.assertIsNotNone(event)
                self.assertEqual(event["method"], expected)
                self.assertNotIn("SHA256", str(event))
                self.assertNotIn("secret", str(event))
        self.assertEqual(mod.parse_line(cases[0][0])["source_ip"], "100.92.24.113")

    def test_journal_json_identifier_and_pid(self):
        row = {
            "SYSLOG_IDENTIFIER": "sshd", "_EXE": "/usr/sbin/sshd", "_PID": "12345",
            "MESSAGE": "Accepted publickey for ubuntu from 192.0.2.8 port 42010 ssh2: ECDSA-SK SHA256:fingerprint",
        }
        event = mod.parse_line(json.dumps(row))
        self.assertEqual(event["method"], "fido_ecdsa_sk")
        self.assertEqual(event["journal_pid"], 12345)
        row["SYSLOG_IDENTIFIER"] = "attacker"
        self.assertIsNone(mod.parse_line(json.dumps(row)))
        row["SYSLOG_IDENTIFIER"] = "sshd"
        row["_EXE"] = "/tmp/fake-sshd"
        self.assertIsNone(mod.parse_line(json.dumps(row)))

    def test_ignore_failed_attempts_and_untrusted_lines(self):
        for raw in [
            "Failed password for ubuntu from 192.0.2.9 port 1234 ssh2",
            "Postponed publickey for ubuntu from 192.0.2.9 port 1234 ssh2",
            "prefix Accepted password for ubuntu from 192.0.2.9 port 1234 ssh2",
            "Accepted password for ubuntu from attacker.example port 1234 ssh2",
            "Accepted password for ubuntu from 192.0.2.9 port 0 ssh2",
            "Accepted publickey for ubuntu from 192.0.2.9 port 1234 ssh2: ECDSA-SK SHA256:secret" + "x" * 10000,
            "{invalid json",
            "Accepted publickey for ubuntu from 192.0.2.9 port 1234 ssh2: SHA256:secret",
        ]:
            if "SHA256:secret" in raw and len(raw) < 8192:
                self.assertEqual(mod.parse_line(raw)["method"], "public_key")
            else:
                self.assertIsNone(mod.parse_line(raw))

    def test_discord_mentions_are_disabled(self):
        event = {"user": "@everyone" + chr(96), "source_ip": "192.0.2.9", "source_port": 65000, "method": "fido_ecdsa_sk"}
        msg = json.loads(mod.discord_message(event, "host@everyone"))
        self.assertEqual(msg["allowed_mentions"], {"parse": []})
        self.assertEqual(msg["embeds"][0]["fields"][3]["value"], "Public key (FIDO / ECDSA-SK)")
        self.assertNotIn("@everyone", str(msg))

    def test_deduplicate_and_do_not_send_without_explicit_opt_in(self):
        lines = iter(["Accepted password for root from 192.0.2.11 port 12345 ssh2\n"] * 2)
        stdout = StringIO()
        with patch.object(mod, "send_discord") as notify, contextlib.redirect_stdout(stdout):
            self.assertEqual(mod.run(lines), 0)
        self.assertEqual(len(stdout.getvalue().splitlines()), 1)
        notify.assert_not_called()


if __name__ == "__main__":
    unittest.main()
