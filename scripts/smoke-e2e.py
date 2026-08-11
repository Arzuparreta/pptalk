#!/usr/bin/env python3
"""Exercise real QUIC, MLS, files and linked-device fan-out on loopback."""

from __future__ import annotations

import argparse
import json
import queue
import socket
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path


def free_port() -> int:
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


class Node:
    """An opaque mailbox so store-and-forward can be exercised for real."""

    def __init__(self, binary: Path, data_dir: Path) -> None:
        self.port = free_port()
        self.url = f"http://127.0.0.1:{self.port}"
        self.process = subprocess.Popen(
            [str(binary), "--listen", f"127.0.0.1:{self.port}", "--data-dir", str(data_dir)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                stderr = self.process.stderr.read() if self.process.stderr else ""
                raise RuntimeError(f"mailbox node exited early: {stderr}")
            try:
                with urllib.request.urlopen(f"{self.url}/healthz", timeout=1) as response:
                    if response.status == 200:
                        return
            except (urllib.error.URLError, OSError, TimeoutError):
                time.sleep(0.2)
        raise RuntimeError(f"mailbox node never became healthy on {self.url}")

    def close(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.communicate(timeout=8)
                return
            except subprocess.TimeoutExpired:
                self.process.kill()
        self.process.communicate()


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
        self.stderr_lines: list[str] = []
        self.reader = threading.Thread(target=self._read_events, daemon=True)
        self.stderr_reader = threading.Thread(target=self._read_stderr, daemon=True)
        self.reader.start()
        self.stderr_reader.start()

    def _read_events(self) -> None:
        assert self.process.stdout is not None
        try:
            for line in self.process.stdout:
                try:
                    self.events.put(json.loads(line))
                except json.JSONDecodeError:
                    continue
        except ValueError:
            # The stream can be closed by cleanup after another peer fails.
            pass

    def _read_stderr(self) -> None:
        assert self.process.stderr is not None
        try:
            for line in self.process.stderr:
                self.stderr_lines.append(line.rstrip())
        except ValueError:
            pass

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
                value = self.events.get(timeout=min(0.2, max(0, deadline - time.monotonic())))
            except queue.Empty:
                return_code = self.process.poll()
                if return_code is not None:
                    self.reader.join(timeout=1)
                    self.stderr_reader.join(timeout=1)
                    stderr = "\n".join(self.stderr_lines)
                    raise RuntimeError(
                        f"daemon exited with code {return_code} while waiting for {name}; "
                        f"seen={seen}; stderr={stderr}"
                    )
                continue
            seen.append(value.get("event"))
            if value.get("event") == name and predicate(value):
                return value
            self.pending.append(value)
        stderr = "\n".join(self.stderr_lines)
        raise RuntimeError(f"timeout waiting for {name}; seen={seen}; stderr={stderr}")

    def close(self) -> None:
        if self.process.poll() is None:
            try:
                self.send({"command": "shutdown"})
                self.process.wait(timeout=8)
            except (BrokenPipeError, subprocess.TimeoutExpired):
                self.process.kill()
                self.process.wait(timeout=8)
        if self.process.stdin is not None:
            self.process.stdin.close()
        self.reader.join(timeout=1)
        self.stderr_reader.join(timeout=1)

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
    parser.add_argument("--node-binary", type=Path, default=Path("target/debug/pptalk-node"))
    args = parser.parse_args()
    binary = args.binary.resolve()
    node_binary = args.node_binary.resolve()
    repository = Path(__file__).resolve().parent.parent
    peers: list[Peer] = []
    node: Node | None = None
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
            invite_event = alice.event("invite")
            invite = invite_event["url"]
            assert invite_event["qr_svg"].startswith("<svg")
            bob.send({"command": "accept", "url": invite})
            bob.event(
                "contacts",
                lambda event: any(
                    contact.get("name") == "Alice" for contact in event.get("contacts", [])
                ),
            )
            alice.event(
                "contacts",
                lambda event: any(
                    contact.get("name") == "Bob" for contact in event.get("contacts", [])
                ),
            )
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

            alice.send(
                {
                    "command": "send_file",
                    "contact": "Bob",
                    "path": str(repository / "README.md"),
                }
            )
            alice.event("file_sent")
            direct_file = bob.event("file_received")
            if Path(direct_file["path"]).read_bytes() != (repository / "README.md").read_bytes():
                raise RuntimeError("received direct attachment differs from source")

            cancelled_path = root / "cancelled-transfer.bin"
            with cancelled_path.open("wb") as cancelled_file:
                cancelled_file.truncate(8 * 1024 * 1024)
            alice.send(
                {
                    "command": "send_file",
                    "contact": "Bob",
                    "path": str(cancelled_path),
                }
            )
            cancellable = alice.event(
                "transfer_progress",
                lambda event: event.get("cancelable") is True
                and event.get("file_name") == cancelled_path.name,
            )
            alice.send(
                {
                    "command": "cancel_transfer",
                    "transfer_id": cancellable["transfer_id"],
                }
            )
            alice.event(
                "transfer_cancelled",
                lambda event: event.get("transfer_id") == cancellable["transfer_id"],
            )

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
            participant = ringing_call["participants"][0]
            alice.send(
                {
                    "command": "set_participant_volume",
                    "call_id": ringing_call["call_id"],
                    "device_id": participant["device_id"],
                    "volume": 0.35,
                }
            )
            volume = alice.event("participant_volume")
            if volume.get("device_id") != participant["device_id"] or volume.get("volume") != 0.35:
                raise RuntimeError(f"participant volume did not round-trip: {volume}")
            alice.send({"command": "hold_call", "call_id": ringing_call["call_id"]})
            alice.event("call_held")
            bob.event("call_remote_held")
            alice.send({"command": "resume_call", "call_id": ringing_call["call_id"]})
            alice.event("call_resumed")
            bob.event("call_remote_resumed")
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

            # Store-and-forward. Everything above needed both daemons alive at the
            # same time; this is the only path that survives presences that never
            # overlap. A fresh pair keeps the assertions free of accumulated state.
            node = Node(node_binary, root / "mailbox-data")
            carol_profile = root / "carol.json"
            dave_profile = root / "dave.json"
            for profile, name in ((carol_profile, "Carol"), (dave_profile, "Dave")):
                subprocess.run(
                    [str(binary), "init", "--profile", str(profile), "--name", name],
                    check=True,
                    stdout=subprocess.DEVNULL,
                )
            carol, dave = Peer(binary, carol_profile), Peer(binary, dave_profile)
            peers.extend((carol, dave))
            carol.event("ready")
            dave.event("ready")
            carol.send({"command": "invite", "expires_seconds": 3600})
            dave.send({"command": "accept", "url": carol.event("invite")["url"]})
            dave.event(
                "contacts",
                lambda event: any(
                    contact.get("name") == "Carol" for contact in event.get("contacts", [])
                ),
            )
            carol.event(
                "contacts",
                lambda event: any(
                    contact.get("name") == "Dave" for contact in event.get("contacts", [])
                ),
            )

            # Dave is away when Carol picks a mailbox, so the announcement has to
            # wait in her outbox. Without it he would never learn where to deposit.
            dave.close()
            carol.send({"command": "set_mailbox", "url": node.url})
            configured = carol.event("mailbox_configured", timeout=60)
            if not configured.get("reachable"):
                raise RuntimeError(f"mailbox probe failed: {configured}")
            # The daemon stores a normalized URL, so compare without the trailing slash.
            if str(configured.get("mailbox_url", "")).rstrip("/") != node.url.rstrip("/"):
                raise RuntimeError(f"mailbox was not stored: {configured}")

            dave = Peer(binary, dave_profile)
            peers.append(dave)
            dave.event("ready")
            carol.event(
                "outbox_delivered", lambda event: event.get("count", 0) > 0, timeout=60
            )

            # Now the sender is the one who stays online and the recipient is gone
            # for good: only a deposit can still reach her.
            carol.close()
            dave.send({"command": "send", "contact": "Carol", "message": "deposited while away"})
            deposited = dave.event(
                "message_sent",
                lambda event: event.get("body") == "deposited while away",
                timeout=60,
            )
            if deposited.get("delivery") != "mailbox":
                raise RuntimeError(
                    f"message for an offline contact never reached the mailbox: {deposited}"
                )

            carol = Peer(binary, carol_profile)
            peers.append(carol)
            carol.event("ready")
            carol.event(
                "message",
                lambda event: event.get("body") == "deposited while away",
                timeout=60,
            )

            # Clearing is the exact payload the desktop's "Quitar buzón" button sends.
            carol.send({"command": "set_mailbox", "url": None})
            cleared = carol.event("mailbox_configured", timeout=60)
            if cleared.get("mailbox_url") is not None:
                raise RuntimeError(f"mailbox was not cleared: {cleared}")
        finally:
            for peer in reversed(peers):
                peer.close()
            if node is not None:
                node.close()

    print(
        "pptalk e2e smoke: symmetric contacts, direct files, cancellation, replies, "
        "edits, calls, reconnect, MLS files, history sync, multi-device, revocation "
        "and mailbox store-and-forward passed"
    )


if __name__ == "__main__":
    main()
