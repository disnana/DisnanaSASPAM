# Security model and limitations

[日本語](../security.md) | **English** · [Documentation index](../README.md) · [English README](../../README.en.md)

> **v1.0.0, not independently audited.** Passing automated CI, an SSH integration test, or checksumming a Release archive **does not** establish production security. Plan a separate security review and operator test before stable release.

## What SASPAM aims to protect

- Additional **manual permission after** public-key/FIDO/password SSH primary authentication, before granting access through the `sshd` account PAM stage
- One-use request IDs, approval deadlines, explicit denial and fail-closed response to malformed/invalid protocol messages or component unavailability
- Clear separation between the root PAM/coordinator processes and the non-root Discord Bot
- Confined local Unix sockets and approval actor role checks, including expected guild/channel
- Configuration validation, small PAM-specific file edits, explicit activation and a recovery path
- Optional, operator-controlled OpenSSH connection/authentication limits

## What is not guaranteed

- Protecting a host where an attacker already has root and can modify PAM or services
- Protecting a compromised Discord approver account or Bot token
- Completely trustworthy Discord message logs (messages can be edited/deleted); preserve and export local audit logs where needed
- Reauthorizing an already established SSH session, an already-open multiplexed channel or arbitrary commands executed inside it
- Preventing SSH authentication attempts *before* primary authentication; SASPAM's account hook runs **after** success
- Guaranteeing hardware FIDO coverage without hardware tests, or certifying all cloud console implementations
- Protecting metadata sent to public push services from the provider's access or retention

## Role authorization and mention behavior

The Bot accepts approval or denial only when the interaction comes from the expected guild and channel, channel visibility is present and the actor holds at least one configured approval role. The coordinator independently verifies the configured role IDs, guild/channel and request state.

Role IDs can be configured as a comma-separated list (logical **OR**). The `@everyone` role is rejected as an approval role. Notifications can mention all configured roles (default), `@everyone` with explicit confirmation, or no one. **A ping is not permission**: an unauthorized viewer cannot approve merely because they received `@everyone`.

Discord's gateway sends member roles to the adapter; the coordinator cannot independently consult Discord on every click. A compromised local Bot process could claim false role membership over its allowed IPC socket. Harden the bot account, token, permissions and process isolation. The Bot's virtual environment is accessible to the dedicated group for execution, while `/etc/saspam/discord.env` and the core configuration must remain root-only.

## Socket and process boundaries

- `/run/saspam/pam.sock`: root-only PAM communication, checked against local peer identity
- `/run/saspam/bot.sock`: dedicated bot-accessible socket and process-UID verification
- No remote/public approval API; ntfy.sh and Bark provide **notifications only**
- PAM module does not create a network client or Discord connection; blocking on local IPC is not active CPU polling
- Rust daemon uses an async runtime, caps pending requests and validates input; further load/DoS/replay auditing is required

## SSH hardening (opt in)

```bash
sudo saspam security audit
sudo saspam security apply --ack-recovery
sudo saspam security rollback
```

Current proposed OpenSSH settings are `MaxAuthTries 4`, `LoginGraceTime 45` and `MaxStartups 10:30:60`. They limit attempts per connection, pre-authentication grace time and concurrent unauthenticated connections; they do **not** implement an IP-based time ban or automatically install Fail2ban.

The tool intentionally does **not** change the SSH port, disable password authentication or change `PermitRootLogin`. Existing `Match` rules, network firewalls and distribution-specific sshd configuration can override expectations. Check the interaction of `LoginGraceTime` and the SASPAM approval deadline before production application. Never impose a hardening value without testing a second login and preserving console access.

## Updates, supply chain and recovery

The updater stages a versioned GitHub Release, checks SHA-256, validates the configuration and Python code, snapshots the old installation, and arms an independent systemd rollback timer. It preserves the SSH PAM hook while services restart, so interruption results in **denial**, not silent access.

The current SHA-256 file is published alongside the release and is **not an independent authenticity signature**. Signed artifact provenance, pinned/verified Python transitive dependencies, build reproducibility, dynamic fault injection, and a third-party audit should be completed before calling this stable.

The `--block-new-ssh` option is **disabled**: it returns an error rather than applying firewall rules with uncertain rollback behavior. Docker isolation and VPN/web approval are also outside alpha.4.

## Reporting a vulnerability

Do not disclose exploit details in a public Issue. Use GitHub private vulnerability reporting if available, or another verified private channel with the maintainers.

See [architecture](architecture.md), [recovery](recovery.md), and [updates](updates.md).
