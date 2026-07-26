#!/usr/bin/env python3
"""Exercise real QUIC, MLS, files and linked-device fan-out on loopback."""

from __future__ import annotations

import argparse
import json
import queue
import subprocess
import tempfile
import threading
import time
from pathlib import Path


class Peer:
    def __init__(self, binary: Path, profile: Path) -> None:
        self.process = subprocess.Popen(
            [str(binary), "daemon", "--profile", str(profile)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self.events: queue.Queue[dict] = queue.Queue()
        self.pending: list[dict] = []
        self.reader = threading.Thread(target=self._read_events, daemon=True)
        self.reader.start()

    def _read_events(self) -> None:
        assert self.process.stdout is not None
        for line in self.process.stdout:
            try:
                self.events.put(json.loads(line))
            except json.JSONDecodeError:
                continue

    def send(self, command: dict[str, object]) -> None:
        assert self.process.stdin is not None
        self.process.stdin.write(json.dumps(command) + "\n")
        self.process.stdin.flush()

    def event(self, name: str, predicate=lambda _event: True, timeout: float = 30) -> dict:
        for index, value in enumerate(self.pending):
            if value.get("event") == name and predicate(value):
                return self.pending.pop(index)
        deadline = time.monotonic() + timeout
        seen: list[str | None] = []
        while time.monotonic() < deadline:
            try:
                value = self.events.get(timeout=max(0, deadline - time.monotonic()))
            except queue.Empty:
                break
            seen.append(value.get("event"))
            if value.get("event") == name and predicate(value):
                return value
            self.pending.append(value)
        stderr = ""
        if self.process.poll() is not None and self.process.stderr is not None:
            stderr = self.process.stderr.read()
        raise RuntimeError(f"timeout waiting for {name}; seen={seen}; stderr={stderr}")

    def close(self) -> None:
        if self.process.poll() is None:
            try:
                self.send({"command": "shutdown"})
                self.process.communicate(timeout=8)
                return
            except (BrokenPipeError, subprocess.TimeoutExpired):
                self.process.kill()
        self.process.communicate()

    def assert_no_event(self, name: str, predicate=lambda _event: True, duration: float = 4) -> None:
        for value in self.pending:
            if value.get("event") == name and predicate(value):
                raise RuntimeError(f"unexpected queued {name} after revocation: {value}")
        deadline = time.monotonic() + duration
        while time.monotonic() < deadline:
            try:
                value = self.events.get(timeout=max(0, deadline - time.monotonic()))
            except queue.Empty:
                return
            if value.get("event") == name and predicate(value):
                raise RuntimeError(f"unexpected {name} after revocation: {value}")
            self.pending.append(value)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, default=Path("target/debug/pptalk-cli"))
    args = parser.parse_args()
    binary = args.binary.resolve()
    repository = Path(__file__).resolve().parent.parent
    peers: list[Peer] = []
    with tempfile.TemporaryDirectory(prefix="pptalk-e2e-") as temporary:
        root = Path(temporary)
        alice_profile = root / "alice.json"
        bob_profile = root / "bob.json"
        laptop_profile = root / "alice-laptop.json"
        for profile, name in ((alice_profile, "Alice"), (bob_profile, "Bob")):
            subprocess.run(
                [str(binary), "init", "--profile", str(profile), "--name", name],
                check=True,
                stdout=subprocess.DEVNULL,
            )
        alice, bob = Peer(binary, alice_profile), Peer(binary, bob_profile)
        peers.extend((alice, bob))
        try:
            alice.event("ready")
            bob.event("ready")
            alice.send({"command": "group_history", "group_id": "invalid"})
            alice.event("error")
            alice.send({"command": "devices"})
            alice.event("devices")
            alice.send({"command": "invite", "expires_seconds": 3600})
            invite = alice.event("invite")["url"]
            bob.send({"command": "accept", "url": invite})
            bob.send({"command": "send", "contact": "Alice", "message": "hello Alice"})
            bob.event("message_sent")
            alice.event("message", lambda event: event.get("body") == "hello Alice")
            alice.send({"command": "send", "contact": "Bob", "message": "hello Bob"})
            original_direct = alice.event("message_sent")
            bob.event("message", lambda event: event.get("body") == "hello Bob")
            bob.send(
                {
                    "command": "send",
                    "contact": "Alice",
                    "message": "reply to Bob",
                    "reply_to": original_direct["message_id"],
                }
            )
            bob.event(
                "message_sent",
                lambda event: event.get("reply_to") == original_direct["message_id"],
            )
            alice.event("message", lambda event: event.get("body") == "reply to Bob")
            alice.send(
                {
                    "command": "edit_message",
                    "contact": "Bob",
                    "message_id": original_direct["message_id"],
                    "message": "hello Bob edited",
                }
            )
            alice.event("message_edited")
            bob.event("message_edited", lambda event: event.get("body") == "hello Bob edited")

            alice.send({"command": "start_call", "contact": "Bob", "ring": False})
            silent_call = alice.event("call_started")
            bob.event(
                "call_invite",
                lambda event: event.get("call_id") == silent_call["call_id"]
                and not event.get("ring"),
            )
            bob.send(
                {
                    "command": "leave_call",
                    "contact": "Alice",
                    "call_id": silent_call["call_id"],
                }
            )
            bob.event("call_left")
            alice.event("call_leave")

            alice.send({"command": "start_call", "contact": "Bob", "ring": True})
            ringing_call = alice.event("call_started")
            bob.event(
                "call_invite",
                lambda event: event.get("call_id") == ringing_call["call_id"] and event.get("ring"),
            )
            bob.send(
                {
                    "command": "join_call",
                    "contact": "Alice",
                    "call_id": ringing_call["call_id"],
                }
            )
            bob.event("call_joined")
            alice.event("call_connected")
            bob.send(
                {
                    "command": "leave_call",
                    "contact": "Alice",
                    "call_id": ringing_call["call_id"],
                }
            )
            bob.event("call_left")
            alice.event("call_leave")

            alice.send({"command": "create_group", "name": "Partida", "members": ["Bob"]})
            groups = alice.event(
                "groups", lambda event: any(group["name"] == "Partida" for group in event["groups"])
            )
            group_id = next(group["id"] for group in groups["groups"] if group["name"] == "Partida")
            bob.event(
                "groups", lambda event: any(group["id"] == group_id for group in event["groups"])
            )
            alice.send(
                {"command": "group_send", "group_id": group_id, "message": "mutable group"}
            )
            mutable_group = alice.event(
                "group_message",
                lambda event: event.get("body") == "mutable group" and event.get("outgoing"),
            )
            bob.event("group_message", lambda event: event.get("body") == "mutable group")
            alice.send(
                {
                    "command": "group_edit_message",
                    "group_id": group_id,
                    "message_id": mutable_group["message_id"],
                    "message": "mutable group edited",
                }
            )
            alice.event("group_message_edited")
            bob.event(
                "group_message_edited",
                lambda event: event.get("body") == "mutable group edited",
            )
            alice.send(
                {
                    "command": "group_delete_message",
                    "group_id": group_id,
                    "message_id": mutable_group["message_id"],
                }
            )
            alice.event("group_message_deleted")
            bob.event("group_message_deleted")

            bob.close()
            alice.send({"command": "send", "contact": "Bob", "message": "queued direct"})
            queued = alice.event(
                "message_sent", lambda event: event.get("body") == "queued direct", timeout=40
            )
            if queued.get("delivery") != "queued":
                raise RuntimeError(f"offline direct message was not queued: {queued}")
            alice.send(
                {"command": "group_send", "group_id": group_id, "message": "causal reconnect"}
            )
            alice.event(
                "group_message",
                lambda event: event.get("body") == "causal reconnect" and event.get("outgoing"),
                timeout=40,
            )
            bob = Peer(binary, bob_profile)
            peers.append(bob)
            bob.event("ready")
            bob.event("message", lambda event: event.get("body") == "queued direct", timeout=40)
            bob.event(
                "group_message", lambda event: event.get("body") == "causal reconnect", timeout=40
            )
            alice.send({"command": "search", "query": "causal reconnect"})
            group_search = alice.event("search_results")
            if not any(
                result.get("conversation_type") == "group"
                and result.get("conversation_key") == group_id
                for result in group_search.get("results", [])
            ):
                raise RuntimeError(f"group history was not searchable: {group_search}")

            alice.send(
                {
                    "command": "group_send_file",
                    "group_id": group_id,
                    "path": str(repository / "README.md"),
                }
            )
            alice.event("group_file_sent")
            received = bob.event("group_file_received")
            if Path(received["path"]).read_bytes() != (repository / "README.md").read_bytes():
                raise RuntimeError("received group attachment differs from source")

            alice.send({"command": "link_device", "label": "Laptop"})
            link = alice.event("device_link")["url"]
            subprocess.run(
                [str(binary), "import-device", "--profile", str(laptop_profile), link],
                check=True,
                stdout=subprocess.DEVNULL,
            )
            laptop = Peer(binary, laptop_profile)
            peers.append(laptop)
            laptop.event("ready")
            laptop.event("history_synced", timeout=40)
            laptop.send({"command": "history", "contact": "Bob"})
            laptop_history = laptop.event("history")
            if not any(
                message.get("body") == "hello Bob edited" and message.get("edited")
                for message in laptop_history.get("messages", [])
            ):
                raise RuntimeError(
                    f"linked device did not receive edited direct history: {laptop_history}"
                )
            laptop.event(
                "groups",
                lambda event: any(
                    group["id"] == group_id and group.get("device_count") == 3
                    for group in event["groups"]
                ),
                timeout=40,
            )
            bob.event(
                "contacts",
                lambda event: event["contacts"] and event["contacts"][0].get("device_count") == 2,
            )
            bob.event(
                "groups",
                lambda event: any(
                    group["id"] == group_id and group.get("device_count") == 3
                    for group in event["groups"]
                ),
                timeout=40,
            )
            bob.send({"command": "send", "contact": "Alice", "message": "both devices"})
            sent = bob.event("message_sent")
            if sent.get("devices") != 2:
                raise RuntimeError(f"expected two-device fan-out, got {sent}")
            alice.event("message", lambda event: event.get("body") == "both devices")
            laptop.event("message", lambda event: event.get("body") == "both devices")
            bob.send(
                {"command": "group_send", "group_id": group_id, "message": "all group devices"}
            )
            bob.event(
                "group_message",
                lambda event: event.get("body") == "all group devices" and event.get("outgoing"),
            )
            alice.event("group_message", lambda event: event.get("body") == "all group devices")
            laptop.event("group_message", lambda event: event.get("body") == "all group devices")

            alice.send({"command": "devices"})
            devices = alice.event(
                "devices",
                lambda event: any(
                    device["active"] and not device["current"]
                    for device in event["devices"]
                ),
            )
            laptop_id = next(
                device["id"]
                for device in devices["devices"]
                if device["active"] and not device["current"]
            )
            alice.send(
                {
                    "command": "revoke_device",
                    "device_id": laptop_id,
                    "reason": "e2e revocation test",
                }
            )
            alice.event(
                "devices",
                lambda event: any(
                    device["id"] == laptop_id and not device["active"]
                    for device in event["devices"]
                ),
            )
            bob.event(
                "contacts",
                lambda event: event["contacts"] and event["contacts"][0].get("device_count") == 1,
                timeout=40,
            )
            bob.send({"command": "send", "contact": "Alice", "message": "after revoke"})
            delivered = bob.event(
                "message_sent", lambda event: event.get("body") == "after revoke", timeout=40
            )
            if delivered.get("devices") != 1:
                raise RuntimeError(f"revoked device remained a delivery target: {delivered}")
            alice.event("message", lambda event: event.get("body") == "after revoke")
            laptop.assert_no_event("message", lambda event: event.get("body") == "after revoke")
        finally:
            for peer in reversed(peers):
                peer.close()

    print(
        "pptalk e2e smoke: replies, edits, calls, reconnect, MLS files, "
        "history sync, multi-device and revocation passed"
    )


if __name__ == "__main__":
    main()
