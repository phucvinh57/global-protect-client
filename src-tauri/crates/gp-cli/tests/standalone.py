"""Isolated CLI acceptance tests; no VPN, root, desktop, or keyring required."""
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import termios
import time
import unittest

BINARY = str(Path(sys.argv.pop(1)).resolve())
FIXTURE = Path(__file__).with_name("helper.py")


class Standalone(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="gp-cli-test-")
        self.root = Path(self.temp.name)
        self.runtime = self.root / "runtime"
        self.runtime.mkdir(mode=0o700)
        self.socket = self.runtime / "gp-client/control.sock"
        self.cleanup = self.root / "cleanup"
        self.helper = self.root / "gp-helper"
        shutil.copyfile(FIXTURE, self.helper)
        self.helper.chmod(0o700)
        sudo = self.root / "sudo"
        sudo.write_text('#!/bin/sh\nif [ "$1" = "-v" ]; then exit 0; fi\nshift\nif [ "$1" = "-v" ]; then exit 0; fi\nshift\nexec "$@"\n')
        sudo.chmod(0o700)
        self.env = dict(os.environ, XDG_CONFIG_HOME=str(self.root / "config"), XDG_RUNTIME_DIR=str(self.runtime),
                        PATH=str(self.root) + ":" + os.environ["PATH"], GP_HELPER_PATH=str(self.helper),
                        GP_TEST_CLEANUP=str(self.cleanup), DBUS_SESSION_BUS_ADDRESS="unix:path=/nonexistent-gp-cli-test-bus")
        self.children = []
        self.masters = []

    def tearDown(self):
        try:
            self.cli("disconnect")
        except Exception:
            pass
        for child in self.children:
            if child.stdin:
                child.stdin.close()
            if child.poll() is None:
                child.terminate()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
            if child.stdout:
                child.stdout.close()
            if child.stderr:
                child.stderr.close()
        for master in self.masters:
            os.close(master)
        self.temp.cleanup()

    def cli(self, *args, ok=True):
        result = subprocess.run([BINARY, *args], env=self.env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=10)
        if ok:
            self.assertEqual(result.returncode, 0, result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout)
        return result

    def profile(self, *extra):
        return self.cli("profile", "add", "Work", "--portal", "https://vpn.example.test", "--username", "alice", *extra).stdout.strip()

    def controller(self, mode="success"):
        env = dict(self.env, GP_TEST_MODE=mode)
        child = subprocess.Popen([BINARY, "__controller"], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.children.append(child)
        try:
            self.answer(child, {"helper": str(self.helper), "profile_id": "test", "profile_name": "Work",
                                "request": {"cmd": "connect", "portal": "https://vpn.example.test", "username": "alice", "password": "synthetic-secret"}})
        except BrokenPipeError:
            pass  # A competing controller can reject startup before this write.
        return child

    def answer(self, child, value):
        child.stdin.write(json.dumps(value) + "\n")
        child.stdin.flush()

    def event(self, child):
        data = getattr(child, "_test_buffer", b"")
        deadline = time.monotonic() + 5
        while b"\n" not in data:
            remaining = deadline - time.monotonic()
            self.assertGreater(remaining, 0, "Timed out waiting for controller event")
            self.assertTrue(select.select([child.stdout], [], [], remaining)[0])
            chunk = os.read(child.stdout.fileno(), 4096)
            self.assertTrue(chunk, "controller EOF")
            data += chunk
        line, child._test_buffer = data.split(b"\n", 1)
        return json.loads(line)

    def until_event(self, child, name):
        for _ in range(10):
            event = self.event(child)
            if event["ev"] == name:
                return event
        self.fail("Expected event " + name)

    def gone(self):
        deadline = time.monotonic() + 5
        while self.socket.exists() and time.monotonic() < deadline:
            time.sleep(0.02)
        self.assertFalse(self.socket.exists(), "controller socket not removed")

    def test_profile_crud_all_options_and_ambiguity(self):
        profile = self.profile("--cafile", "/test/ca", "--client-cert", "/test/cert", "--client-key", "/test/key",
                               "--trusted-fingerprint", "pin-sha256:test", "--gateway", "gateway.test", "--client-os", "windows",
                               "--script", "/test/script", "--hip", "--requires-otp")
        p = json.loads(self.cli("profile", "show", profile).stdout)
        self.assertEqual(p, dict(id=profile, name="Work", portal="https://vpn.example.test", username="alice", remember=False,
                                 cafile="/test/ca", clientCert="/test/cert", clientKey="/test/key", trustedFingerprint="pin-sha256:test",
                                 gateway="gateway.test", clientOs="windows", script="/test/script", hip=True, requiresOtp=True))
        second = self.profile()
        ambiguous = self.cli("profile", "show", "Work", ok=False).stderr
        self.assertIn(profile, ambiguous)
        self.assertIn(second, ambiguous)
        self.cli("profile", "edit", profile, "--name", "Office", "--hip=false", "--requires-otp=false", "--cafile", "")
        edited = json.loads(self.cli("profile", "show", "Office").stdout)
        self.assertIsNone(edited["cafile"])
        self.assertFalse(edited["hip"])
        self.cli("profile", "delete", profile)
        self.assertNotIn(profile, self.cli("profile", "list").stdout)

    def test_noninteractive_credentials_and_fields(self):
        self.assertIn("interactive terminal", self.cli("profile", "add", "Missing", ok=False).stderr)
        self.profile()
        self.assertIn("Password requires an interactive terminal", self.cli("connect", "Work", ok=False).stderr)
        self.assertIn("unexpected argument", self.cli("connect", "Work", "--password", "unused", ok=False).stderr)

    def test_detached_status_disconnect_waits_for_cleanup(self):
        child = self.controller()
        self.until_event(child, "connected")
        child.stdin.close()
        child.stdin = None
        child.stdout.close()
        self.assertEqual(json.loads(self.cli("status").stdout)["state"], "connected")
        self.assertEqual(self.socket.stat().st_mode & 0o777, 0o600)
        self.cli("disconnect")
        self.assertTrue(self.cleanup.exists())
        self.gone()
        child.wait(timeout=5)
        self.cli("disconnect")

    def test_prompt_relay_and_cancel(self):
        child = self.controller("prompts")
        self.until_event(child, "cert_untrusted")
        self.answer(child, {"cmd": "trust_cert"})
        self.until_event(child, "mfa_challenge")
        self.answer(child, {"cmd": "submit_otp", "otp": "synthetic-token"})
        self.until_event(child, "gateways")
        self.answer(child, {"cmd": "select_gateway", "address": "gateway.test"})
        self.until_event(child, "connected")
        self.cli("disconnect")
        self.assertTrue(self.cleanup.exists())
        child.wait(timeout=5)  # initiating pipe stays open: shutdown must still finish

    def test_terminal_disappears_before_connected(self):
        child = self.controller("pending")
        self.until_event(child, "mfa_challenge")
        child.stdin.close()
        child.stdin = None
        child.wait(timeout=5)
        self.assertTrue(self.cleanup.exists())
        self.gone()

    def test_disconnect_during_authentication(self):
        child = self.controller("pending")
        self.until_event(child, "mfa_challenge")
        self.cli("disconnect")
        self.assertTrue(self.cleanup.exists())
        child.wait(timeout=5)
        self.gone()

    def test_helper_failure(self):
        child = self.controller("failure")
        self.assertIn("Simulated", self.until_event(child, "error")["msg"])
        child.wait(timeout=5)
        self.assertNotEqual(child.returncode, 0)
        self.gone()

    def test_helper_crash(self):
        child = self.controller("crash")
        child.wait(timeout=5)
        self.assertNotEqual(child.returncode, 0)
        self.gone()

    def test_unexpected_tunnel_exit(self):
        child = self.controller("exit")
        self.until_event(child, "connected")
        child.wait(timeout=5)
        self.gone()
        self.assertEqual(json.loads(self.cli("status").stdout)["state"], "disconnected")

    def test_concurrent_start(self):
        first = self.controller("pending")
        self.until_event(first, "mfa_challenge")
        second = self.controller("pending")
        self.assertIn("Another CLI", self.until_event(second, "error")["msg"])
        second.wait(timeout=5)
        self.assertTrue(self.socket.exists())

    def test_stale_socket_and_symlink_rejection(self):
        self.socket.parent.mkdir(mode=0o700)
        sock = socket.socket(socket.AF_UNIX)
        sock.bind(str(self.socket))
        self.socket.chmod(0o600)
        sock.close()
        self.assertEqual(json.loads(self.cli("status").stdout)["state"], "disconnected")
        self.assertFalse(self.socket.exists())
        self.socket.symlink_to(self.helper)
        self.assertIn("Unsafe control socket", self.cli("status", ok=False).stderr)
        self.socket.unlink()

    def terminal_connect(self, mode="success", extra=()):
        self.profile(*extra)
        return self.terminal_command(["connect", "Work"], mode)

    def terminal_command(self, args, mode="success"):
        master, slave = pty.openpty()
        self.masters.append(master)
        def session():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)
        child = subprocess.Popen([BINARY, *args], env=dict(self.env, GP_TEST_MODE=mode), stdin=slave, stdout=slave, stderr=slave, preexec_fn=session)
        os.close(slave)
        self.children.append(child)
        return child, master

    def expect(self, master, wanted):
        data = b""
        deadline = time.monotonic() + 8
        while wanted.encode() not in data and time.monotonic() < deadline:
            if select.select([master], [], [], 0.1)[0]:
                try:
                    data += os.read(master, 4096)
                except OSError:
                    break
        self.assertIn(wanted.encode(), data)
        return data

    def test_connect_terminal_closure_and_secret_hiding(self):
        child, master = self.terminal_connect(extra=("--requires-otp",))
        self.expect(master, "Password:")
        os.write(master, b"synthetic-secret\n")
        output = self.expect(master, "One-time passcode:")
        self.assertNotIn(b"synthetic-secret", output)
        os.write(master, b"synthetic-token\n")
        output = self.expect(master, "Connected:")
        self.assertNotIn(b"synthetic-token", output)
        self.assertNotIn(b"synthetic-secret", output)
        child.wait(timeout=5)
        self.assertEqual(child.returncode, 0)
        os.close(master)
        self.masters.remove(master)
        self.assertEqual(json.loads(self.cli("status").stdout)["state"], "connected")
        self.cli("disconnect")
        self.assertTrue(self.cleanup.exists())

    def test_certificate_rejection_and_cancelled_prompt_fail(self):
        child, master = self.terminal_connect("certificate")
        self.expect(master, "Password:")
        os.write(master, b"synthetic-secret\n")
        self.expect(master, "Trust this certificate")
        os.write(master, b"no\n")
        child.wait(timeout=5)
        self.assertNotEqual(child.returncode, 0)
        self.assertTrue(self.cleanup.exists())
        self.gone()

    def test_interactive_certificate_mfa_and_gateway(self):
        child, master = self.terminal_connect("prompts")
        self.expect(master, "Password:")
        os.write(master, b"synthetic-secret\n")
        self.expect(master, "Trust this certificate")
        os.write(master, b"yes\n")
        self.expect(master, "One-time passcode:")
        os.write(master, b"synthetic-token\n")
        self.expect(master, "Gateway number:")
        os.write(master, b"1\n")
        self.expect(master, "Connected:")
        child.wait(timeout=5)
        self.assertEqual(child.returncode, 0)
        self.cli("disconnect")

    def test_closing_terminal_during_mfa_cleans_up(self):
        child, master = self.terminal_connect("mfa")
        self.expect(master, "Password:")
        os.write(master, b"synthetic-secret\n")
        self.expect(master, "One-time passcode:")
        os.close(master)
        self.masters.remove(master)
        child.wait(timeout=5)
        self.gone()
        self.assertTrue(self.cleanup.exists())

    def test_connect_reports_helper_failure(self):
        child, master = self.terminal_connect("failure")
        self.expect(master, "Password:")
        os.write(master, b"synthetic-secret\n")
        self.expect(master, "Simulated authentication failure")
        child.wait(timeout=5)
        self.assertNotEqual(child.returncode, 0)
        self.gone()

    def test_remember_option_requires_consent(self):
        child, master = self.terminal_command(["profile", "add", "Remembered", "--portal", "https://vpn.example.test", "--username", "alice", "--remember"])
        self.expect(master, "Enable remembering passwords")
        os.write(master, b"yes\n")
        child.wait(timeout=5)
        self.assertEqual(child.returncode, 0)
        self.assertTrue(json.loads(self.cli("profile", "show", "Remembered").stdout)["remember"])
        self.cli("profile", "edit", "Remembered", "--remember=false")
        self.assertFalse(json.loads(self.cli("profile", "show", "Remembered").stdout)["remember"])

    def test_unsafe_runtime_directory_is_rejected(self):
        self.runtime.chmod(0o755)
        try:
            self.assertIn("private", self.cli("status", ok=False).stderr)
        finally:
            self.runtime.chmod(0o700)

    def test_hidden_prompt_ctrl_c(self):
        child, master = self.terminal_connect()
        self.expect(master, "Password:")
        os.write(master, b"\x03")
        child.wait(timeout=5)
        self.assertNotEqual(child.returncode, 0)
        self.assertTrue(termios.tcgetattr(master)[3] & termios.ECHO)


if __name__ == "__main__":
    signal.alarm(120)
    unittest.main(verbosity=2)
