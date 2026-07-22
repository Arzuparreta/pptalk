# Threat model

## Protected

- Message, attachment and call content against relays, mailbox nodes, network
  observers and service operators.
- Identity/device changes against forgery and log forks.
- Local conversation databases at rest through SQLCipher.
- Invitation reachability data against modification; the inviter consumes a
  pending capability on the first authenticated response.
- Manual media quality choices against silent fallback.

## Observable metadata

A network observer or relay can learn approximate online times, endpoint pairs,
traffic volume and timing. Padding reduces size leakage but does not provide an
anonymity network. A mailbox operator sees capability buckets, expiry and blob
sizes. Contact display names are not registered globally.

## Trust assumptions

The operating system, active device process and input/output drivers are trusted.
Malware with access to an unlocked process can read messages. Contact safety
requires checking an out-of-band fingerprint when impersonation matters.

## Recovery and compromise

There is no central password reset. A still-authorized device can revoke another
device; peers reject its stale identity log, stop direct fan-out and group owners
remove its MLS leaf. If every device is lost, the identity and
unexported history are unavailable. This is an availability cost, not a backdoor.

The current profile stores endpoint/device seed material in a mode-0600 file.
SQLCipher protects conversation history, but its key currently lives beside the
profile. Full-disk encryption and a locked desktop session are therefore part of
the local-at-rest trust assumption for this release. Moving profile seeds into
the OS credential vault remains required before calling the client hardened.

## Non-goals for 1.0

Anonymous communication, public discovery, communities, bots, recording,
transcription, browser access, cloud backup and game overlays are out of scope.
