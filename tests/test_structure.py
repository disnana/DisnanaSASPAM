"""Static scope checks. These do not replace PAM integration tests."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


class StructureTests(unittest.TestCase):
    def test_account_hook_is_after_primary_authentication(self):
        pam = (ROOT / "crates/saspam-pam/src/lib.rs").read_text()
        self.assertIn("pam_sm_acct_mgmt", pam)
        self.assertIn('service != "sshd"', pam)
        self.assertNotIn("pam_sm_authenticate(", pam)

    def test_pam_hook_never_installed_into_common_account_or_console(self):
        cli = (ROOT / "crates/saspam-cli/src/main.rs").read_text()
        self.assertIn('const PAM_FILE: &str = "/etc/pam.d/sshd"', cli)
        self.assertNotIn('"/etc/pam.d/common-account"', cli)
        self.assertNotIn('"/etc/pam.d/login"', cli)

    def test_installer_does_not_run_saspam_enable(self):
        script = (ROOT / "scripts/install.sh").read_text()
        self.assertNotIn("sudo saspam enable --ack-recovery --ack-tested\nsystemctl", script)
        self.assertIn("SHA256SUMS", script)
        self.assertIn("sha256sum -c -", script)

    def test_ssh_hardening_does_not_restrict_config_dir(self):
        cli = (ROOT / "crates/saspam-cli/src/main.rs").read_text()
        self.assertIn("write_hardening(dest, HARDENING)", cli)
        self.assertNotIn("write_private(dest, HARDENING)", cli)

    def test_venv_exec_permissions_in_both_installers(self):
        for path in ("scripts/install.sh", "scripts/install-from-build.sh"):
            data = (ROOT / path).read_text()
            with self.subTest(path=path):
                self.assertIn("chown -R root:saspam-bot /opt/saspam/venv", data)
                self.assertIn("chmod -R u=rwX,g=rX,o= /opt/saspam/venv", data)
                self.assertIn("runuser -u saspam-bot -- /opt/saspam/venv/bin/python -c", data)
                self.assertNotIn("chmod -R a+rX /etc/saspam", data)

    def test_safe_update_never_disables_pam(self):
        upgrade = (ROOT / "scripts/update.sh").read_text()
        rollback = (ROOT / "scripts/rollback-upgrade.sh").read_text()
        self.assertNotIn("saspam disable", upgrade)
        self.assertNotIn("saspam disable", rollback)
        self.assertIn("systemd-run --unit=", upgrade)
        self.assertIn("trap on_exit EXIT", upgrade)
        self.assertIn("flock -w", rollback)
        self.assertIn("--block-new-ssh", upgrade)
        self.assertIn("refusing unsafe firewall changes", upgrade)

    def test_updater_contains_pam_preserving_recovery(self):
        upgrade=(ROOT / "scripts/update.sh").read_text()
        recovery=(ROOT / "scripts/rollback-upgrade.sh").read_text()
        self.assertIn("systemd-run --unit=", upgrade)
        self.assertIn("flock -n 8", upgrade)
        self.assertIn("PAM_GATED=0", upgrade)
        self.assertIn("systemctl stop saspam-discord.service saspamd.service", upgrade)
        self.assertNotIn("sudo saspam disable", upgrade)
        self.assertNotIn("saspam disable", recovery)
        self.assertIn("rolling-back:*", recovery)

    def test_release_has_update_recovery_and_published_tests(self):
        yml=(ROOT / ".github/workflows/release.yml").read_text()
        self.assertIn("update.sh rollback-upgrade.sh",yml)
        self.assertIn("published_upgrade.sh",yml)
        self.assertTrue((ROOT / "docs/updates.md").exists())

    def test_bot_runs_without_root(self):
        unit = (ROOT / "systemd/saspam-discord.service").read_text()
        self.assertIn("User=saspam-bot", unit)
        self.assertIn("NoNewPrivileges=yes", unit)


if __name__ == "__main__":
    unittest.main()
