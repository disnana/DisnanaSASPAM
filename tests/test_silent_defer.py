"""Discord component acknowledgement must be silent and edit only the source Embed."""
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

import integrations.discord.bot as bot

GUILD = 111111111111111111
CHANNEL = 222222222222222222
ROLE = 333333333333333333
ACTOR = 555555555555555555


class FakeMember:
    def __init__(self, roles):
        self.id = ACTOR
        self.roles = [SimpleNamespace(id=r) for r in roles]


def interaction(roles):
    response = SimpleNamespace(defer=AsyncMock(), send_message=AsyncMock())
    message = SimpleNamespace(author=SimpleNamespace(id=987654321098765432), edit=AsyncMock())
    return SimpleNamespace(
        user=FakeMember(roles),
        guild_id=GUILD, channel_id=CHANNEL,
        permissions=SimpleNamespace(view_channel=True),
        response=response, message=message,
        client=SimpleNamespace(user=SimpleNamespace(id=987654321098765432)),
        followup=SimpleNamespace(send=AsyncMock())
    )


class SilentDeferTests(unittest.IsolatedAsyncioTestCase):
    async def test_authorized_click_only_edits_original_message(self):
        data = {
            "status": "approved", "request_id": "00000000-0000-0000-0000-000000000001",
            "user": "ubuntu", "remote_host": "192.0.2.7", "server_name": "test",
            "created_unix": 1793891000, "expires_unix": 1793891060,
            "actor_id": str(ACTOR)
        }
        async def fake_request(payload):
            if payload["action"] == "decide":
                self.assertEqual(payload["role_ids"], [str(ROLE)])
                return {"ok": True}
            if payload["action"] == "get":
                return {"ok": True, "requests": [data]}
            raise AssertionError("unexpected IPC request")
        view = bot.RequestView(data["request_id"], GUILD, CHANNEL, frozenset({ROLE}))
        action = next(item for item in view.children if item.label == "承認")
        event = interaction({ROLE})
        with patch.object(bot.discord, "Member", FakeMember), patch.object(bot, "request", side_effect=fake_request):
            await action.callback(event)
        event.response.defer.assert_awaited_once_with(thinking=False)
        event.response.send_message.assert_not_awaited()
        event.followup.send.assert_not_awaited()
        event.message.edit.assert_awaited_once()
        self.assertIn("承認", event.message.edit.await_args.kwargs["embed"].title)
        self.assertTrue(all(item.disabled for item in view.children))

    async def test_unauthorized_role_still_silent_and_cannot_approve(self):
        view = bot.RequestView("00000000-0000-0000-0000-000000000002", GUILD, CHANNEL, frozenset({ROLE}))
        event = interaction({444444444444444444})
        with patch.object(bot.discord, "Member", FakeMember), patch.object(bot, "request", new_callable=AsyncMock) as req:
            await view.children[0].callback(event)
        event.response.defer.assert_awaited_once_with(thinking=False)
        event.response.send_message.assert_not_awaited()
        event.followup.send.assert_not_awaited()
        event.message.edit.assert_not_awaited()
        req.assert_not_awaited()


if __name__ == "__main__":
    unittest.main()
