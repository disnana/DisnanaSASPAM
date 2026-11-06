# Installation and configuration

[日本語](../installation.md) | **English** · [Documentation index](../README.md) · [English README](../../README.en.md)

> **Developer preview — v0.1.0-alpha.4.** Not independently security audited. Use a disposable Ubuntu VM first. Keep a working SSH session and a tested provider console or serial console before installing the PAM hook.

## Requirements

- Ubuntu 22.04 or 24.04, **amd64** or **arm64**, with systemd and OpenSSH Server. Other versions are not all covered by CI.
- SSH server configuration allows `UsePAM yes`. The installer and CLI must not silently change this.
- A Discord bot and private notification channel, with a valid bot token, guild ID, channel ID, and at least one approval role ID.
- A recovery channel **independent from new SSH logins**, for example a VM serial console or hypervisor console. Verify that your provider console is actually a local login path, not just an SSH proxy.

## Install the published alpha

For the published test release, **specify the prerelease tag**:

```bash
curl -fsSL https://raw.githubusercontent.com/disnana/DisnanaSASPAM/main/scripts/install.sh | sudo bash -s -- --version v0.1.0-alpha.4
```

The script checks the Release archive against its published `SHA256SUMS`, installs the binaries and Python adapter under systemd, and leaves the SSH PAM stack unchanged. **It does not enable the approval gate**. The checksums are not a publisher signature: inspect the source and verify the release independently if your trust policy requires it.

For a source build instead:

```bash
sudo apt-get install -y git build-essential pkg-config libpam0g-dev python3 python3-venv python3-pip
git clone https://github.com/disnana/DisnanaSASPAM.git
cd DisnanaSASPAM
# Install a stable Rust toolchain from https://rustup.rs/ first.
cargo test --locked --workspace
cargo build --locked --release --workspace
sudo bash scripts/install-from-build.sh
```

The developer installer also leaves `/etc/pam.d/sshd` alone. Do not use a mutable source checkout as a substitute for a reviewed Release on production SSH infrastructure.

## Create and prepare a Discord bot

1. In the Discord Developer Portal, create a bot and obtain its token. Never paste the token into an issue, the shell command line, or a Git commit.
2. Add the bot to the intended Discord server. Allow it to view the **dedicated private channel**, send messages, embed links, and read message history. Role or `@everyone` notifications may also require mention permissions.
3. Enable Discord's developer mode to copy the guild/server ID, channel ID and approval role IDs. Multiple approval roles can be entered with commas; **any one** role suffices.
4. Restrict the notification channel's view permission to appropriate staff. **The bot checks member roles and channel visibility again** when someone clicks a button. The `@everyone` role cannot be used as an approval role.

The privileged Message Content Intent is **not required**.

## First-time setup and language

```bash
sudo saspam setup
```

The setup wizard starts with a choice of **日本語 (Japanese)** or **English**. The chosen language is stored in the root-only configuration and used for the management UI, Discord approval embeds/buttons, and ntfy/Bark notification text.

Provide the server name, approval timeout (10–180 seconds, default 60), guild ID, channel ID, approval role IDs, and the hidden Discord bot token. Choose whether to notify all configured approval roles, `@everyone` (extra explicit confirmation), or nobody. Optionally configure ntfy and Bark **as notification-only services**.

Example approval-role entry:

```text
Guild ID: 111111111111111111
Channel ID: 222222222222222222
Approval role IDs: 333333333333333333, 444444444444444444
```

Both role IDs must be real Discord role IDs in the same guild. The bot should have suitable channel permissions. A viewer without an allowed role is not an approver.

## Check before enabling SSH protection

```bash
sudo saspam doctor
sudo saspam test
```

`doctor` checks the sshd syntax, effective `UsePAM` setting, module installation, root coordinator IPC, and whether the Discord service is active. The coordinator being active alone is **not enough**. `test` submits a synthetic post-auth request **without changing PAM**; approve it from the configured Discord channel.

After a successful test, verify an out-of-band login path **and keep your current SSH session open**, then:

```bash
sudo saspam enable --ack-recovery --ack-tested
```

The test receipt must be from the current configuration and within 15 minutes. Try another SSH connection from a **different terminal** and confirm approval, denial, and timeout while the original session remains open.

The managed PAM account hook applies only to `sshd`. The installed software does not add hooks to local console `login`, `sudo`, `su`, or `common-account`. See [architecture](architecture.md) and [recovery](recovery.md).

## Change one setting later

You do not need to rerun the entire setup. Use the TestDisk-like terminal menu (arrow keys, Space/Enter, Esc), or a specific subcommand:

```bash
sudo saspam                   # Interactive main menu
sudo saspam config            # Partial settings menu
sudo saspam config show       # Display non-secret settings
sudo saspam config set language en
sudo saspam config set language ja
sudo saspam config set mentions roles
sudo saspam config set mentions none
sudo saspam config set timeout 60
sudo saspam config set roles 333333333333333333,444444444444444444
sudo saspam config set ntfy topic --generate
sudo saspam config set ntfy on
sudo saspam config set bark key  # Prompts for the key without echoing it
sudo saspam config set bark on
```

If SSH protection is enabled, a config change that restarts the services can temporarily deny requests. The CLI requires confirmation or `--ack-interruption` where appropriate and attempts to restore previous config if validation/restart fails. **Do not treat this as zero-downtime configuration editing.**

### Test push notifications

```bash
sudo saspam notify test ntfy
sudo saspam notify test bark
sudo saspam notify test all
```

The selected provider must be enabled. These tests verify delivery from the daemon; **they do not approve an SSH session**. The public ntfy.sh topic and the Bark device key should be chosen and handled as sensitive data. Push messages may expose hostnames, usernames and source IPs to the provider.

## Diagnose common failures

| Symptom | First check |
|---|---|
| `sshd -t` fails | Existing SSH configuration needs attention |
| `UsePAM no` | SASPAM's post-auth hook will not run |
| `saspamd` fails to start | `sudo journalctl -u saspamd -n 80 --no-pager` |
| Discord Bot stopped | `sudo journalctl -u saspam-discord -n 80 --no-pager` |
| `203/EXEC Permission denied` | Check `namei -l /opt/saspam/venv/bin/python` and `saspam-bot` execution rights |
| No Discord message | Verify bot token, guild/channel, bot permissions, network, and logs |
| Approval button denied | Confirm one allowed approval role, matching guild/channel, and view permission |
| `saspam enable` rejected | Re-run `doctor`, finish the Discord test, confirm console recovery |
| Timed-out SSH login | Default behavior is Fail Closed; see [recovery](recovery.md) |

Read [updates and rollback](updates.md) before upgrading a host whose SSH PAM gate is already active.

## alpha.5: Primary SSH authentication method

The Discord approval Embed shows verified method categories (public key, password, FIDO, keyboard-interactive or unknown) from OpenSSH's `SSH_AUTH_INFO_0` PAM environment. The module never forwards raw public keys, fingerprints or passwords. Hardware FIDO classification still needs a real-device end-to-end test
