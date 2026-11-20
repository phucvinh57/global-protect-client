#!/usr/bin/env python3
"""Simulated JSON-lines helper. Never modifies networking or invokes sudo."""
import json
import os
import sys
import time
from pathlib import Path


def emit(**event):
    print(json.dumps(event), flush=True)


def read():
    line = sys.stdin.readline()
    return json.loads(line) if line else {"cmd": "disconnect"}


if "--detach" in sys.argv:
    if os.fork():
        os._exit(0)
    os.setsid()

mode = os.environ.get("GP_TEST_MODE", "success")
request = read()
assert request["cmd"] == "connect"
emit(ev="log", level="trace", msg="test diagnostic " + request.get("password", ""))
if mode == "crash":
    sys.exit(3)
if mode == "failure":
    emit(ev="error", msg="Simulated authentication failure")
else:
    emit(ev="state", state="authenticating")
    questions = []
    if mode in ("certificate", "prompts"):
        questions.append((dict(ev="cert_untrusted", fingerprint="pin-sha256:test", details="Test certificate"), "trust_cert"))
    if mode in ("mfa", "prompts", "pending"):
        questions.append((dict(ev="mfa_challenge", message="Enter the test token"), "submit_otp"))
    if mode == "prompts":
        questions.append((dict(ev="gateways", selecting=True, list=[dict(name="Test gateway", address="gateway.test")]), "select_gateway"))
    for question, expected in questions:
        emit(**question)
        answer = read()
        if answer["cmd"] == "disconnect":
            break
        assert answer["cmd"] == expected
    else:
        emit(ev="connected", ifname="test0", addr="192.0.2.1", dns=["192.0.2.53"], gateway="Test gateway")
        emit(ev="state", state="connected")
        if mode == "exit":
            time.sleep(0.25)
        else:
            assert read()["cmd"] == "disconnect"
    # Simulate slow route/DNS cleanup. Tests assert the acknowledgment waits.
    time.sleep(0.15)
Path(os.environ["GP_TEST_CLEANUP"]).touch()
emit(ev="state", state="disconnected")
