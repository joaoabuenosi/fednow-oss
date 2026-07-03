# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[SemVer](https://semver.org) (pre-1.0: minor bumps may break APIs).

## [Unreleased]

Planned: MQ-compatible simulator/adapter interface, camt.056/camt.029
(return request flow), message signing once the Technical Specifications wire
format is obtained ([#14](https://github.com/joaoabuenosi/fednow-oss/issues/14)),
release artifact signing (Sigstore), Java/Python SDKs, public benchmarks.

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

[Unreleased]: https://github.com/joaoabuenosi/fednow-oss/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/joaoabuenosi/fednow-oss/releases/tag/v0.1.0
