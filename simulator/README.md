# fednow-sim

A local FedNow Service simulator: POST a pacs.008 customer credit transfer,
receive the pacs.002 advice the FedNow Service would send — under scenarios you
control, including the one that hurts in production (no answer at all).

> v0 speaks HTTP (dev mode). An MQ-compatible interface is planned; the
> scenario engine is shared.

## Quickstart

```sh
cargo run -p fednow-sim
# in another shell: generate a valid pacs.008 and send it
cargo run -p fednow-core --example build_pacs008 > /tmp/pacs008.xml
curl -s -X POST --data-binary @/tmp/pacs008.xml \
  -H "content-type: application/xml" http://localhost:8080/fednow/messages
```

The response is a pacs.002 `ACSC` advice (accepted, settlement completed) that
`fednow-core` itself validates as direction-clean.

Docker:

```sh
docker build -f simulator/Dockerfile -t fednow-sim .
docker run -p 8080:8080 fednow-sim
```

## Scenarios

Priority: config file → amount trigger → default.

| Trigger | Scenario | Advice |
|---|---|---|
| default | settle | `ACSC` + acceptance time + effective settlement date |
| amount ends `.11` | reject | `RJCT` reason `AC04` |
| amount ends `.22` | accept without posting | `ACWP` |
| amount ends `.33` | **timeout** | none — HTTP `202`, no pacs.002 |
| amount ends `.44` | delayed settle | `ACSC` after 2 s |
| amount ends `.55` | reject by the service | `RJCT` proprietary reason `E990` (vs `.11`'s participant reject) |
| amount ends `.66` | ACWP + follow-up | `ACWP` now; `ACCC` follow-up queued, retrieved with a pacs.028 |
| profile-invalid message | always rejected | `RJCT` proprietary reason `SIMV`, violated rule codes in `AddtlInf` |

Each trigger maps to an official Customer Testing Program scenario — see the
[zero-to-CTP chapter](../docs/handbook/04-zero-to-ctp.md).

Config file (`fednow-sim.toml` or `FEDNOW_SIM_CONFIG`), keyed by creditor-agent
routing number — see [fednow-sim.toml.example](fednow-sim.toml.example).
Actions: `settle`, `accept-without-posting`, `reject` (+ `reason`), `timeout`,
`delay` (+ `delay_ms`). The real service's technical error codes will replace
`SIMV` once the Technical Specifications are available (issue #14).

## Timeout reconciliation — the point of this simulator

A timed-out payment is *unresolved*, not failed: internally it still settled;
your sender just never heard it. The simulator keeps an advice ledger for every
processed payment, and a **payment status request (pacs.028)** — posted to the
same endpoint, like on the real MQ channel — returns the withheld advice:

```sh
# 1. Send a pacs.008 with an amount ending in .33 → HTTP 202, no advice.
# 2. Ask what happened:
curl -s -X POST --data-binary @pacs028.xml \
  -H "content-type: application/xml" http://localhost:8080/fednow/messages
# → the ACSC advice: it settled all along. A blind resend would have paid twice.
```

Unknown original message id → HTTP `404`. The full walkthrough lives in the
FedNow Integration Handbook chapter on timeout reconciliation (`docs/handbook/`).

## MQ mode — the real connection's semantics

The production FedNow connection is an IBM MQ queue pair, and it is
**asynchronous**: a send returns nothing; advices arrive later on your receive
queue, wrapped in the FedNow technical envelope with a Business Application
Header. MQ mode mirrors exactly that:

```sh
# PUT: fire-and-forget send of a FedNowIncoming envelope → 202, empty body
curl -s -X POST --data-binary @envelope.xml \
  -H "content-type: application/xml" \
  http://localhost:8080/mq/participants/021040078/send

# GET: next FedNowOutgoing envelope from your receive queue (204 when empty)
curl -s http://localhost:8080/mq/participants/021040078/receive
```

Differences from the HTTP dev mode, all deliberate:

- **Every** outcome is asynchronous — even profile-validation rejections
  (`RJCT`/`SIMV`) arrive as queued advices, not HTTP errors.
- The post-ACWP follow-up status (`.66` trigger) is **pushed** to the queue
  ~500 ms later; no pacs.028 needed (the HTTP mode cannot push).
- A timeout (`.33`) leaves the queue empty — poll all you want, nothing comes
  until a pacs.028 (sent through the same `/send`) replays the withheld advice.

Scenario triggers, config file and the advice ledger are shared between modes.

## Endpoints

| Method | Path | Purpose |
|---|---|---|
| POST | `/fednow/messages` | HTTP dev mode: pacs.008 → pacs.002 advice; pacs.028 → stored advice replay |
| POST | `/mq/participants/{rtn}/send` | MQ mode: fire-and-forget `FedNowIncoming` (pacs.008, pacs.028) |
| GET | `/mq/participants/{rtn}/receive` | MQ mode: destructive get of the next `FedNowOutgoing` |
| GET | `/healthz` | liveness |

State is in-memory and per-process (v0): restart forgets past payments and
queued advices.
