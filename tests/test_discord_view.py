"""Validate Discord role-authorization logic without network calls or secrets."""
import unittest
from unittest.mock import patch
import json

from integrations.discord.bot import RequestView, command, embed, notification_mention, has_approval_role, parse_role_ids

GUILD = 111111111111111111
CHANNEL = 222222222222222222
ROLES = frozenset({333333333333333333, 444444444444444444})


class DiscordViewTests(unittest.TestCase):
    def test_persistent_request_buttons(self):
        view = RequestView("00000000-0000-0000-0000-000000000001", GUILD, CHANNEL, ROLES)
        self.assertTrue(view.is_persistent())
        self.assertEqual({item.label for item in view.children}, {"承認", "拒否"})

    def test_english_buttons_and_embed(self):
        view = RequestView("00000000-0000-0000-0000-000000000003", GUILD, CHANNEL, ROLES, language="en")
        self.assertEqual({item.label for item in view.children}, {"Approve", "Deny"})
        req = {
            "request_id": "00000000-0000-0000-0000-000000000003",
            "user": "ubuntu", "remote_host": "192.0.2.1", "server_name": "example-host",
            "created_unix": 1793891000, "expires_unix": 1793891060, "status": "pending",
        }
        self.assertIn("SSH Login Approval", embed(req, "en").title)
        self.assertIn("SSHログイン承認", embed(req, "ja").title)

    def test_completed_view_is_disabled(self):
        view = RequestView("00000000-0000-0000-0000-000000000002", GUILD, CHANNEL, ROLES, done=True)
        self.assertTrue(all(item.disabled for item in view.children))

    def test_mentions_all_approval_roles(self):
        text, allowed = notification_mention("roles", ROLES)
        for role_id in ROLES:
            self.assertIn(f"<@&{role_id}>", text)
        self.assertFalse(allowed.everyone)
        self.assertFalse(allowed.users)
        self.assertEqual({role.id for role in allowed.roles}, ROLES)

    def test_everyone_is_explicit_and_distinct_from_role_authorization(self):
        text, allowed = notification_mention("everyone", ROLES)
        self.assertEqual(text, "@everyone")
        self.assertTrue(allowed.everyone)
        self.assertFalse(allowed.roles)
        self.assertFalse(allowed.users)
        # An everyone ping does not authorize someone who lacks an approval role.
        self.assertFalse(has_approval_role(GUILD, CHANNEL, set(), True, GUILD, CHANNEL, ROLES))

    def test_no_ping_default_and_invalid_mode_rejection(self):
        text, allowed = notification_mention("none", ROLES)
        self.assertIsNone(text)
        self.assertFalse(allowed.everyone)
        self.assertFalse(allowed.roles)
        with self.assertRaises(ValueError):
            notification_mention("bad-mode", ROLES)

    def test_authentication_method_field_hidden_by_default(self):
        for status in ("pending", "approved", "denied", "expired"):
            request = {
                "request_id": "00000000-0000-0000-0000-000000000001",
                "user": "ubuntu", "remote_host": "192.0.2.1",
                "server_name": "vm", "status": status,
                "auth_methods": ["fido_ecdsa_sk", "password"],
            }
            for lang in ("ja", "en"):
                fields = embed(request, lang).fields
                self.assertFalse(any("認証方式" in f.name or "authentication method" in f.name
                                     for f in fields))
                self.assertFalse(any("FIDO" in f.value or "Unknown" in f.value for f in fields))
        request = {"status": "pending", "user": "ubuntu", "remote_host": "192.0.2.1",
                   "server_name": "vm", "request_id": "00000000-0000-0000-0000-000000000002"}
        self.assertFalse(any("認証方式" in f.name for f in embed(request).fields))
        self.assertTrue(any("不明（取得不可）" in f.value for f in embed(request, auth_mode="experimental").fields))

    def test_primary_ssh_auth_methods_are_localized(self):
        base = {
            "request_id": "00000000-0000-0000-0000-000000000001",
            "user": "ubuntu", "remote_host": "192.0.2.3", "server_name": "host",
            "status": "pending", "created_unix": 1793891000, "expires_unix": 1793891060,
        }
        for method, ja, en in [
            ("public_key", "公開鍵認証", "Public key"),
            ("password", "パスワード認証", "Password"),
            ("fido", "FIDO対応鍵", "FIDO-backed"),
            ("fido_ecdsa_sk", "FIDO / ECDSA-SK", "FIDO / ECDSA-SK"),
            ("fido_ed25519_sk", "FIDO / Ed25519-SK", "FIDO / Ed25519-SK"),
            ("keyboard_interactive", "キーボード対話認証", "Keyboard-interactive"),
        ]:
            with self.subTest(method=method):
                base["auth_methods"] = [method]
                self.assertTrue(any(ja in field.value for field in embed(base, "ja", auth_mode="experimental").fields))
                self.assertTrue(any(en in field.value for field in embed(base, "en", auth_mode="experimental").fields))
        base["auth_methods"] = ["public_key", "password"]
        self.assertTrue(any("公開鍵認証 + パスワード認証" in field.value for field in embed(base, "ja", auth_mode="experimental").fields))
        base["auth_methods"] = ["public_key", "public_key"]
        self.assertTrue(any("公開鍵認証 + 公開鍵認証" in field.value for field in embed(base, "ja", auth_mode="experimental").fields))
        base.pop("auth_methods")
        self.assertTrue(any("不明（取得不可）" in field.value for field in embed(base, "ja", auth_mode="experimental").fields))
        base["auth_methods"] = ["unknown"]
        self.assertTrue(any("Unknown" in field.value for field in embed(base, "en", auth_mode="experimental").fields))

    def test_japanese_embed_pending_and_completed_states(self):
        req = {
            "request_id": "00000000-0000-0000-0000-000000000001",
            "user": "ubuntu",
            "remote_host": "192.0.2.1",
            "server_name": "example-host",
            "created_unix": 1793891000,
            "expires_unix": 1793891060,
            "status": "pending",
            "actor_id": None,
        }
        pending = embed(req)
        self.assertIn("SSHログイン承認リクエスト", pending.title)
        self.assertIn("\n\n", pending.description)
        self.assertTrue(any("接続先" in field.name for field in pending.fields))
        self.assertTrue(any("承認期限" in field.name for field in pending.fields))
        for status, keyword in (("approved", "承認"), ("denied", "拒否"), ("expired", "タイムアウト")):
            with self.subTest(status=status):
                req["status"] = status
                req["actor_id"] = "555555555555555555" if status != "expired" else None
                finished = embed(req)
                self.assertIn(keyword, finished.title)
                self.assertFalse(any("承認期限" in field.name for field in finished.fields))
                self.assertIn("処理済み", finished.footer.text)

    def test_bot_sends_protocol_v2_and_role_context(self):
        class FakeSocket:
            def __init__(self):
                self.sent = b""
            def __enter__(self): return self
            def __exit__(self, *_): return False
            def settimeout(self, value): pass
            def connect(self, address): pass
            def sendall(self, data): self.sent = data
            def recv(self, length): return b'{"ok":true}\n'
        sock = FakeSocket()
        with patch("integrations.discord.bot.socket.socket", return_value=sock):
            self.assertTrue(command({"action": "decide", "request_id": "request-1",
                                     "decision": "approve", "actor_id": "555555555555555555",
                                     "guild_id": str(GUILD), "channel_id": str(CHANNEL),
                                     "role_ids": [str(next(iter(ROLES)))]})["ok"])
        data = json.loads(sock.sent.decode())
        self.assertEqual(data["version"], 2)
        self.assertEqual(data["guild_id"], str(GUILD))
        self.assertEqual(data["channel_id"], str(CHANNEL))
        self.assertIn(int(data["role_ids"][0]), ROLES)

    def test_comma_separated_role_ids(self):
        self.assertEqual(parse_role_ids("333333333333333333, 444444444444444444"), ROLES)

    def test_invalid_and_duplicate_roles(self):
        for text in ("", "333333333333333333,", "333333333333333333,hello",
                     "333333333333333333,333333333333333333"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                parse_role_ids(text)

    def test_either_approval_role_is_sufficient(self):
        self.assertTrue(has_approval_role(GUILD, CHANNEL, {333333333333333333}, True, GUILD, CHANNEL, ROLES))
        self.assertTrue(has_approval_role(GUILD, CHANNEL, {444444444444444444}, True, GUILD, CHANNEL, ROLES))

    def test_unauthorized_context_is_rejected(self):
        cases = [
            (GUILD, CHANNEL, set(), True),
            (GUILD, CHANNEL, {555555555555555555}, True),
            (GUILD, CHANNEL, {333333333333333333}, False),
            (999999999999999999, CHANNEL, {333333333333333333}, True),
            (GUILD, 999999999999999999, {333333333333333333}, True),
            (None, CHANNEL, {333333333333333333}, True),
        ]
        for guild, channel, roles, visible in cases:
            with self.subTest(guild=guild, channel=channel, roles=roles, visible=visible):
                self.assertFalse(has_approval_role(guild, channel, roles, visible, GUILD, CHANNEL, ROLES))


if __name__ == "__main__":
    unittest.main()
