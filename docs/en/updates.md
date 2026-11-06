# Updates, downgrades and automated rollback (alpha)

[日本語](../updates.md) | **English** · [Documentation index](../README.md) · [English README](../../README.en.md)

The SASPAM update transaction is designed to **keep the SSH PAM account hook installed at all times**. While the coordinator or Discord Bot is stopped, new requests can fail closed; the updater is **not** intended to bypass MFA or silently disable protection.

## Before starting

- Use an **out-of-band console** and keep an existing administrative SSH session open
- Confirm `sudo saspam doctor` and `sudo saspam test` have succeeded with the current configuration
- Record your currently installed version with `sudo cat /var/lib/saspam/installed-version`
- If SASPAM is currently enabled, plan for new logins to be denied while the coordinator restarts
- Use a disposable Ubuntu VM to rehearse updates and recovery first; the transaction implementation remains an unaudited alpha

## Commands

```bash
# Download/check the requested release without switching live files
sudo saspam update --version v0.1.0-alpha.4 --dry-run

# Upgrade to a newer published tag when available
sudo saspam update --version v0.1.0-alpha.4

# Reinstall exactly the current version (normally rejected without the flag)
sudo saspam update --version v0.1.0-alpha.4 --allow-reinstall

# A downgrade must be explicitly allowed
sudo saspam update --version v0.1.0-alpha.3 --allow-downgrade

# Restore the previous committed transaction
sudo saspam update --rollback
```

The normal transaction performs a new Discord approval test before committing. `--skip-approval-test` is supported only **when the SSH PAM gate is disabled**. Version ordering requires an explicit downgrade flag for older releases. The updater rejects concurrent operations and keeps recovery backups restricted to root.

> The examples above are **syntax examples**, not a promise that a particular old/new version is compatible with the current configuration. In particular, downgrading to a release without role-based configuration or the current update protocol can make services fail; the preflight config check should refuse an incompatible target. Do not force an incompatible downgrade.

## Transaction sequence

1. **Preflight:** validate tag syntax, architecture, selected version and requested downgrade flag; check PAM/service state and the existence of recovery helpers
2. **Verify download:** obtain the versioned Release archive and its `SHA256SUMS`; reject unexpected archive members and non-regular files
3. **Validate before cutover:** test the packaged daemon against the current config with `--check-config`, compile the Python Bot, create a new isolated Python venv and verify it runs as the dedicated Bot user
4. **Snapshot:** copy existing binaries, PAM library, service units, configuration, secrets and the current Python venv into a root-only transaction directory under `/var/lib/saspam/transactions`
5. **Arm independent recovery:** schedule systemd to run `rollback-upgrade.sh` if the transaction is left pending. The watchdog is independent of the updater's terminal/SSH session
6. **Swap services:** stop Bot and coordinator, replace binaries and venv, reload systemd and start services that were previously active. SASPAM's PAM configuration line remains installed throughout
7. **Verify and commit:** run an approval check unless an explicitly permitted test skip applies; on success mark the transaction committed and disarm the timer
8. **Restore on error:** if the updater fails or the timer detects an incomplete transaction, restore previous files and restart previous services. If recovery itself fails, the PAM gate remains fail closed; use the console

The updater uses a lock to prevent concurrent transactions and an independent recovery lock. A successful manual rollback and a simulated interrupted-upgrade restoration were exercised on an ephemeral Ubuntu GitHub Actions runner. This is useful evidence, **not** a guarantee that abrupt crashes and enabled-PAM upgrades are safe on every host.

## First migration from alpha.3

Alpha.3 does **not** ship the update transaction helper. The first alpha.3 → alpha.4 transition must be done carefully:

1. Confirm your recovery console and preserve an active admin SSH session
2. Run `sudo saspam disable` from that session before replacing components
3. Install alpha.4 using the versioned [installation command](installation.md)
4. Retain the existing configuration (or rerun setup if needed), then run `sudo saspam doctor` and complete a real Discord approval via `sudo saspam test`
5. Enable the gate only after successful tests, with `sudo saspam enable --ack-recovery --ack-tested`, and test from another SSH connection

Further alpha.4+ updates can use `saspam update` with the existing PAM hook kept in place, subject to compatibility and recovery checks.

## Why new-SSH blocking is not supported yet

`--block-new-ssh` currently **fails with an error** instead of altering iptables/nftables, UFW or port settings. Blocking a management interface without a verified, automatic firewall-rule rollback could permanently lock an administrator out, especially with custom SSH ports, IPv6, Tailscale or NetBird. The optional feature needs independent integration tests before it can be offered safely.

## Failures and logs

```bash
sudo journalctl -u saspamd -u saspam-discord -n 100 --no-pager
sudo saspam status
sudo saspam update --rollback
```

Keep the backup transaction files until you've reviewed and confirmed recovery. If the daemon cannot recover, use the provider console and, only as a deliberate recovery measure, `sudo saspam disable` (or remove SASPAM's exact managed PAM line). Do not delete unrelated PAM configuration.

See [recovery](recovery.md) and [security limitations](security.md).
