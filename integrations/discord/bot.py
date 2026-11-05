"""Discord approval adapter for DisnanaSASPAM.

Run as the unprivileged saspam-bot user. Only the root daemon decides
authentication; notifications never carry a reusable authorization secret.
"""
from __future__ import annotations

import asyncio
import json
import logging
import os
import socket
from pathlib import Path
from typing import Any

import discord

SOCKET_PATH = "/run/saspam/bot.sock"
STATE_FILE = Path("/var/lib/saspam-discord/messages.json")
LOG = logging.getLogger("saspam.discord")


def command(payload: dict[str, Any]) -> dict[str, Any]:
    """One JSON request / response over a root-owned local Unix socket."""
    wire = json.dumps({"version": 1, **payload}, separators=(",", ":")).encode() + b"\n"
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as conn:
        conn.settimeout(5)
        conn.connect(SOCKET_PATH)
        conn.sendall(wire)
        parts = bytearray()
        while b"\n" not in parts:
            chunk = conn.recv(4096)
            if not chunk:
                raise RuntimeError("daemon disconnected")
            parts.extend(chunk)
            if len(parts) > 65536:
                raise RuntimeError("daemon response too large")
    return json.loads(parts.split(b"\n", 1)[0])


async def request(payload: dict[str, Any]) -> dict[str, Any]:
    return await asyncio.to_thread(command, payload)


def load_state() -> dict[str, int]:
    try:
        return {str(k): int(v) for k, v in json.loads(STATE_FILE.read_text()).items()}
    except (FileNotFoundError, ValueError, OSError):
        return {}


def save_state(data: dict[str, int]) -> None:
    STATE_FILE.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    path = STATE_FILE.with_name(f".messages.{os.getpid()}.tmp")
    if path.exists():
        path.unlink()
    with open(path, "x", encoding="utf-8") as f:
        os.chmod(path, 0o600)
        json.dump(data, f)
        f.flush()
        os.fsync(f.fileno())
    os.replace(path, STATE_FILE)


def embed(req: dict[str, Any]) -> discord.Embed:
    status = req["status"]
    colors = {"pending": 0xF6AD45, "approved": 0x31A354, "denied": 0xD9534F, "expired": 0x888888}
    e = discord.Embed(title="DisnanaSASPAM · SSH login", color=colors.get(status, 0x888888))
    e.add_field(name="Status", value=status.upper(), inline=False)
    for key, label in (("server_name", "Server"), ("user", "User"), ("remote_host", "Remote"),
                       ("request_id", "Request ID")):
        e.add_field(name=label, value=discord.utils.escape_markdown(str(req.get(key, "?"))[:150]), inline=key != "request_id")
    actor = req.get("actor_id")
    if actor:
        e.add_field(name="Decided by", value=f"<@{actor}>", inline=True)
    e.add_field(name="Requested", value=f"<t:{int(req['created_unix'])}:F> · <t:{int(req['created_unix'])}:R>", inline=False)
    e.add_field(name="Expires", value=f"<t:{int(req['expires_unix'])}:F>", inline=False)
    e.set_footer(text="Approval follows successful SSH authentication; no action = deny")
    return e


def has_approval_role(
    guild_id: int | None,
    channel_id: int | None,
    member_role_ids: set[int],
    visible: bool,
    expected_guild_id: int,
    expected_channel_id: int,
    allowed_role_ids: frozenset[int],
) -> bool:
    """Require the right server, right channel, view permission and ANY role."""
    return (
        guild_id == expected_guild_id
        and channel_id == expected_channel_id
        and visible
        and bool(member_role_ids.intersection(allowed_role_ids))
    )


def parse_role_ids(raw: str) -> frozenset[int]:
    values = [item.strip() for item in raw.split(",")]
    if not 1 <= len(values) <= 32 or any(not item.isascii() or not item.isdecimal() or not 15 <= len(item) <= 22 for item in values):
        raise ValueError("DISCORD_APPROVAL_ROLE_IDS requires 1-32 comma-separated numeric role IDs")
    result = frozenset(map(int, values))
    if len(result) != len(values):
        raise ValueError("duplicate approval role ID")
    return result


class RequestView(discord.ui.View):
    def __init__(self, request_id: str, guild_id: int, channel_id: int, allowed_roles: frozenset[int], done: bool = False):
        super().__init__(timeout=None)
        self.request_id = request_id
        self.guild_id = guild_id
        self.channel_id = channel_id
        self.allowed_roles = allowed_roles
        for decision, label, style in (("approve", "Approve", discord.ButtonStyle.success),
                                       ("deny", "Deny", discord.ButtonStyle.danger)):
            button = discord.ui.Button(label=label, style=style,
                                       custom_id=f"saspam:v1:{decision}:{request_id}", disabled=done)
            button.callback = self._callback(decision)
            self.add_item(button)

    def _callback(self, decision: str):
        async def action(interaction: discord.Interaction) -> None:
            # Component interactions contain a guild Member with the roles
            # supplied by Discord. No privileged Guild Members intent required.
            if not isinstance(interaction.user, discord.Member):
                await interaction.response.send_message("Guild membership is required.", ephemeral=True)
                return
            role_ids = {role.id for role in interaction.user.roles}
            if not has_approval_role(
                interaction.guild_id, interaction.channel_id, role_ids,
                interaction.permissions.view_channel, self.guild_id, self.channel_id, self.allowed_roles
            ) or not interaction.message or not interaction.client.user or interaction.message.author.id != interaction.client.user.id:
                await interaction.response.send_message("You do not have permission to approve this SSH login.", ephemeral=True)
                return
            await interaction.response.defer(ephemeral=True)
            try:
                result = await request({"action": "decide", "request_id": self.request_id,
                                        "actor_id": str(interaction.user.id), "decision": decision,
                                        "guild_id": str(interaction.guild_id),
                                        "channel_id": str(interaction.channel_id),
                                        "role_ids": [str(role_id) for role_id in sorted(role_ids)]})
            except (OSError, ValueError, RuntimeError) as exc:
                LOG.warning("IPC decision failed: %s", type(exc).__name__)
                await interaction.followup.send("Approval service unavailable; access stays denied.", ephemeral=True)
                return
            if not result.get("ok"):
                await interaction.followup.send("Request already finished, expired, or not permitted.", ephemeral=True)
                return
            # discord.py View has no disable_all_items() (that API belongs to Pycord).
            for item in self.children:
                item.disabled = True
            req_reply = await request({"action": "get", "request_id": self.request_id})
            if req_reply.get("ok") and req_reply.get("requests") and interaction.message:
                await interaction.message.edit(embed=embed(req_reply["requests"][0]), view=self)
            await interaction.followup.send("Decision recorded.", ephemeral=True)
        return action


class SaspamBot(discord.Client):
    def __init__(self, guild_id: int, channel_id: int, allowed_roles: frozenset[int]):
        intents = discord.Intents.default()
        intents.message_content = False  # No privileged intent or message reading required
        super().__init__(intents=intents, allowed_mentions=discord.AllowedMentions.none())
        self.guild_id = guild_id
        self.channel_id = channel_id
        self.allowed_roles = allowed_roles
        self.messages: dict[str, int] = load_state()
        self.poller: asyncio.Task | None = None

    async def setup_hook(self) -> None:
        for req_id, message_id in self.messages.items():
            self.add_view(RequestView(req_id, self.guild_id, self.channel_id, self.allowed_roles), message_id=message_id)
        self.poller = asyncio.create_task(self.poll(), name="saspam-poller")

    async def close(self) -> None:
        if self.poller:
            self.poller.cancel()
        await super().close()

    async def channel(self) -> discord.abc.Messageable:
        ch = self.get_channel(self.channel_id)
        if ch is None:
            ch = await self.fetch_channel(self.channel_id)
        if not isinstance(ch, (discord.TextChannel, discord.Thread)) or ch.guild.id != self.guild_id:
            raise RuntimeError("Discord channel does not belong to the configured guild")
        return ch

    async def poll(self) -> None:
        await self.wait_until_ready()
        while not self.is_closed():
            try:
                ch = await self.channel()
                reply = await request({"action": "list"})
                if not reply.get("ok"):
                    raise RuntimeError("daemon list unavailable")
                pending = {r["request_id"]: r for r in reply.get("requests", [])}
                for req_id, req in pending.items():
                    if req_id not in self.messages:
                        message = await ch.send(embed=embed(req), view=RequestView(req_id, self.guild_id, self.channel_id, self.allowed_roles))
                        self.messages[req_id] = message.id
                        save_state(self.messages)
                for req_id, message_id in list(self.messages.items()):
                    if req_id in pending:
                        continue
                    result = await request({"action": "get", "request_id": req_id})
                    if result.get("ok") and result.get("requests"):
                        row = result["requests"][0]
                        if row["status"] == "pending":
                            continue
                        message = await ch.fetch_message(message_id)
                        await message.edit(embed=embed(row), view=RequestView(req_id, self.guild_id, self.channel_id, self.allowed_roles, done=True))
                    del self.messages[req_id]
                    save_state(self.messages)
            except asyncio.CancelledError:
                raise
            except (discord.DiscordException, OSError, ValueError, RuntimeError) as exc:
                LOG.warning("SASPAM polling error: %s: %s", type(exc).__name__, exc)
            await asyncio.sleep(2)


def main() -> None:
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    token = os.getenv("DISCORD_TOKEN", "")
    guild_id = int(os.getenv("DISCORD_GUILD_ID", "0"))
    channel_id = int(os.getenv("DISCORD_CHANNEL_ID", "0"))
    try:
        allowed_roles = parse_role_ids(os.getenv("DISCORD_APPROVAL_ROLE_IDS", ""))
    except ValueError as exc:
        raise SystemExit(str(exc)) from exc
    if not token or not guild_id or not channel_id or guild_id in allowed_roles:
        raise SystemExit("DISCORD_TOKEN, DISCORD_GUILD_ID, DISCORD_CHANNEL_ID and non-everyone approval roles are required")
    SaspamBot(guild_id, channel_id, allowed_roles).run(token, log_handler=None)


if __name__ == "__main__":
    main()
