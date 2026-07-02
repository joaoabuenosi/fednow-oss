# FedNow OSS

**Open-source tooling to lower the cost of building _send_ capability on the FedNow® Service.**

> ⚠️ **Early development.** Nothing here is production-ready yet. APIs will change without
> notice until v0.1. Follow the issues/milestones to see where we are.

Most of the 1,500+ institutions on the FedNow network are receive-only: implementing the
send side (ISO 20022 messaging, signing, timeout reconciliation, 24x7 operations) is
expensive and complex. This monorepo is a reference toolchain to change that, aimed at
community banks, credit unions and service providers in the US.

## Components

| Crate | Directory | Status | What it is |
|---|---|---|---|
| `fednow-core` | [`core/`](core/) | 🚧 in progress | ISO 20022 library: parsing, validation (XSD facets + FedNow profile rules), message construction and XMLDSig signing |
| `fednow-sim` | [`simulator/`](simulator/) | 📋 planned | Local FedNow simulator in Docker: accepts pacs.008, replies pacs.002, configurable accept/reject/timeout scenarios, RFP — a preparation tool for the Fed's Customer Testing Program (CTP) |
| `fednow-gateway` | [`gateway/`](gateway/) | 📋 planned | Production send middleware: hexagonal architecture, per-payment state machine, event sourcing, outbox pattern, pacs.028 reconciler |
| `fednow-conformance` | [`conformance/`](conformance/) | 📋 planned | Conformance suite any implementation can run |

## Current milestone

**M1 — `fednow-core` parses and validates a pacs.008 (FI-to-FI customer credit transfer) with green CI.**

```sh
cargo test --workspace
```

## Supported message types (target set)

pacs.008 (credit transfer) · pacs.002 (status) · pacs.028 (status request) ·
pain.013/pain.014 (request for payment) · camt.056/camt.029 (return request/response) ·
admi (ping/broadcast)

## Design principles

- **No blind resends, ever.** Unresolved submissions are reconciled via pacs.028.
- **Idempotency keys are mandatory** at the gateway's northbound API.
- **24x7x365**: zero-downtime deploys, no maintenance windows.
- **No telemetry, no phone-home.** Zero credentials in this repo.
- Docs are a product: the *FedNow Integration Handbook* (in `docs/`) will center on the
  hard production case — timeout reconciliation.

## Security

See [SECURITY.md](SECURITY.md) for the vulnerability disclosure process.

## License

Apache-2.0. See [LICENSE](LICENSE).

---
*"FedNow" is a registered service mark of the Federal Reserve Banks. This is an
independent open-source project, not affiliated with or endorsed by the Federal Reserve.*
