# Recovery from SSH lockout and failed updates

[日本語](../recovery.md) | **English** · [Documentation index](../README.md) · [English README](../../README.en.md)

> **Critical:** a malfunctioning PAM hook, an offline coordinator, or an unavailable Discord approval path can prevent **new SSH logins**. This is intentional Fail Closed behavior. Before enabling SASPAM, verify a working **out-of-band** provider console, hypervisor console, or physical/serial TTY and keep an existing administrator SSH session open.

## Before enabling the gate

- Confirm that your provider's "console" connects to a local login path and does not merely initiate another SSH connection.
- Keep one working administrative SSH session open while testing a separate SSH connection.
- Back up `/etc/pam.d/sshd` and record any existing PAM customizations.
- Run `sudo saspam doctor` and complete a real `sudo saspam test` approval.
- After enabling, separately confirm **Approve**, **Deny**, and **timeout** behavior. Verify that local console, `sudo`, and `su` still function normally.

## Immediate recovery from an open session or console

```bash
sudo saspam disable
sudo /usr/sbin/sshd -t
sudo saspam status
sudo journalctl -u saspamd -u saspam-discord -n 100 --no-pager
```

`disable` removes **only** SASPAM's managed line from the SSH PAM stack. It does not remove other PAM controls or turn off OpenSSH authentication.

If the CLI is missing or broken, remove the **exact** managed line below from `/etc/pam.d/sshd` using the console:

```text
account required /usr/local/lib/security/pam_saspam.so # DisnanaSASPAM-managed
```

Do not delete unrelated `account` entries. The first-enable backup is at `/var/lib/saspam/sshd.pam.backup`. Compare differences before restoring the whole backup, since administrators may have added valid changes since it was taken.

## Bot not running: exit code 203/EXEC

An earlier alpha installer could create the Python virtual environment under restrictive permissions, preventing the unprivileged `saspam-bot` account from executing the interpreter. Newer installers grant the dedicated group the permissions needed to run the venv **without opening `/etc/saspam` secrets**.

```bash
sudo systemctl status saspam-discord --no-pager -l
sudo journalctl -u saspam-discord -n 80 --no-pager -o cat
sudo namei -l /opt/saspam/venv/bin/python
sudo runuser -u saspam-bot -- /opt/saspam/venv/bin/python -c 'import discord; print(discord.__version__)'
```

The `saspam test` command makes a synthetic request without editing PAM. It checks for an inactive Discord service before waiting. Successful `doctor` output proves only that the service is active, not that real Discord approval works; always test the button.

If the Bot stops, no extra approval is granted; requests ordinarily time out. If the daemon stops, the PAM hook refuses immediately. Do **not** change the fail-closed behavior to fail-open as a shortcut.

## Interrupted update or downgrade

The staged updater, available since alpha.4, keeps transaction backups under `/var/lib/saspam/transactions` and uses a separately scheduled systemd rollback timer. It snapshots the previous binaries, settings, virtual environment, and unit files before switching, without removing the SSH PAM hook.

```bash
sudo journalctl -u saspamd -u saspam-discord -n 100 --no-pager
sudo saspam update --rollback
sudo saspam status
```

A failed rollback can leave the daemon unavailable; new SSH connections will remain denied. Use the out-of-band console to repair or disable SASPAM's SSH hook. The timer and restore behavior have been exercised on an ephemeral Ubuntu runner, but **enabled-PAM upgrade and unexpected process failure on the actual host still require separate validation**.

Alpha.3 and earlier have no update transaction mechanism. Follow the [legacy migration procedure](updates.md) to reach the current release, then run `doctor` and an approval test before re-enabling. Normal updates from alpha.4 and later keep the PAM hook installed.

## Removing the software

```bash
sudo saspam disable
sudo saspam security rollback
sudo saspam uninstall --yes
```

The uninstall helper removes managed binaries and services but deliberately retains secrets, backups, audit logs and some working state for operator review. Purge retained credentials only after you have confirmed that they are no longer needed.

See [installation](installation.md), [security model](security.md), and [update procedure](updates.md).
