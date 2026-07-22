# Self-hosting `pptalk-node`

The node is optional. Direct communication and local/LAN use do not depend on it.
It is licensed AGPL-3.0-or-later because operators provide it as a network
service.

```sh
cargo build --release -p pptalk-node
./target/release/pptalk-node \
  --listen 0.0.0.0:9464 \
  --data-dir /var/lib/pptalk-node
```

Health check: `GET /healthz`.

Mailbox API:

```text
POST /v1/mailboxes/<64-hex-capability>/messages?ttl=86400
GET  /v1/mailboxes/<64-hex-capability>/messages?limit=128
```

Put TLS in front of the listener, restrict request rates at the reverse proxy,
and monitor free disk. Defaults are four MiB per envelope, 256 MiB per capability
and seven days. The node never receives a decryption key.

Configure the desktop in Settings, or initialize a headless profile with:

```sh
pptalk-cli init --profile alice.json --name Alice \
  --mailbox-url https://pptalk.example
```

Mailbox capabilities are directional: even two contacts using the same node
cannot drain each other's queues. Plain HTTP is rejected except for loopback
development URLs. The current node provides durable mailbox service; live media
uses the authenticated Iroh mesh and does not depend on this HTTP service.
