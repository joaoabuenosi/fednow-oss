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
| `fednow-sim` | [`simulator/`](simulator/) | 🚧 v0 (HTTP dev mode) | Local FedNow simulator: accepts pacs.008, replies pacs.002 advices under configurable accept/reject/ACWP/timeout scenarios — a preparation tool for the Fed's Customer Testing Program (CTP) |
| `fednow-gateway` | [`gateway/`](gateway/) | 🚧 v0 (REST + reconciler) | Production send middleware: event-sourced state machine, idempotency-keyed REST API, background pacs.028 reconciler; durable storage and the MQ adapter next |
| `fednow-conformance` | [`conformance/`](conformance/) | 📋 planned | Conformance suite any implementation can run |

## Current milestone

**M3 — `fednow-sim` v0: local simulator with accept/reject/timeout scenarios.**
Done: M1 (pacs.008 parse/validate); M2 (credit-transfer message set on the real
FedNow profiles, calibrated against the official Release 1 samples — message
signing is tracked in [#14](https://github.com/joaoabuenosi/fednow-oss/issues/14),
blocked on the access-controlled Technical Specifications).

```sh
cargo test --workspace
```

## Try the whole loop

Two terminals: the simulator plays the FedNow Service, the gateway is your
sending institution.

```sh
# terminal 1 — the FedNow Service (simulated)
cargo run -p fednow-sim

# terminal 2 — your gateway
cargo run -p fednow-gateway

# send a payment (amounts ending .33 simulate the hard case: no advice)
curl -s -X POST http://localhost:8090/payments \
  -H "content-type: application/json" -H "Idempotency-Key: demo-1" \
  -d '{"reference":"DEMO0001","amount_cents":125000,
       "debtor_name":"Jane","debtor_account":"123456789012",
       "creditor_name":"John","creditor_account":"987654321000",
       "creditor_agent_routing_number":"091000019","category_purpose":"CONS"}'
# → {"state":"SETTLED", ...}  — a FedNow-profile pacs.008 went out, the
#   pacs.002 advice came back, the state machine settled. Same Idempotency-Key
#   again returns the same payment; timeouts resolve via pacs.028 in the
#   background — never a blind resend.
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
