# fednow-gateway

Send middleware for the FedNow Service: idempotency-keyed REST northbound,
event-sourced state machine on SQLite, a real outbox, a background
reconciler that resolves timeouts via pacs.028 (never a blind resend), and
a swappable southbound (simulator today — HTTP or MQ semantics — the real
IBM MQ adapter per [`docs/design/mq-transport.md`](../docs/design/mq-transport.md)).

New here? The repo-root [QUICKSTART](../QUICKSTART.md) sends a payment in
five minutes. SDKs: [Python](../sdk/python/) · [Java](../sdk/java/).

## Run

```sh
cargo run -p fednow-sim                                 # terminal 1
FEDNOW_GW_SOUTHBOUND=mq cargo run -p fednow-gateway     # terminal 2
# or both at once: docker compose up --build
```

## REST API

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/payments` | Submit a payment. **`Idempotency-Key` header mandatory** (missing → `400`). Profile violation → `422` + `{"codes": [...]}`. |
| `GET` | `/payments/{key}` | Current state (`404` for unknown keys). |
| `POST` | `/payments/{key}/reconcile` | Drive one reconciliation pass now (also runs on the background sweeper). |
| `GET` | `/ops/summary` | Operational snapshot: counts by state, `outbox_pending`, `oldest_unresolved_age_secs`. |
| `GET` | `/healthz` | Liveness. |

Submission body (amounts are **integer cents**):

```json
{
  "reference": "ORDER0001",
  "amount_cents": 125000,
  "debtor_name": "Jane Example",   "debtor_account": "123456789012",
  "creditor_name": "John Example", "creditor_account": "987654321000",
  "creditor_agent_routing_number": "091000019",
  "category_purpose": "CONS",
  "end_to_end_identification": "optional — defaults to reference",
  "uetr": "optional"
}
```

States: `CREATED → VALIDATED → SUBMITTED → ACK_PENDING → SETTLED | REJECTED`,
plus `TIMEOUT_UNRESOLVED` (a work item, resolved by the reconciler — see the
[timeout chapter](../docs/handbook/02-timeout-reconciliation.md)).

## Configuration (environment)

| Variable | Default | Meaning |
|---|---|---|
| `FEDNOW_GW_ADDR` | `0.0.0.0:8090` | REST listen address |
| `FEDNOW_GW_SIM_URL` | `http://localhost:8080` | Southbound target (fednow-sim) |
| `FEDNOW_GW_SOUTHBOUND` | `http` | `http` = synchronous dev mode; `mq` = production semantics (fire-and-forget sends + advice queue, enveloped messages) |
| `FEDNOW_GW_SENDER_RTN` | `021040078` | Your connection party id (routing number) |
| `FEDNOW_GW_DB` | `fednow-gateway.db` | SQLite event store path (state survives restarts) |
| `FEDNOW_GW_TIMEOUT_SECS` | `20` | Presumed timeout: `ACK_PENDING` older than this becomes `TIMEOUT_UNRESOLVED` |
| `FEDNOW_GW_BACKOFF_SECS` | `30` | Minimum interval between pacs.028 queries per payment |
| `FEDNOW_GW_SWEEP_SECS` | `10` | Background sweeper cadence (outbox retry → advice pump → reconciliation) |

## Operating notes

- **Watch `/ops/summary`.** `outbox_pending` growing means the transport is
  down or refusing; `oldest_unresolved_age_secs` is the number to page on.
- **Restart-safe by construction**: events and the outbox are SQLite rows
  written transactionally; reopening the database replays every payment.
- **Never resend.** There is deliberately no endpoint to re-emit a pacs.008.
  Ambiguity is resolved by pacs.028 — the handbook explains why the obvious
  alternative loses money.
