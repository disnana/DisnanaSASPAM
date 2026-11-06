# Experimental OpenSSH-to-PAM authentication metadata adapter

**Not supported for production or included in any stable SASPAM Release.**

This research patch targets upstream OpenSSH pinned OpenSSH 10.6p2 portable stable-branch commit `0351d353099d1dcd75ac456720dd14fd90ad1cfb` (not the Ubuntu distribution's security-patched OpenSSH). It does not replace system `sshd`, modify existing PAM files, or run automatically on user hosts.

The experimental bridge now records each privileged-monitor-verified factor (including partially completed multi-factor logins), before exposing the bounded method list to PAM account. It uses only method names and public-key algorithms, never key contents. The stock OpenSSH privilege-separated monitor verifies the final authentication method before executing PAM account rules, but `SSH_AUTH_INFO_0` is empty at the time SASPAM's account hook runs. The patch forwards a **bounded method name and public-key algorithm only** via `SASPAM_VERIFIED_AUTH_INFO_0` from the already-privileged monitor's verified state. No key bytes, passwords or fingerprints are included.

The normal SASPAM PAM module can use the signal for a display-only category; authentication authorization still depends on the original PAM account gate and Discord approval.

Security caveats:
- This modifies OpenSSH itself, an extremely sensitive service
- A custom build bypasses distro-specific patches and unattended security updates
- Upstream / distro review, real FIDO hardware tests, compatibility with all supported Ubuntu package versions, update/rollback/lockout tests and independent audits are needed
- Do **not** deploy the research OpenSSH build to production

The integration test builds OpenSSH in an ephemeral GitHub Actions runner and binds an extra localhost SSH port only. The system OpenSSH installation is unchanged.

### Experimental tests added
- Two server-enforced factors (`AuthenticationMethods publickey,password`) must appear in order **before** Discord approval
- Stock Ubuntu runs still accept **Unknown**; experimental patched OpenSSH tests require exact, non-guessed methods
- The buffer is process-local to each privileged OpenSSH monitor and bounded at 512 bytes

### I/O and latency constraints (v1 gate)

- The privileged monitor maintains a 512-byte fixed-size, per-SSH-connection method list. Processing is event-driven **only after a verified authentication factor**; no additional file reads, log polling, disk writes, background threads, or network requests are introduced by this patch.
- The one-time `do_pam_putenv` call immediately before the existing PAM account check is an in-memory metadata transfer, not a new authentication decision.
- If the metadata buffer fills up or parsing cannot determine a valid method, the display falls back to Unknown **without failing or delaying the login solely because of the display feature**. Original PAM/Discord authorization remains fail-closed.
- Benchmarks still required: measure per-login CPU time, RSS, syscall counts and p95/p99 authorization latency under simultaneous password, publickey and ECDSA-SK SSH sessions. Any new I/O or significant latency regression is a v1 blocker.
- Do not automatically deploy a custom sshd binary on Ubuntu hosts. Upstream security updates and rollback support must be preserved.

Research source pin was moved from 10.6p1 to the 10.6p2 security-fixed stable branch; the deployed Ubuntu package remains untouched. This does **not** establish distribution patch compatibility or production suitability.
