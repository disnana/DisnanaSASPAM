# Updates, downgrades and automated rollback (v1.0.0)

[日本語](../updates.md) | **English** · [Documentation index](../README.md) · [English README](../../README.en.md)

The SASPAM update transaction is designed to **keep the SSH PAM account hook installed at all times**. While the coordinator or Discord Bot is stopped, new requests can fail closed; the updater is **not** intended to bypass MFA or silently disable protection.

## Before starting

- Use an **out-of-band console** and keep an existing administrative SSH session open
- Confirm `sudo saspam doctor` and `sudo saspam test` have succeeded with the current configuration
- Record your currently installed version with `sudo cat /var/lib/saspam/installed-version`
- If SASPAM is currently enabled, plan for new logins to be denied while the coordinator restarts
- Use a disposable Ubuntu VM to rehearse updates and recovery first; v1.0.0 is not independently audited and is not recommended for production SSH servers

## Commands

These examples upgrade an older installation that already has the updater (alpha.4 or later) to v1.0.0. Alpha.3 and earlier require the separate legacy procedure below.

```bash
# Download/check the requested release without switching live files
sudo saspam update --version v1.0.0 --dry-run

# Upgrade an older installation to the published release
sudo saspam update --version v1.0.0
```

If v1.0.0 is already installed, an intentional reinstall requires `--allow-reinstall`. A same-version preflight also needs this flag: `sudo saspam update --version v1.0.0 --allow-reinstall --dry-run`. Rollback restores a committed update backup; it does not undo the initial installation.

Only when deliberately selecting an older, compatibility-reviewed tag should you add `--allow-downgrade`. Downgrading to an alpha is not the normal update path. Never force a release that cannot read the current configuration or support the update/recovery protocol.

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

## Legacy migration from alpha.3 or earlier (no updater)

Alpha.3 and earlier do **not** ship the update transaction helper. The following procedure is only for those legacy versions. It does not establish that every legacy configuration has been validated; rehearse compatibility and recovery on a disposable VM first:

1. Confirm your recovery console, preserve an active admin SSH session and securely back up the secret configuration
2. Run `sudo saspam disable` and verify with `sudo saspam status` before replacing components. SASPAM approval is absent during this interval, so control access while migrating
3. Install v1.0.0 using the explicitly versioned [installation command](installation.md)
4. **Run `sudo saspam setup` for every legacy migration** to review/save the configuration and restart the daemon and Bot. The installer alone does not restart running old processes. Alpha.1's old user-ID settings are incompatible and must be replaced with role-based settings. Then run `sudo saspam doctor` and complete a real Discord approval via `sudo saspam test`
5. Enable the gate only after successful tests, with `sudo saspam enable --ack-recovery --ack-tested`, and test from another SSH connection

Alpha.4 and later use `saspam update --version v1.0.0` with the existing PAM hook kept in place, subject to compatibility and recovery checks. Alpha.4 requires the explicit version; an intermediate installation of alpha.5 is not required by the updater.

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

## alpha.5 and later: Discover Releases without a version number

```bash
sudo saspam update
sudo saspam update --list
sudo saspam update --latest
```

The CLI retrieves recent published GitHub Releases and offers an interactive selector showing newer/current/older status. Only Releases with a compatible architecture archive and SHA256SUMS appear. **`--latest` includes prereleases**, unlike omitting the installer's version (which selects the latest stable Release). Use `--version v1.0.0` to pin this stable version. A downgrade must be explicitly authorized
