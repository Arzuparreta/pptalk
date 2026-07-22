# Protocol v1

All structured messages use CBOR with a four-MiB allocation limit. Unknown
incompatible versions are rejected before domain application.

## Durable synchronization

`SyncFrame::Hello` advertises device identity and per-conversation causal
frontiers. `Events` sends only sequences above that frontier. `Ack` advances the
sender outbox. Event IDs and `(conversation, author_device, sequence)` are unique,
so reconnect and mailbox retries are harmless.

The v0.1 daemon applies this model through encrypted per-device outbox retries
and group frontier requests. A peer only re-emits events it originally authored,
which preserves transport-authenticated authorship. Membership records carry a
per-identity history floor, so a newly admitted identity cannot recover events
from before its admission. A newly linked device of an existing identity keeps
that identity's floor but receives a separate MLS leaf.

Blob manifests contain ciphertext and chunk hashes. Chunks may arrive out of
order, are deduplicated by index and are verified individually and as a complete
ciphertext before atomic assembly. Direct sends switch the remaining chunks to
the mailbox after a path failure.

## Calls

`CallSignal` covers targeted invitations, silent join, leave and publication
changes. RTP packets use QUIC datagrams on a separate media ALPN; group calls
fan out one encoded stream across a peer mesh. The protocol reserves SDP/ICE and
router-offer variants for interoperable transports, but the v0.1 runtime does
not advertise an SFU.

## Nodes

Node access is capability based. A mailbox capability is a 32-byte unguessable
token represented as lowercase hex in HTTP paths. The payload remains a complete
client-encrypted transport envelope. `POST` deposits with bounded TTL; `GET`
atomically drains a bounded batch. Clients deduplicate event IDs after retries.
