#!/usr/bin/env python3
"""Use a genuine Linux PTY to exercise the published SASPAM interactive CLI."""
import argparse
import errno
import os
import pty
import re
import select
import subprocess
import sys
import time
import unittest

BINARY = None


class MenuPtyTests(unittest.TestCase):
    def open_menu(self):
        master, slave = pty.openpty()
        env = {**os.environ, "TERM": "xterm-256color"}
        process = subprocess.Popen(
            [BINARY, "menu"], stdin=slave, stdout=slave, stderr=slave,
            env=env, start_new_session=True, close_fds=True,
        )
        os.close(slave)
        self.addCleanup(self.cleanup_process, process, master)
        self.read_until(master, b"Back", wait=5, alternative="戻る".encode())
        self.assertIsNone(process.poll(), "SASPAM exited before the first keypress")
        return process, master

    @staticmethod
    def cleanup_process(process, master):
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)
        os.close(master)

    @staticmethod
    def read_until(master, marker: bytes, wait=5, alternative=None):
        received = bytearray()
        deadline = time.monotonic() + wait
        while time.monotonic() < deadline:
            ready, _, _ = select.select([master], [], [], min(0.15, max(0, deadline - time.monotonic())))
            if not ready:
                continue
            try:
                chunk = os.read(master, 32768)
            except OSError as ex:
                if ex.errno == errno.EIO:
                    break
                raise
            if not chunk:
                break
            received.extend(chunk)
            if marker in received or (alternative is not None and alternative in received):
                return bytes(received)
        raise AssertionError(f"Expected PTY output {marker!r}, got tail={bytes(received[-500:])!r}")

    @staticmethod
    def send(master, keys):
        os.write(master, keys)

    def expect_selection(self, master, n):
        # Cyan highlighted item uses the ❯ marker.
        frame = self.read_until(master, "❯".encode() + f" {n}".encode(), wait=4)
        self.assertIn("❯".encode(), frame)

    def test_arrows_in_ansi_csi_and_ss3_do_not_exit(self):
        process, master = self.open_menu()
        self.send(master, b"\x1b[B")
        self.expect_selection(master, 2)
        self.assertIsNone(process.poll())
        self.send(master, b"\x1b[A")
        self.expect_selection(master, 1)
        self.send(master, b"\x1bOB")
        self.expect_selection(master, 2)
        self.send(master, b"\x1b[1;5A")
        self.expect_selection(master, 1)
        self.send(master, b"10\r")
        self.assertEqual(process.wait(timeout=5), 0)

    def test_invalid_multidigit_numbers_do_not_execute_any_item(self):
        process, master = self.open_menu()
        self.send(master, b"12\r")
        self.read_until(master, "番号が範囲外".encode(), alternative=b"Number out of range")
        self.assertIsNone(process.poll(), "'12' incorrectly executed a command or quit")
        self.send(master, b"\x7f\x7f111\r")
        self.read_until(master, "番号が範囲外".encode(), alternative=b"Number out of range")
        self.assertIsNone(process.poll(), "'111' incorrectly executed a command or quit")
        self.send(master, b"\x7f\x7f\x7f10\r")
        self.assertEqual(process.wait(timeout=5), 0)

    def test_incomplete_escape_sequence_does_not_close_menu(self):
        process, master = self.open_menu()
        self.send(master, b"\x1b[")
        time.sleep(0.55)
        self.assertIsNone(process.poll(), "Incomplete CSI incorrectly treated as Esc")
        self.send(master, b"\x1bXB")  # Unexpected Alt+X + B is not Back.
        time.sleep(0.1)
        self.assertIsNone(process.poll())
        self.send(master, b"10\r")
        self.assertEqual(process.wait(timeout=5), 0)

    def test_plain_escape_and_zero_back_close_menu(self):
        process, master = self.open_menu()
        self.send(master, b"\x1b")
        self.assertEqual(process.wait(timeout=5), 0)
        process2, master2 = self.open_menu()
        self.send(master2, b"0\r")
        self.assertEqual(process2.wait(timeout=5), 0)

    def test_non_terminal_numbers_reprompt_on_invalid_input(self):
        proc = subprocess.run(
            [BINARY, "menu"], input="12\n111\n10\n",
            capture_output=True, text=True, timeout=5,
        )
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(proc.stderr.count("範囲外") + proc.stderr.count("Invalid number"), 2)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", help="path to built saspam executable")
    args = parser.parse_args()
    BINARY = os.path.abspath(args.binary)
    if not os.path.isfile(BINARY):
        parser.error("saspam binary not found")
    unittest.main(argv=[sys.argv[0]], verbosity=2)
