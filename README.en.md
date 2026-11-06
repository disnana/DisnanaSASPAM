# DisnanaSASPAM

[**日本語**](README.md) | **English**

**Disnana Simple Advanced Security PAM** adds an explicit Discord approval gate **after successful primary SSH authentication** and before granting an SSH session on Ubuntu. The PAM component, approval coordinator, and management CLI are written in Rust; the Discord adapter runs as an unprivileged Python service.

> **v0.1.0-alpha.5 · Developer preview.** This project has not passed an independent security audit and is **not recommended for production SSH servers**. GitHub Actions tests real SSH/PAM approval flows on Ubuntu, but real Discord Gateway end-to-end behavior, hardware FIDO, and recovery with an enabled production PAM gate require additional testing. Keep an existing administrative SSH session and an out-of-band console before enabling it.

## What it does

- **Post-authentication only:** intercepts the `sshd` PAM **account** stage after password, public-key, or FIDO primary authentication succeeds. It does not replace SSH authentication.
- **SSH-only scope:** modifies `/etc/pam.d/sshd`; does **not** install hooks into `login`, `sudo`, `su`, or `common-account`. A normal physical/serial console is outside SASPAM's PAM gate.
- **Role-based Discord approval:** accepts one or more comma-separated approval role IDs. A member must have **at least one** role *and* channel visibility in the expected guild/channel to approve or deny.
- **Japanese or English:** choose a language during setup and change it later. The core CLI, Discord embeds/buttons, and ntfy/Bark notification text follow the setting.
- **Minimal approval interaction:** Discord button clicks are silently acknowledged; the bot edits the original embed and disables completed buttons, without success follow-up messages.
- **Optional notifications:** ntfy.sh and Bark **deliver alerts only**. They do not grant SSH access; any VPN-backed web approval UI is not available in this alpha.
- **Fail Closed:** denial, expiration, coordinator failure, or unavailable Discord approval never automatically grants login.
- **Operator controls:** interactive terminal menu, targeted configuration changes, notification tests, opt-in OpenSSH connection limits, and a staged update/rollback mechanism.

### Architecture at a glance

```text
SSH primary authentication succeeds
  └─ sshd PAM account hook: pam_saspam.so (Rust)
      └─ root-only /run/saspam/pam.sock
          └─ saspamd (Rust approval coordinator)
              ├─ restricted /run/saspam/bot.sock → non-root Discord bot
              │   └─ member approves/denies in the configured guild/channel
              └─ optional HTTPS alerts → ntfy.sh / Bark (notification only)
                   ├─ approved → PAM_SUCCESS → SSH session allowed
                   └─ denied / expired / unavailable → PAM_PERM_DENIED
```

The Discord bot connects to Discord normally; Tailscale or NetBird is **not required for Discord approval**. A local console session typically remains available for recovery, but verify your provider's actual console implementation before relying on it.

## Install on a disposable Ubuntu VM

Supported **test targets:** Ubuntu 22.04 / 24.04 on amd64 or arm64 with systemd and OpenSSH Server. The source code also targets newer Ubuntu versions, but not every version is verified by the current CI matrix.

For the **currently published alpha**, specify its version explicitly:

```bash
curl -fsSL https://raw.githubusercontent.com/disnana/DisnanaSASPAM/main/scripts/install.sh | sudo bash -s -- --version v0.1.0-alpha.5
```

The installer downloads the Release archive and checks the published `SHA256SUMS`. **Installing does not enable the PAM gate.** There is no stable Release yet, so omitting `--version` is not the supported alpha-installation path.

For additional confidence, review and save the installer before running it as root. SHA-256 checks a downloaded file against the checksum provided by the same release source; **it is not independent publisher signature verification**.

### Prepare Discord

Create a bot in the Discord Developer Portal. Prepare a dedicated private channel and obtain the bot token, guild ID, channel ID, and one or more approval role IDs. Grant the bot access to the channel and permission to send messages, embeds, and read history. You do not need the privileged Message Content Intent.

```bash
sudo saspam setup       # Choose Japanese/English and configure Discord/push notifications
sudo saspam doctor      # Validate sshd, PAM module, daemon IPC, and Bot service
sudo saspam test        # Send a real Discord approval test without editing PAM
```

To configure multiple approver roles, enter IDs separated by commas:

```text
Discord server (guild) ID: 111111111111111111
Discord notification channel ID: 222222222222222222
Allowed Discord role IDs: 333333333333333333, 444444444444444444
```

Either role is sufficient (**OR**). The `@everyone` role cannot be an approver. Discord channel view permission is also required.

During setup you can choose to **mention every configured approval role** (default), **mention @everyone** (explicit confirmation), or **send without mentions**. Notification audience and authorization are separate: an `@everyone` ping **does not** give everyone approval permission. Actual pings depend on Discord channel/bot permissions.

### Enable the gate only after testing

Verify that the test was approved, that you can still access an existing administrative session, and that a real provider console/serial console works. Then:

```bash
sudo saspam enable --ack-recovery --ack-tested
```

A successful approval test must be recent (within 15 minutes of the current configuration). **Keep your existing SSH session open.** Try a *new* SSH session in a separate terminal and confirm approval, denial, and expiration before disconnecting the original session.

To remove only SASPAM's SSH PAM hook:

```bash
sudo saspam disable
sudo saspam status
```

## Interactive menu, settings, and tests

```bash
sudo saspam                   # Arrow keys / Enter / Space / Esc
sudo saspam menu              # Open the menu explicitly
sudo saspam config            # Edit one setting without repeating initial setup
sudo saspam config show       # Show settings with secret keys masked
sudo saspam config set language en
sudo saspam config set language ja
sudo saspam config set mentions roles
sudo saspam config set mentions none
sudo saspam config set timeout 60
sudo saspam config set roles 333333333333333333,444444444444444444
sudo saspam config set ntfy topic --generate
sudo saspam config set ntfy on
sudo saspam config set bark key        # Secret entered interactively, not as an argument
sudo saspam config set bark on
sudo saspam notify test ntfy
sudo saspam notify test bark
sudo saspam notify test all
```

Changes that restart services while SASPAM's SSH hook is enabled can temporarily reject new logins; an explicit acknowledgement is required. A change is validated and rollback to the previous configuration is attempted on failure. The feature is **not** an SSH authorization bypass.

Bark and ntfy providers are external services; consider privacy implications before sending source-IP or hostname metadata. Never put a Bark device key or Discord bot token on the command line or in Git.

### Authentication methods in approval requests (alpha.5)

Discord embeds now show the **primary SSH authentication method** based on OpenSSH's PAM `SSH_AUTH_INFO_0` data: Public key, Password, FIDO security key, Keyboard-interactive, or multiple completed methods. If unavailable, the display explicitly says **Unknown** rather than guessing. SSH key material, fingerprints and passwords are never sent to Discord

The parser has unit tests, but live SSH/PAM CI on Ubuntu 22.04/24.04 exposed an empty PAM authentication-info string, so these hosts currently show **Unknown**. Reliable method reporting and physical FIDO verification remain unresolved

### Discover and select Releases without a version number

```bash
sudo saspam update             # Choose from recent published Releases
sudo saspam update --list      # View available versions and installed status
sudo saspam update --latest    # Automatically select the newest compatible Release
```

The interactive menu also offers the Release picker. Older versions require explicit downgrade confirmation; reinstalling the current version requires `--allow-reinstall`. The selected version then uses the existing fail-closed update/rollback process

## Updating and rolling back

The first upgrade **from alpha.3 to alpha.4** must use the installer with SASPAM disabled because alpha.3 lacks the updater. Keep console access, disable the hook, install alpha.4, then rerun `doctor` and the Discord approval test before re-enabling.

For future versions that include the updater:

```bash
sudo saspam update --version v0.1.0-alpha.5 --dry-run
sudo saspam update --version v0.1.0-alpha.5 --allow-reinstall
sudo saspam update --version v0.1.0-alpha.3 --allow-downgrade
sudo saspam update --rollback
```

The updater stages and validates new files, snapshots the old installation and configuration, arms an independent systemd rollback timer, restarts services, and requires a Discord approval check before committing (unless the PAM gate is disabled and a test skip is explicitly requested). **The SSH PAM hook remains installed throughout the transaction.** If recovery fails, access remains denied rather than silently opening SSH.

The `--block-new-ssh` option is deliberately **not implemented** in this alpha: it returns an error instead of installing untested firewall rules.

See the [update/rollback guide](docs/en/updates.md) for details.

## Optional SSH pre-authentication limits

The SSH hardening tool is **opt-in** and does not change the listening port, turn off password authentication, or automatically disable root login:

```bash
sudo saspam security audit
sudo saspam security apply --ack-recovery
sudo saspam security rollback
```

The current policy proposes `MaxAuthTries 4`, `LoginGraceTime 45`, and `MaxStartups 10:30:60`. These are connection/authentication limits, **not IP-based time bans**. Avoid applying hardening to a production host without checking that the SSH login grace period and SASPAM approval timeout behave correctly together. Fail2ban configuration and firewall access bans are not installed automatically.

## Troubleshooting and security limits

If SSH access fails, use a still-open management session or an actual provider console:

```bash
sudo saspam disable
sudo /usr/sbin/sshd -t
sudo saspam status
sudo journalctl -u saspamd -u saspam-discord -n 100 --no-pager
```

The bot's service is `saspam-discord`. A stopped bot causes approval requests to expire without allowing entry; a stopped daemon causes immediate PAM-side denial. The coordinator status check is **not** a substitute for testing a real Discord approval.

Neither GitHub Actions tests nor checksum verification are a security certification. Hardware FIDO, full live Discord E2E, enabled-PAM failure injection, signed/provenance-verified releases, and independent security review remain release gates. Docker isolation and VPN-backed Bark/ntfy approval are **not shipped** in alpha.4.

## Documentation / ドキュメント

| Topic | English | 日本語 |
|---|---|---|
| Start here | [English README](README.en.md) | [日本語 README](README.md) |
| Installation and setup | [English](docs/en/installation.md) | [日本語](docs/installation.md) |
| Architecture and trust boundaries | [English](docs/en/architecture.md) | [日本語](docs/architecture.md) |
| Recovery and lockout | [English](docs/en/recovery.md) | [日本語](docs/recovery.md) |
| Security and limitations | [English](docs/en/security.md) | [日本語](docs/security.md) |
| Update/downgrade/rollback | [English](docs/en/updates.md) | [日本語](docs/updates.md) |

See also [documentation index](docs/README.md).

## License and responsible disclosure

MIT License. Please use a private reporting channel such as GitHub private vulnerability reporting instead of publicly posting exploitable vulnerabilities in an Issue.
