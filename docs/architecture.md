# Architecture

## Product boundary

pptalk is a private friend graph, not a hosted community service. There is no
global account table, tenant database or authoritative service. A "tenant" is a
cryptographic identity log replicated by its devices; a conversation is an
independent encrypted event log whose creator controls membership.

The desktop application is native Qt Quick. It contains no browser engine,
Chromium runtime or Electron dependency. Rust crates hold the portable domain,
wire protocol, encrypted storage, transport and media policy.

## Data path

```text
Qt Quick UI
    | native controller
    v
conversation events --> end-to-end encryption --> authenticated QUIC
    |                                               |
    +-- SQLCipher local history                     +-- direct UDP path
    +-- causal outbox                               +-- encrypted relay fallback
                                                       |
                                                       +-- optional opaque mailbox
```

An event has a content-derived random ID, author identity and device, monotonic
per-device sequence, causal frontier and versioned body. Inserts are idempotent.
Peers exchange frontiers and only send missing events. A new group member is
given state from the membership commit onward; earlier events are not included.

## Identity and device setup

The first device generates an Ed25519 signing key and a genesis identity event.
Additional devices create their own keys and are linked with signed `AddDevice`
events. Revocation is another signed event. Losing every active device means
losing the identity; there is intentionally no mandatory recovery authority.
An authorized device announces its own endpoint and MLS key package to existing
peers. Group owners add it as a distinct MLS leaf; no leaf secret is cloned.
Revoking a device removes it from direct fan-out and causes owned groups to
advance to an epoch that excludes it.

Contact links contain a one-use capability, a signed device/address proof and an
expiry. Possession and signature validation of the explicitly shared link is the
initial contact verification; applications can additionally compare identity
fingerprints out of band.

## Network and nodes

Iroh provides authenticated QUIC endpoints, NAT traversal, discovery and direct
path selection. `pptalk/sync/1` carries durable frames and `pptalk/call/1` carries
ephemeral call signaling. Relays see endpoint timing and ciphertext sizes, never
message plaintext. Padding is supported by the transport envelope.

The optional node is replaceable and carries no identity authority. Its durable,
directional capability mailbox has byte quotas and TTL deletion. The current
node implements mailbox service only; live NAT traversal/relay is supplied by
Iroh and calls use client mesh.

If direct delivery and every advertised mailbox fail, the encrypted envelope is
kept in the sender's SQLCipher outbox and retried with ordered exponential
backoff. Group peers also exchange causal frontiers when reconnecting. Each
author re-emits only its own transport-authenticated events; an identity added
later is bounded by its membership timestamp and cannot request earlier history.

## Media

GStreamer performs native capture, DSP, encoding and decoding. Capture pipelines
emit RTP-ready buffers for the call transport. Automatic quality may adapt to
capability and packet loss. Manual quality is immutable: if unsupported the
operation fails; it never silently changes resolution, frame rate or bitrate.

Calls use mesh for small groups, with eight participants as the optimization
target rather than a protocol limit. Entering without ringing and ringing
selected members are distinct signaling actions.

## Crate boundaries

- `protocol`: versioned CBOR wire types and limits.
- `core`: identity logs, conversation rules, signatures and encryption.
- `storage`: SQLCipher history, outbox, blobs and replay protection.
- `network`: Iroh transport plus sync and call channels.
- `media`: native GStreamer capability and quality policy.
- `mls`: RFC 9420 membership epochs and forward-secure group messages.
- `apps/cli`: headless identity, contact and encrypted messaging workflow.
- `apps/desktop`: Qt Quick desktop application.
- `apps/node`: optional AGPL opaque mailbox service.
