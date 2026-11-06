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
    wire = json.dumps({"version": 2, **payload}, separators=(",", ":")).encode() + b"\n"
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


def safe_value(value: Any) -> str:
    """Prevent source-controlled fields from injecting formatting or mentions."""
    text = str(value if value is not None else "不明").replace("\n", " ").replace("\r", " ").strip()[:128]
    return discord.utils.escape_markdown(discord.utils.escape_mentions(text))


def translate(language: str, japanese: str, english: str) -> str:
    return english if language == "en" else japanese


def embed(req: dict[str, Any], language: str = "ja") -> discord.Embed:
    """Compact Japanese/English status card without leaking token or key data."""
    status = req.get("status", "pending")
    colors = {"pending": 0xE3A008, "approved": 0x2ECC71, "denied": 0xE74C3C, "expired": 0x778899}
    titles = {
        "pending": ("🔐 SSHログイン承認リクエスト", "🔐 SSH Login Approval"),
        "approved": ("✅ SSHログインを承認しました", "✅ SSH Login Approved"),
        "denied": ("⛔ SSHログインを拒否しました", "⛔ SSH Login Denied"),
        "expired": ("⏰ SSHログイン承認がタイムアウトしました", "⏰ SSH Approval Timed Out"),
    }
    descriptions = {
        "pending": ("**承認待ち** — SSHの一次認証に成功した接続があります", "**Pending** — SSH primary authentication succeeded"),
        "approved": ("**承認済み** — PAMがこのログインを許可しました", "**Approved** — PAM approved this login"),
        "denied": ("**拒否済み** — このログインは許可されません", "**Denied** — This login was rejected"),
        "expired": ("**期限切れ** — 承認されなかったためログインを拒否しました", "**Expired** — No approval was received"),
    }
    title_ja, title_en = titles.get(status, ("🛡️ SSHログイン確認", "🛡️ SSH Login"))
    desc_ja, desc_en = descriptions.get(status, ("状態不明", "Unknown status"))
    user = safe_value(req.get("user"))
    remote = safe_value(req.get("remote_host"))
    hostname = safe_value(req.get("server_name"))
    e = discord.Embed(
        title=translate(language, title_ja, title_en),
        description=translate(language, desc_ja, desc_en) + "\n\n" +
                    translate(language, f"ユーザー **{user}** が **{remote}** からログインを試みています",
                              f"User **{user}** requested access from **{remote}**"),
        color=colors.get(status, 0x778899),
    )
    actor = str(req.get("actor_id") or "")
    if status in ("approved", "denied") and actor.isascii() and actor.isdecimal() and 15 <= len(actor) <= 22:
        verb = translate(language, "承認" if status == "approved" else "拒否",
                         "Approved" if status == "approved" else "Denied")
        e.add_field(name=translate(language, "操作した人", "Decision by"),
                    value=f"<@{actor}> — {verb}", inline=False)
    e.add_field(name=translate(language, "🖥️ 接続先サーバー", "🖥️ Server"), value=hostname, inline=True)
    e.add_field(name=translate(language, "🌐 接続元IP / ホスト", "🌐 Source IP / Host"), value=remote, inline=True)
    # Show only OpenSSH's verified auth method categories, never raw key data.
    auth_labels = {
        "public_key": ("公開鍵認証", "Public key"),
        "password": ("パスワード認証", "Password"),
        "fido": ("公開鍵認証（FIDO対応鍵）", "Public key (FIDO-backed)"),
        "fido_ecdsa_sk": ("公開鍵認証（FIDO / ECDSA-SK）", "Public key (FIDO / ECDSA-SK)"),
        "fido_ed25519_sk": ("公開鍵認証（FIDO / Ed25519-SK）", "Public key (FIDO / Ed25519-SK)"),
        "keyboard_interactive": ("キーボード対話認証", "Keyboard-interactive"),
        "hostbased": ("ホストベース認証", "Host-based"),
        "unknown": ("不明（取得不可）", "Unknown (unavailable)"),
    }
    auth_methods = req.get("auth_methods") or ["unknown"]
    if not isinstance(auth_methods, list) or len(auth_methods) > 8:
        auth_methods = ["unknown"]
    labels = []
    for method in auth_methods:
        ja, en = auth_labels.get(method, auth_labels["unknown"]) if isinstance(method, str) else auth_labels["unknown"]
        label = translate(language, ja, en)
        # Preserve ordered multi-factor sequences, including publickey,publickey.
        labels.append(label)
    e.add_field(name=translate(language, "🔑 SSH一次認証方式", "🔑 SSH authentication method"),
                value=" + ".join(labels), inline=False)
    created = int(req.get("created_unix", 0))
    expires = int(req.get("expires_unix", 0))
    if created > 0:
        e.add_field(name=translate(language, "📅 要求日時", "📅 Requested at"), value=f"<t:{created}:F>", inline=False)
    if status == "pending" and expires > 0:
        e.add_field(name=translate(language, "⏳ 承認期限", "⏳ Approval deadline"),
                    value=f"<t:{expires}:R> (<t:{expires}:T>)", inline=False)
    e.add_field(name=translate(language, "セッションID", "Session ID"),
                value=f"\u0060{safe_value(req.get('request_id'))}\u0060", inline=False)
    e.set_footer(text=translate(language, 
        "DisnanaSASPAM • 未承認ならアクセス拒否" if status == "pending" else "DisnanaSASPAM • 処理済み",
        "DisnanaSASPAM • Denied unless approved" if status == "pending" else "DisnanaSASPAM • Completed"))
    return e


def notification_mention(mode: str, roles: frozenset[int]) -> tuple[str | None, discord.AllowedMentions]:
    """Mention mode is independent of login authorization."""
    if mode == "none":
        return None, discord.AllowedMentions.none()
    if mode == "roles":
        # Mention every configured approver role, not the users individually.
        ids = sorted(roles)
        return " ".join(f"<@&{role_id}>" for role_id in ids), discord.AllowedMentions(
            everyone=False, users=False, roles=[discord.Object(id=role_id) for role_id in ids],
            replied_user=False
        )
    if mode == "everyone":
        return "@everyone", discord.AllowedMentions(everyone=True, users=False, roles=False, replied_user=False)
    raise ValueError("Unknown DISCORD_MENTION_MODE: use none, roles, or everyone")


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
    def __init__(self, request_id: str, guild_id: int, channel_id: int, allowed_roles: frozenset[int], done: bool = False, language: str = "ja"):
        super().__init__(timeout=None)
        self.request_id = request_id
        self.language = language
        self.guild_id = guild_id
        self.channel_id = channel_id
        self.allowed_roles = allowed_roles
        for decision, label, style in (("approve", translate(language, "承認", "Approve"), discord.ButtonStyle.success),
                                       ("deny", translate(language, "拒否", "Deny"), discord.ButtonStyle.danger)):
            button = discord.ui.Button(label=label, style=style,
                                       custom_id=f"saspam:v2:{decision}:{request_id}", disabled=done)
            button.callback = self._callback(decision)
            self.add_item(button)

    def _callback(self, decision: str):
        async def action(interaction: discord.Interaction) -> None:
            # A component message-update defer is a silent ACK. Do not create an
            # ephemeral reply, progress indicator or Follow-up for this action.
            await interaction.response.defer(thinking=False)
            if not isinstance(interaction.user, discord.Member):
                LOG.warning("Rejected non-member approval interaction")
                return
            role_ids = {role.id for role in interaction.user.roles}
            if not has_approval_role(
                interaction.guild_id, interaction.channel_id, role_ids,
                interaction.permissions.view_channel, self.guild_id, self.channel_id, self.allowed_roles
            ) or not interaction.message or not interaction.client.user or interaction.message.author.id != interaction.client.user.id:
                LOG.warning("Rejected unauthorized approval interaction for request %s", self.request_id)
                return
            try:
                result = await request({"action": "decide", "request_id": self.request_id,
                                        "actor_id": str(interaction.user.id), "decision": decision,
                                        "guild_id": str(interaction.guild_id),
                                        "channel_id": str(interaction.channel_id),
                                        "role_ids": [str(role_id) for role_id in sorted(role_ids)]})
                if not result.get("ok"):
                    # Another client may already have approved, denied or expired
                    # this request. The poller refreshes the original Embed.
                    LOG.info("Approval request %s already completed or rejected", self.request_id)
                    return
                req_reply = await request({"action": "get", "request_id": self.request_id})
                if req_reply.get("ok") and req_reply.get("requests"):
                    updated = req_reply["requests"][0]
                    if updated.get("status") != "pending":
                        for item in self.children:
                            item.disabled = True
                    await interaction.message.edit(embed=embed(updated, self.language), view=self)
                else:
                    LOG.warning("Unable to load finalized state for request %s", self.request_id)
            except (OSError, ValueError, RuntimeError, discord.DiscordException) as exc:
                # Fail closed: the coordinator remains authoritative. The poller
                # can still update the original message once it reconnects.
                LOG.warning("Approval update failed (%s) for request %s", type(exc).__name__, self.request_id)
        return action

class SaspamBot(discord.Client):
    def __init__(self, guild_id: int, channel_id: int, allowed_roles: frozenset[int], mention_mode: str = "none", language: str = "ja"):
        intents = discord.Intents.default()
        intents.message_content = False  # No privileged intent or message reading required
        super().__init__(intents=intents, allowed_mentions=discord.AllowedMentions.none())
        self.guild_id = guild_id
        self.channel_id = channel_id
        self.allowed_roles = allowed_roles
        self.language = language
        self.mention_text, self.notification_mentions = notification_mention(mention_mode, allowed_roles)
        self.messages: dict[str, int] = load_state()
        self.poller: asyncio.Task | None = None

    async def setup_hook(self) -> None:
        for req_id, message_id in self.messages.items():
            self.add_view(RequestView(req_id, self.guild_id, self.channel_id, self.allowed_roles, language=self.language), message_id=message_id)
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
                        message = await ch.send(
                            content=self.mention_text,
                            embed=embed(req, self.language),
                            view=RequestView(req_id, self.guild_id, self.channel_id, self.allowed_roles, language=self.language),
                            allowed_mentions=self.notification_mentions,
                        )
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
                        await message.edit(embed=embed(row, self.language), view=RequestView(req_id, self.guild_id, self.channel_id, self.allowed_roles, done=True, language=self.language))
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
        # Existing installs without a mention setting remain quiet by default.
        mention_mode = os.getenv("DISCORD_MENTION_MODE", "none")
        language = os.getenv("DISCORD_LANGUAGE", "ja")
        if language not in ("ja", "en"):
            raise ValueError("DISCORD_LANGUAGE must be ja or en")
        notification_mention(mention_mode, allowed_roles)
    except ValueError as exc:
        raise SystemExit(str(exc)) from exc
    if not token or not guild_id or not channel_id or guild_id in allowed_roles:
        raise SystemExit("DISCORD_TOKEN, DISCORD_GUILD_ID, DISCORD_CHANNEL_ID and non-everyone approval roles are required")
    SaspamBot(guild_id, channel_id, allowed_roles, mention_mode=mention_mode, language=language).run(token, log_handler=None)


if __name__ == "__main__":
    main()
