"""Validate Discord role-authorization logic without network calls or secrets."""
import unittest

from integrations.discord.bot import RequestView, has_approval_role, parse_role_ids

GUILD = 111111111111111111
CHANNEL = 222222222222222222
ROLES = frozenset({333333333333333333, 444444444444444444})


class DiscordViewTests(unittest.TestCase):
    def test_persistent_request_buttons(self):
        view = RequestView("00000000-0000-0000-0000-000000000001", GUILD, CHANNEL, ROLES)
        self.assertTrue(view.is_persistent())
        self.assertEqual({item.label for item in view.children}, {"Approve", "Deny"})

    def test_completed_view_is_disabled(self):
        view = RequestView("00000000-0000-0000-0000-000000000002", GUILD, CHANNEL, ROLES, done=True)
        self.assertTrue(all(item.disabled for item in view.children))

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
