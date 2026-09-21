# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[SemVer](https://semver.org) (pre-1.0: minor bumps may break APIs).

## [Unreleased]

## [0.3.1] — 2026-09-21

A release-pipeline fix. No library, gateway, simulator or conformance code
changed between 0.3.0 and 0.3.1 — only the workflow that builds and signs the
artifacts, plus the version and notes that go with it.

### Fixed

- **Release pipeline** — the SPDX SBOM step was pinned to an outdated
  `anchore/sbom-action` and produced no file; v0.3.0 was published without
  assets and is superseded by 0.3.1. The pin ([#81](https://github.com/joaoabuenosi/fednow-oss/pull/81)
  replaced the floating `@v0` ref with the SHA of v0.9.0, a version that
  predates the `output-file`, `upload-artifact` and `upload-release-assets`
  inputs the step passes) is now v0.24.2, whose `action.yml` declares all
  three. The action only warns about inputs it does not know, so the run
  carried on and failed two steps later in `sha256sum`, before cosign ever
  ran; nothing was signed
  ([#83](https://github.com/joaoabuenosi/fednow-oss/pull/83)).

### Added

- Each SBOM step is now followed by a check that the file it owns exists and
  is non-empty, so a generator that silently produces nothing fails at the
  step responsible for it and names the likely cause
  ([#83](https://github.com/joaoabuenosi/fednow-oss/pull/83)).
- The release workflow accepts `workflow_dispatch` with a `dry_run` input
  (default on). A dry run executes the same guard, build, test, SBOM and
  checksum steps against the working tree — naming every asset from a
  synthetic `v<workspace version>` tag — and attaches the results to the run,
  so the pipeline can be proven green before a tag is pushed. Signing and the
  GitHub release live in a separate job that a dry run skips entirely, so the
  rehearsal path is never granted `id-token: write` or `contents: write`
  ([#83](https://github.com/joaoabuenosi/fednow-oss/pull/83)).

### Changed

- A dispatched publishing run (`dry_run` off) is refused unless it starts from
  a tag ref, since cosign binds the signing certificate to the ref
  ([#83](https://github.com/joaoabuenosi/fednow-oss/pull/83)).

## [0.3.0] — 2026-09-21

Verifiable releases — signed keyless with Sigstore, with a CycloneDX SBOM —
client SDKs for Python and the JVM, and a five-minute path from
`docker compose up` to a settled payment.

### Supply chain

- **Keyless release signing**: releases are now signed with Sigstore/cosign
  (GitHub Actions OIDC — no private key, no repository secret) and carry a
  `.cosign.bundle` per asset. A merged **CycloneDX** SBOM (`cargo-cyclonedx`)
  ships alongside the existing SPDX one. Third-party actions in the release
  and Scorecard workflows are pinned to commit SHAs and every job declares
  minimal `permissions`. `SECURITY.md` now documents the exact
  `cosign verify-blob` command instead of asserting that releases are signed
  ([#81](https://github.com/joaoabuenosi/fednow-oss/pull/81)).
- New **OpenSSF Scorecard** workflow publishes its results to the public
  OpenSSF API, uploads findings to code scanning, and adds the README badge
  ([#81](https://github.com/joaoabuenosi/fednow-oss/pull/81)).
- The release workflow verifies, as its first step, that the pushed tag equals
  `v` + `[workspace.package] version` from `Cargo.toml`, and fails naming both
  values otherwise. Nothing is built, packaged or signed before that check, so
  a mistyped tag can no longer produce a signed artifact whose name disagrees
  with its contents
  ([#82](https://github.com/joaoabuenosi/fednow-oss/pull/82)).
- Security: rustls 0.23.41 → 0.23.45 (pulled in transitively by ureq) for
  [RUSTSEC-2026-0285](https://rustsec.org/advisories/RUSTSEC-2026-0285.html):
  TLS 1.3 handshake messages were accepted across encryption level
  boundaries. Lockfile-only; no API or behaviour change on our side
  ([#72](https://github.com/joaoabuenosi/fednow-oss/pull/72)).

### SDKs

- **Python SDK** (`sdk/python/`, `fednow-gateway-client`): zero-dependency
  client — idempotent `submit`, `wait_final` that keeps waiting through
  `TIMEOUT_UNRESOLVED`, `ProfileViolation` with rule codes. Unit-tested
  against a stub and integration-tested against the live gateway↔sim stack
  (MQ mode) in CI
  ([#49](https://github.com/joaoabuenosi/fednow-oss/pull/49)).
- **Java SDK** (`sdk/java/`, `io.github.joaoabuenosi:fednow-gateway-client`):
  the same client contract on Java 17 — builder-checked
  `SubmitPaymentRequest`, mandatory idempotency key on `submit`, `waitFinal`
  that understands the timeout case, typed `GatewayException`. One runtime
  dependency (Jackson); HTTP via the JDK's `java.net.http`. Unit- and
  integration-tested against the live stack in CI
  ([#51](https://github.com/joaoabuenosi/fednow-oss/pull/51)).

### fednow-core

- **quick-xml 0.42**: name accessors now yield `&str`, so element-name
  matching compares against string literals. `ParseError::Xml` and
  `BuildError::Serialize` now wrap the 0.42 `DeError`/`SeError` types
  (upstream renamed `DeError::UnexpectedStart` to `MixedContent`); parsing
  behaviour is unchanged
  ([#71](https://github.com/joaoabuenosi/fednow-oss/pull/71)).

### fednow-gateway

- `/ops/summary` endpoint: payment counts by state plus reconciler health, so
  an operator can see the whole book without querying the event store
  ([#50](https://github.com/joaoabuenosi/fednow-oss/pull/50)).

### Documentation

- **QUICKSTART.md**: 5-minute Docker + curl walkthrough, every output
  captured from a live run (settle, reject, timeout→pacs.028, 422 with rule
  codes); compose pins a 2s sweeper so the documented timings hold
  ([#47](https://github.com/joaoabuenosi/fednow-oss/pull/47)).
- Handbook chapter 5: the returns flow (pacs.004, camt.056/camt.029)
  ([#50](https://github.com/joaoabuenosi/fednow-oss/pull/50)).
- Design doc for the real IBM MQ transport (`docs/design/mq-transport.md`):
  MQI via the IBM redistributable client behind an `ibm-mq` feature flag
  ([#48](https://github.com/joaoabuenosi/fednow-oss/pull/48)).

### Project

- Dependency majors consolidated: ureq 3 (adapter migration), rusqlite 0.40
  (MSRV → 1.95), toml 1, checkout v7, gh-release v3
  ([#46](https://github.com/joaoabuenosi/fednow-oss/pull/46)).
- CI/process: the esteira pipeline (plugin settings, `AGENTS.md`, CODEOWNERS,
  PR template, weekly digest)
  ([#58](https://github.com/joaoabuenosi/fednow-oss/pull/58)).

Planned: real IBM MQ transport implementation (phases in the design doc),
message signing once the Technical Specifications wire format is obtained
([#14](https://github.com/joaoabuenosi/fednow-oss/issues/14)), published
release artifacts — crates.io, container images, PyPI and Maven Central
([#64](https://github.com/joaoabuenosi/fednow-oss/issues/64)) — and public
benchmarks.

## [0.2.0] — 2026-07-03

MQ semantics end to end, the full returns message set, and supply-chain
guardrails.

### fednow-core

- **camt.056.001.08** (return request) and **camt.029.001.09** (resolution of
  investigation) modules with FedNow-profile validation; **pacs.004.001.10**
  calibrated against the real Release 1 usage guideline.
- **MQ technical envelope** (`envelope` module): byte-exact `split()`,
  typed `parse()` and `build()` for `FedNowIncoming`/`FedNowOutgoing`
  (AppHdr + Document under type-specific wrappers), plus
  `validate_envelope()` cross-checks (wrapper↔Document, `MsgDefIdr`,
  direction↔service party).
- `Head001Builder` — FedNow-profile Business Application Header construction.

### fednow-sim

- **MQ mode**: `/mq/participants/{rtn}/send` (fire-and-forget PUT of a
  `FedNowIncoming` envelope) + `/mq/participants/{rtn}/receive` (destructive
  GET of the next `FedNowOutgoing`). All outcomes asynchronous — including
  profile-validation rejections; ACWP follow-ups are pushed; timeouts leave
  the queue empty until a pacs.028 replays the withheld advice.

### fednow-gateway

- **MQ-style southbound adapter** (`MqSimPort`, `FEDNOW_GW_SOUTHBOUND=mq`):
  sends wrap the business message in a `FedNowIncoming` envelope with a BAH
  and are fire-and-forget; a `poll_advice` port method plus
  `PaymentService::pump_advices` drain the receive queue and drive the state
  machine from `FedNowOutgoing` advices (correlated by original message id).
  The docker-compose wiring now runs gateway↔sim over MQ semantics.

### fednow-conformance

- Envelope vectors: the corpus (24 vectors) now asserts technical-envelope
  handling — valid both directions, wrapper↔Document mismatch, `MsgDefIdr`
  mismatch — next to the bare-Document vectors.

### Project

- **Supply-chain guardrails**: daily + lockfile-triggered `cargo audit`
  workflow, weekly grouped Dependabot updates. The first audit run caught two
  HIGH DoS advisories in quick-xml 0.37.5 (RUSTSEC-2026-0194/0195) —
  upgraded to 0.41.0.
- Handbook chapter 1: the credit transfer flow (pacs.008 → pacs.002).
- `fednow-core` carries crates.io metadata and its own README.

## [0.1.0] — 2026-07-03

First release: the complete send-side loop — build, validate, send, advise,
reconcile — running end to end against a local FedNow Service simulator.

### fednow-core

- Typed models, parsers and rule validation for **pacs.008.001.08**,
  **pacs.002.001.10** (both FedNow directions), **pacs.028.001.03**,
  **pacs.004.001.10** and **head.001.001.02** (BAH), enforcing the real
  FedNow Service Release 1 profiles (message id shape, FDN/CLRG/SLEV/USABA,
  USD cent amounts, service identifier `021150706`, direction-dependent BAH
  and status rules). Every violation carries a stable rule code and its
  source (XSD facet / ISO rule / FedNow profile).
- Builders for pacs.008, pacs.002 (both directions) and pacs.028 — money is
  integer cents, nothing is defaulted from clocks or randomness.
- Calibrated against the official Release 1 artifacts: all 81 structurally
  valid sample messages parse and validate clean; base ISO 20022 schemas
  vendored and every fixture XSD-validated in CI.
- `validate` example: judge any message file from the command line.

### fednow-sim

- Local FedNow Service simulator (HTTP dev mode + Docker): pacs.008 in,
  pacs.002 advice out, under configurable scenarios — settle, participant
  reject, service reject, accept-without-posting (with follow-up statuses),
  delayed advice, and **timeout** (no advice; the payment settles internally
  and a pacs.028 reveals it — the production lesson this project exists to
  teach). Covers all six official CTP credit-transfer scenarios.

### fednow-gateway

- Send middleware: event-sourced per-payment state machine
  (`CREATED → VALIDATED → SUBMITTED → ACK_PENDING → SETTLED | REJECTED |
  TIMEOUT_UNRESOLVED`), idempotency-keyed REST API, background reconciler
  (declare timeout → pacs.028 with backoff — never a blind resend), durable
  SQLite event store with a **real outbox** (`Submitted` + wire message in
  one transaction; `Published` only after confirmed handoff), state proven
  to survive close-and-reopen.

### fednow-conformance

- Language-agnostic vector corpus (16 vectors, expected verdicts + rule
  codes), `validate` CLI for any file/directory, and a live scenario runner
  that certifies an endpoint against the six CTP credit-transfer scenarios.
  fednow-core passes its own corpus and fednow-sim passes the runner, in CI.

### Documentation & operations

- FedNow Integration Handbook: timeout reconciliation (the hard case) and
  zero-to-CTP chapters, runnable against the simulator.
- Design docs recording the FedNow profile facts and the message-signing
  research (signature travels outside the XML; wire format pending — #14).
- CI on every commit: fmt, clippy `-D warnings`, full test suite, official
  XSD validation. `docker compose up` brings up simulator + gateway.
- Releases ship an SPDX SBOM and SHA-256 checksums.

[Unreleased]: https://github.com/joaoabuenosi/fednow-oss/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/joaoabuenosi/fednow-oss/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/joaoabuenosi/fednow-oss/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/joaoabuenosi/fednow-oss/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/joaoabuenosi/fednow-oss/releases/tag/v0.1.0
