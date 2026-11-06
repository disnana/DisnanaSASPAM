"""Bilingual documentation completeness and local link regression tests.

Runs offline with the normal Python unittest job; no documentation build tools
or external network access are required.
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
TOPICS = ("installation", "architecture", "recovery", "security", "updates")
MARKDOWN_LINK = re.compile(r"(?<!!)\[[^\]]+\]\(([^)]+)\)")


class BilingualDocumentationTests(unittest.TestCase):
    def test_japanese_and_english_readmes_exist(self):
        japanese = (ROOT / "README.md").read_text(encoding="utf-8")
        english = (ROOT / "README.en.md").read_text(encoding="utf-8")
        self.assertIn("[English](README.en.md)", japanese)
        self.assertIn("[**日本語**](README.md)", english)
        self.assertIn("v0.1.0-alpha.4", japanese)
        self.assertIn("v0.1.0-alpha.4", english)

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
        markdown_files = [
            ROOT / "README.md",
            ROOT / "README.en.md",
            ROOT / "docs" / "README.md",
        ]
        for topic in TOPICS:
            markdown_files.extend([
                ROOT / "docs" / (topic + ".md"),
                ROOT / "docs" / "en" / (topic + ".md"),
            ])
        for source in markdown_files:
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
