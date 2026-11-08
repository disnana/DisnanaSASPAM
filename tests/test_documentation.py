"""Bilingual documentation completeness and local link regression tests.

Runs offline with the normal Python unittest job; no documentation build tools
or external network access are required.
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
VERSION = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
TOPICS = ("installation", "architecture", "recovery", "security", "updates")
MARKDOWN_LINK = re.compile(r"(?<!!)\[[^\]]+\]\(([^)]+)\)")
README_FILES = (ROOT / "README.md", ROOT / "README.en.md")
INSTALL_FILES = (ROOT / "docs/installation.md", ROOT / "docs/en/installation.md")
UPDATE_FILES = (ROOT / "docs/updates.md", ROOT / "docs/en/updates.md")
MARKDOWN_FILES = (*README_FILES, *sorted((ROOT / "docs").rglob("*.md")))
VERSION_ARGUMENT = re.compile(r"--version\s+(v\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?)")


class BilingualDocumentationTests(unittest.TestCase):
    def test_japanese_and_english_readmes_exist(self):
        japanese = (ROOT / "README.md").read_text(encoding="utf-8")
        english = (ROOT / "README.en.md").read_text(encoding="utf-8")
        self.assertIn("[English](README.en.md)", japanese)
        self.assertIn("[**日本語**](README.md)", english)
        self.assertIn(VERSION, japanese)
        self.assertIn(VERSION, english)

    def test_current_release_is_named_on_entry_pages(self):
        for path in (*README_FILES, *INSTALL_FILES, *UPDATE_FILES, ROOT / "docs/README.md"):
            with self.subTest(path=str(path.relative_to(ROOT))):
                self.assertIn(VERSION, path.read_text(encoding="utf-8"))

    def test_install_commands_pin_the_current_release(self):
        for path in (*README_FILES, *INSTALL_FILES):
            text = path.read_text(encoding="utf-8")
            commands = [line for line in text.splitlines()
                        if "curl " in line and "/scripts/install.sh" in line]
            with self.subTest(path=str(path.relative_to(ROOT))):
                self.assertTrue(commands, "missing copyable installer command")
                for command in commands:
                    self.assertEqual(VERSION_ARGUMENT.findall(command), [VERSION])
                    self.assertIn("sudo bash -s -- --version", command)

    def test_no_pinned_command_silently_selects_an_old_release(self):
        for path in MARKDOWN_FILES:
            text = path.read_text(encoding="utf-8")
            with self.subTest(path=str(path.relative_to(ROOT))):
                for tag in VERSION_ARGUMENT.findall(text):
                    self.assertEqual(tag, VERSION,
                                     "current guidance must not install/update to a legacy tag")
        audit = (ROOT / "docs/ssh-auth-audit.md").read_text(encoding="utf-8")
        self.assertEqual(re.findall(r"^TAG=(\S+)$", audit, re.MULTILINE), [VERSION])

    def test_no_obsolete_unreleased_status(self):
        obsolete = ("There is no stable Release yet", "正式安定版のReleaseはまだない",
                    "Current developer preview:", "currently published alpha",
                    "before stable release", "before calling this stable",
                    "remain needed for a stable release", "安定版までの課題")
        for path in MARKDOWN_FILES:
            text = path.read_text(encoding="utf-8")
            for phrase in obsolete:
                with self.subTest(path=str(path.relative_to(ROOT)), phrase=phrase):
                    self.assertNotIn(phrase, text)

    def test_latest_update_documents_prerelease_selection(self):
        for path in (README_FILES[0], UPDATE_FILES[0]):
            text = path.read_text(encoding="utf-8")
            self.assertRegex(text, r"`--latest`[^。\n]*試験版も含め")
        for path in (README_FILES[1], UPDATE_FILES[1]):
            text = path.read_text(encoding="utf-8")
            self.assertIn("`--latest` includes prereleases", text)

    def test_installer_default_is_latest_stable(self):
        for path in (README_FILES[0], INSTALL_FILES[0]):
            text = path.read_text(encoding="utf-8")
            self.assertIn("`--version`を省略するとGitHubの最新の正式Release", text)
        for path in (README_FILES[1], INSTALL_FILES[1]):
            text = path.read_text(encoding="utf-8")
            self.assertIn("Omitting `--version` selects GitHub's latest stable Release", text)

    def test_legacy_migrations_restart_services_via_setup(self):
        ja = UPDATE_FILES[0].read_text(encoding="utf-8")
        en = UPDATE_FILES[1].read_text(encoding="utf-8")
        self.assertIn("すべての旧版移行で`sudo saspam setup`を実行", ja)
        self.assertIn("Run `sudo saspam setup` for every legacy migration", en)
        self.assertIn("インストーラーだけでは稼働中の旧プロセスを再起動しません", ja)
        self.assertIn("The installer alone does not restart running old processes", en)

    def test_install_guides_describe_default_hidden_auth_methods(self):
        ja = INSTALL_FILES[0].read_text(encoding="utf-8")
        en = INSTALL_FILES[1].read_text(encoding="utf-8")
        self.assertIn("標準設定では認証方式欄を表示しません", ja)
        self.assertIn("Hidden by default", en)
        self.assertIn("実験モードでも「不明」", ja)
        self.assertIn("Unknown in experimental mode", en)
        self.assertIn("OpenSSHのパッチ適用・ビルド・置換を行いません", ja)
        self.assertIn("does not patch, build or replace OpenSSH", en)

    def test_release_and_install_guides_retain_safety_warnings(self):
        for path in (README_FILES[0], INSTALL_FILES[0], UPDATE_FILES[0]):
            text = path.read_text(encoding="utf-8")
            with self.subTest(path=str(path.relative_to(ROOT))):
                self.assertIn("監査", text)
                self.assertIn("未完了", text)
                self.assertRegex(text, r"本番SSH[^。\n]*(?:推奨しません|導入しないでください)")
                self.assertIn("コンソール", text)
        for path in (README_FILES[1], INSTALL_FILES[1], UPDATE_FILES[1]):
            text = path.read_text(encoding="utf-8")
            with self.subTest(path=str(path.relative_to(ROOT))):
                self.assertIn("not recommended for production SSH servers", text)
                self.assertRegex(text.lower(), r"not (?:passed an independent security audit|independently (?:security )?audited)")
                self.assertIn("console", text)

    def test_all_five_topics_have_language_switches(self):
        for topic in TOPICS:
            with self.subTest(topic=topic):
                ja_file = ROOT / "docs" / (topic + ".md")
                en_file = ROOT / "docs" / "en" / (topic + ".md")
                self.assertTrue(ja_file.is_file(), str(ja_file))
                self.assertTrue(en_file.is_file(), str(en_file))
                ja = ja_file.read_text(encoding="utf-8")
                en = en_file.read_text(encoding="utf-8")
                self.assertIn(f"[English](en/{topic}.md)", ja)
                self.assertIn(f"[日本語](../{topic}.md)", en)
                self.assertIn("[Documentation index](../README.md)", en)
                self.assertGreater(len(en), 1000)

    def test_all_local_markdown_links_resolve(self):
        for source in MARKDOWN_FILES:
            text = source.read_text(encoding="utf-8")
            for link in MARKDOWN_LINK.findall(text):
                destination = link.split("#", 1)[0]
                if not destination or destination.startswith(("http://", "https://", "mailto:")):
                    continue
                if "%" in destination or "?" in destination:
                    continue  # No encoded/parameterized local paths in docs.
                with self.subTest(file=str(source.relative_to(ROOT)), link=link):
                    resolved = (source.parent / destination).resolve()
                    self.assertTrue(resolved.is_file(), f"broken local link in {source}: {link}")

    def test_install_and_recovery_warnings_exist_in_both_languages(self):
        ja = (ROOT / "README.md").read_text(encoding="utf-8")
        en = (ROOT / "README.en.md").read_text(encoding="utf-8")
        for command in ("sudo saspam test", "sudo saspam disable",
                        "sudo saspam config", "sudo saspam notify test ntfy",
                        "sudo saspam update --rollback"):
            with self.subTest(command=command):
                self.assertIn(command, ja)
                self.assertIn(command, en)
        self.assertIn("Fail Closed", ja)
        self.assertIn("Fail Closed", en)
        self.assertIn("--block-new-ssh", ja)
        self.assertIn("--block-new-ssh", en)


if __name__ == "__main__":
    unittest.main()
