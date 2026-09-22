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
export FEDNOW_GW_API_KEY="$(openssl rand -hex 32)"      # the gateway refuses to start without a key
cargo run -p fednow-sim                                 # terminal 1
FEDNOW_GW_API_KEYS="$FEDNOW_GW_API_KEY" \
  FEDNOW_GW_SOUTHBOUND=mq cargo run -p fednow-gateway   # terminal 2 (same key)
# or both at once: docker compose up --build (reads FEDNOW_GW_API_KEY)
```

## Authentication

Every route except `GET /healthz` requires `Authorization: Bearer <key>`.

- **Fail closed.** If `FEDNOW_GW_API_KEYS` is unset or holds no key, the
  gateway exits with an error naming the variable, before it opens its
  database or binds its port. There is no switch to run unauthenticated. It
  also refuses to start on a key shorter than 32 characters, a key with
  whitespace or non-ASCII characters, or a key listed in both tiers. The error
  gives the key's position in the list, never its value.
- **Two tiers.** Full-access keys (`FEDNOW_GW_API_KEYS`) can call every
  route. Read-only keys (`FEDNOW_GW_READ_API_KEYS`) can call `GET` routes
  only; they are meant for monitoring `/ops/summary`.
- **Status codes.** A missing, malformed or unknown key gets `401` with
  `WWW-Authenticate: Bearer realm="fednow-gateway"`. A read-only key on a
  write route gets `403`. Neither response echoes the credential.
- **Rotation without downtime.** Both variables are comma-separated lists.
  Add the new key, restart, move clients over, then remove the old key.
- **Storage.** The gateway keeps only SHA-256 digests of the configured keys,
  and compares a presented key against every digest in constant time. At
  startup it logs how many keys of each tier it loaded, and nothing else about
  them.
- **Not TLS.** The gateway speaks plain HTTP, so a bearer key crosses the
  network in the clear unless something encrypts it. Outside a laptop, put it
  behind a TLS-terminating proxy or a service mesh; that proxy is also where
  mTLS goes, if you want it.

## REST API

| Method | Path | Access | Purpose |
|---|---|---|---|
| `POST` | `/payments` | full | Submit a payment. **`Idempotency-Key` header mandatory** (missing → `400`). Profile violation → `422` + `{"codes": [...]}`. |
| `GET` | `/payments/{key}` | read | Current state (`404` for unknown keys). |
| `POST` | `/payments/{key}/reconcile` | full | Drive one reconciliation pass now (also runs on the background sweeper). |
| `GET` | `/ops/summary` | read | Operational snapshot: counts by state, `outbox_pending`, `oldest_unresolved_age_secs`. |
| `GET` | `/healthz` | public | Liveness. The only route without a key, so probes need none. |

*full* = a `FEDNOW_GW_API_KEYS` key; *read* = either tier; *public* = no key.

Submission body (amounts are **integer cents**):

```json
{
  "reference": "ORDER0001",
  "amount_cents": 125000,
  "debtor_name": "Jane Example",   "debtor_account": "123456789012",
  "creditor_name": "John Example", "creditor_account": "987654321000",
  "creditor_agent_routing_number": "992000008",
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
| `FEDNOW_GW_API_KEYS` | **none: required** | Comma-separated full-access API keys, each at least 32 visible-ASCII characters (`openssl rand -hex 32`). Unset or empty → the gateway refuses to start |
| `FEDNOW_GW_READ_API_KEYS` | empty | Comma-separated read-only API keys (`GET` routes only) |
| `FEDNOW_GW_ADDR` | `0.0.0.0:8090` | REST listen address |
| `FEDNOW_GW_SIM_URL` | `http://localhost:8080` | Southbound target (fednow-sim) |
| `FEDNOW_GW_SOUTHBOUND` | `http` | `http` = synchronous dev mode; `mq` = production semantics (fire-and-forget sends + advice queue, enveloped messages) |
| `FEDNOW_GW_SENDER_RTN` | `991000009` | Your connection party id (routing number) |
| `FEDNOW_GW_DB` | `fednow-gateway.db` | SQLite event store path (state survives restarts) |
| `FEDNOW_GW_TIMEOUT_SECS` | `20` | Presumed timeout: `ACK_PENDING` older than this becomes `TIMEOUT_UNRESOLVED` |
| `FEDNOW_GW_BACKOFF_SECS` | `30` | Minimum interval between pacs.028 queries per payment |
| `FEDNOW_GW_SWEEP_SECS` | `10` | Background sweeper cadence (outbox retry → advice pump → reconciliation) |

## Operating notes

- **Watch `/ops/summary`** with a read-only key. `outbox_pending` growing
  means the transport is down or refusing; `oldest_unresolved_age_secs` is
  the number to page on. Probes that only need liveness use `/healthz`,
  which needs no key.
- **Restart-safe by construction**: events and the outbox are SQLite rows
  written transactionally; reopening the database replays every payment.
- **Never resend.** There is deliberately no endpoint to re-emit a pacs.008.
  Ambiguity is resolved by pacs.028 — the handbook explains why the obvious
  alternative loses money.
