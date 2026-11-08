# Architecture and trust boundaries

[日本語](../architecture.md) | **English** · [Documentation index](../README.md) · [English README](../../README.en.md)

DisnanaSASPAM separates the privileged SSH/PAM decision from an unprivileged Discord notification adapter. The goal is to keep the PAM module small, use local IPC, and avoid making SSH authorization depend on arbitrary public notification URLs.

## Authentication flow

```text
Client connects to sshd
  └─ OpenSSH validates public key / FIDO / password
      └─ Linux-PAM account phase: pam_sm_acct_mgmt (pam_saspam.so)
          └─ Unix socket: /run/saspam/pam.sock (root only)
              └─ saspamd, Rust coordinator (root)
                  ├─ track unique request ID, deadline and decision
                  ├─ /run/saspam/bot.sock (restricted to dedicated Bot)
                  │    └─ Python discord.py Bot (non-root)
                  │        └─ validate guild, channel, VIEW_CHANNEL and role
                  │             ├─ Approve → single-use permission
                  │             └─ Deny → permission rejected
                  └─ HTTPS ntfy/Bark alerts (delivery only, no approval link)

          PAM_SUCCESS only when approved
          PAM_PERM_DENIED on denial, timeout, malformed response or failure
```

Only `/etc/pam.d/sshd` receives SASPAM's managed `account required` line. Local console services, `sudo`, `su` and shared `common-account` are not modified. Some providers label an SSH-backed rescue tunnel a "console"; you must independently verify that your recovery console bypasses sshd.

## Components

| Component | Purpose | Privilege |
|---|---|---|
| `pam_saspam.so` (Rust) | Read PAM service, username and remote host; ask local coordinator; return allow/deny | Loaded inside sshd PAM |
| `saspamd` (Rust) | Maintain request IDs, expiration, actor/role checks, audit events and opt-in push delivery | Root daemon |
| `saspam` (Rust CLI) | Setup, diagnosis, tests, enabling/disabling the managed hook, settings and update/rollback | Root administration |
| `saspam-discord` (discord.py) | Publish localized embeds/buttons, handle decisions, edit original message, persist message references | Dedicated `saspam-bot` account |
| ntfy.sh and Bark | Optional HTTP(S) push notification delivery | External services, **no approval capability** |

The CLI and Bot use the configured language (`ja` or `en`). The Discord action uses a silent component defer and edits the original message; success does not send a separate ephemeral follow-up.

## Security boundaries

**SSH/PAM → daemon:** root-only Unix socket `/run/saspam/pam.sock`, with peer credential checks. The protocol uses versioned newline-delimited JSON over local sockets; no SSH private keys or authentication passwords are transported.

**Bot → daemon:** separate socket `/run/saspam/bot.sock`. Filesystem permissions plus an explicit dedicated Bot UID check restrict who can submit approval decisions. The Bot cannot rewrite PAM configuration with its ordinary privileges. Nevertheless, a compromised Bot process could forge its role assertions over the IPC, so protect its token, UID and socket.

**Discord interaction:** the Bot checks that an interaction is in the configured guild/channel, the actor has channel visibility, and its member roles intersect the configured approval roles. The daemon also checks configured guild, channel and allowed role IDs, single-use decisions and expiration. **Mentions do not confer authorization**: `@everyone` may notify viewers but cannot authorize them.

**Push services:** ntfy and Bark are notification only. There is no publicly exposed "approve" endpoint and no bearer approval token embedded in a push URL. Treat source-IP and server metadata as potentially sensitive to the provider.

## Runtime characteristics

The PAM component does **not** start Discord, an HTTP client, or a background polling loop. It blocks waiting for a local response; blocking time is not sustained CPU utilization. The Rust daemon uses a single async Tokio reactor and waits on socket readiness and a watch channel for decisions, with an upper bound on pending requests. A shared HTTP client avoids per-request TLS setup where possible. The Discord adapter polls the local request state and consumes separate resources; do not confuse its polling frequency with the PAM module's resource use.

The architecture is intentionally **Fail Closed**. If the daemon crashes, the account hook cannot grant access. If the Bot is offline, approval cannot occur and pending requests expire. Pre-existing SSH sessions or multiplexed channels are outside the gate; an already authenticated session is not reauthorized on every command.

## Remaining production work

GitHub Actions exercises real SSH/PAM flows with password and public-key authentication on Ubuntu. Hardware FIDO, full live Discord API end-to-end verification, active-PAM upgrade recovery, fault injection under load, and independent security review remain unfinished in v1.0.0. Production SSH deployment is not recommended. Optional Docker Bot isolation and VPN web approval are not part of v1.0.0.

See [security model](security.md) and [updates](updates.md).
