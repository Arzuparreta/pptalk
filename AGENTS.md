# Working in pptalk

This file contains repository instructions for maintainers and coding agents.
It is not an end-user guide.

## Product constraints

- The desktop client is native Qt Quick. Electron, Chromium and embedded web
  views are forbidden.
- There is no central account authority. Identities and conversations remain
  local-first and server components must stay optional and replaceable.
- Never “fix” a startup or migration problem by deleting a profile, rotating an
  identity or discarding MLS state. Reproduce against a copy and migrate safely.
- Preserve end-to-end encryption across direct, relay and mailbox routes.
- Keep the common gaming path lightweight; avoid background queues, polling or
  media work that consumes resources without an active need.

## Documentation boundaries

- `README.md` is the short product entry point.
- `docs/installation.md` and `docs/user-guide.md` are user-facing and should use
  simple Spanish with exact labels from the UI.
- `docs/development.md`, `docs/architecture.md` and `docs/protocol.md` are for
  contributors. Put build internals and implementation detail there.
- Agent handoffs, temporary status, speculative plans and test transcripts do
  not belong in user documentation.
- Update user docs whenever a visible workflow or label changes.

## Required checks

Run checks proportional to the change. The full validation set is:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 scripts/smoke-e2e.py
```

For desktop changes, also build with CMake. For persisted-state changes, prove
both a new profile and migration of the previous format; never use the user's
only profile as a fixture.

## Process safety

Use `scripts/dev.sh` to manage local processes. A stop operation must validate
the recorded PID against an executable in this repository. Do not use broad
`pkill`, `killall` or patterns that can terminate another project's services.

Build output, logs, PIDs and development node data belong under ignored
directories such as `build/` or `target/`.
