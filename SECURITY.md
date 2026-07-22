# Security policy

pptalk is a developer release and has not received an independent security
audit. Do not use it yet where compromise could put someone in danger.

Report a suspected vulnerability privately through GitHub Security Advisories
for this repository. Include the affected version, platform, reproduction and
impact. Please do not open a public issue before a fix is available.

Every change is checked with Rust tests, Clippy and the RustSec advisory
database. The threat boundary and currently accepted local-key limitation are
documented in [`docs/threat-model.md`](docs/threat-model.md).
