#!/usr/bin/env python3
"""Fake Discord transport for real SSH account-stage integration tests.

Runs only in GitHub CI. A real SSH/PAM/daemon request must exist before voting.
"""
import argparse
import json
import socket
import sys
import time

SOCKET = "/run/saspam/bot.sock"

def call(data):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as conn:
        conn.settimeout(3)
        conn.connect(SOCKET)
        conn.sendall((json.dumps({"version": 2, **data}) + "\n").encode())
        chunks = bytearray()
        while b"\n" not in chunks:
            part = conn.recv(4096)
            if not part:
                raise RuntimeError("daemon closed")
            chunks.extend(part)
            if len(chunks) > 65536:
                raise ValueError("large IPC reply")
    return json.loads(chunks.split(b"\n", 1)[0])

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("decision", choices=("approve", "deny"))
    parser.add_argument("--timeout", type=float, default=15)
    parser.add_argument("--user", default="saspam-ci")
    parser.add_argument("--valid-auth", help="comma-separated allowed authentication categories")
    args = parser.parse_args()
    deadline = time.monotonic() + args.timeout
    while time.monotonic() < deadline:
        reply = call({"action": "list"})
        assert reply["ok"], reply
        matches = [r for r in reply["requests"] if r["user"] == args.user and r["status"] == "pending"]
        if matches:
            row = max(matches, key=lambda r: r["created_unix"])
            if args.valid_auth:
                observed = row.get("auth_methods") or ["unknown"]
                allowed = set(args.valid_auth.split(","))
                if any(method not in allowed for method in observed):
                    print(f"Unexpected auth metadata: {observed}", file=sys.stderr)
                    return 1
            result = call({"action": "decide", "request_id": row["request_id"],
                           "decision": args.decision, "actor_id": "123456789012345678",
                           "guild_id": "111111111111111111", "channel_id": "222222222222222222",
                           "role_ids": ["444444444444444444"]})
            if not result["ok"]:
                print("Could not decide:", result, file=sys.stderr)
                return 1
            # Same request cannot be decided twice (replay prevention).
            again = call({"action": "decide", "request_id": row["request_id"],
                          "decision": "approve", "actor_id": "123456789012345678",
                          "guild_id": "111111111111111111", "channel_id": "222222222222222222",
                          "role_ids": ["333333333333333333"]})
            assert not again["ok"], "double decision unexpectedly accepted"
            print("decision="+args.decision+" request="+row["request_id"])
            return 0
        time.sleep(0.15)
    print("Never received a post-auth PAM request", file=sys.stderr)
    return 1

if __name__ == "__main__":
    raise SystemExit(main())
