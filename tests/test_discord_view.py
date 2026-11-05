"""Basic interaction compatibility checks for discord.py, no network or token."""
import unittest
from integrations.discord.bot import RequestView


class DiscordViewTests(unittest.TestCase):
    def test_persistent_request_buttons(self):
        view = RequestView("00000000-0000-0000-0000-000000000001", frozenset({123456}))
        self.assertTrue(view.is_persistent())
        self.assertEqual(len(view.children), 2)
        self.assertEqual({item.label for item in view.children}, {"Approve", "Deny"})

    def test_completed_view_is_disabled(self):
        view = RequestView("00000000-0000-0000-0000-000000000002", frozenset({123456}), done=True)
        self.assertTrue(all(item.disabled for item in view.children))


if __name__ == "__main__":
    unittest.main()
