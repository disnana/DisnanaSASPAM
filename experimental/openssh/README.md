# Experimental OpenSSH-to-PAM authentication metadata adapter

**Not supported for production or included in any stable SASPAM Release.**

This research patch targets upstream OpenSSH portable tag `V_10_6_P1` (not the Ubuntu distribution's security-patched OpenSSH). It does not replace system `sshd`, modify existing PAM files, or run automatically on user hosts.

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
