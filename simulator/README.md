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
| profile-invalid message | always rejected | `RJCT` proprietary reason `SIMV`, violated rule codes in `AddtlInf` |

Config file (`fednow-sim.toml` or `FEDNOW_SIM_CONFIG`), keyed by creditor-agent
routing number — see [fednow-sim.toml.example](fednow-sim.toml.example).

The timeout scenario is the point of this simulator: your sender must resolve
it with a payment status request (pacs.028), never a blind resend. The real
service's technical error codes will replace `SIMV` once the Technical
Specifications are available (issue #14).

## Endpoints

| Method | Path | Purpose |
|---|---|---|
| POST | `/fednow/messages` | pacs.008 in, pacs.002 advice out |
| GET | `/healthz` | liveness |
