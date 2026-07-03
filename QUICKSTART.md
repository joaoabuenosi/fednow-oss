# Quick Start — send your first FedNow payment in 5 minutes

No Rust knowledge required. You need **Docker** (with compose) and `curl`.
Every command and every output below was executed against the real code —
what you see is what you get.

## 1. Start the stack

```sh
git clone https://github.com/joaoabuenosi/fednow-oss.git
cd fednow-oss
docker compose up --build
```

Two services come up:

- **fednow-sim** on `:8080` — plays the FedNow Service, with MQ-style
  queue-pair semantics (fire-and-forget sends, advices on a receive queue).
- **fednow-gateway** on `:8090` — your sending institution: idempotent REST
  API, event-sourced state machine, outbox, background reconciler.

> Prefer raw binaries? `cargo run -p fednow-sim` and
> `FEDNOW_GW_SOUTHBOUND=mq cargo run -p fednow-gateway` do the same.

## 2. Send a payment that settles

```sh
curl -s -X POST http://localhost:8090/payments \
  -H "content-type: application/json" \
  -H "Idempotency-Key: quickstart-1" \
  -d '{
    "reference": "QS0001",
    "amount_cents": 125000,
    "debtor_name": "Jane Example",   "debtor_account": "123456789012",
    "creditor_name": "John Example", "creditor_account": "987654321000",
    "creditor_agent_routing_number": "091000019",
    "category_purpose": "CONS"
  }'
```

```json
{"idempotency_key":"quickstart-1","state":"ACK_PENDING","message_identification":"20260703021040078QS0001","end_to_end_identification":"QS0001","uetr":null,"queries_sent":0,"rejection_reason":null,"events":4}
```

Note the state: **`ACK_PENDING`, not settled.** A FedNow-profile pacs.008
went out over the (simulated) MQ connection, and MQ sends are
fire-and-forget — the answer arrives asynchronously on a receive queue.
That is how the real service behaves, and the gateway is built around it.

Ask again a couple of seconds later:

```sh
curl -s http://localhost:8090/payments/quickstart-1
```

```json
{"idempotency_key":"quickstart-1","state":"SETTLED","message_identification":"20260703021040078QS0001","end_to_end_identification":"QS0001","uetr":null,"queries_sent":0,"rejection_reason":null,"events":5}
```

The pacs.002 advice (`ACSC`) arrived on the queue, the background pump
applied it, the state machine settled. **The money moved.**

Two rules you just used without noticing:

- The `Idempotency-Key` header is **mandatory**. Repeat the exact same POST
  and you get the settled payment back — nothing touches the wire twice.
- Amounts are **integer cents**. No floats anywhere near money.

## 3. Send a payment that gets rejected

Amounts steer the simulator (Stripe-sandbox style). Anything ending in
`.11` is refused by the receiving bank:

```sh
curl -s -X POST http://localhost:8090/payments \
  -H "content-type: application/json" \
  -H "Idempotency-Key: quickstart-2" \
  -d '{ "reference": "QS0002", "amount_cents": 125011,
    "debtor_name": "Jane Example",   "debtor_account": "123456789012",
    "creditor_name": "John Example", "creditor_account": "987654321000",
    "creditor_agent_routing_number": "091000019", "category_purpose": "CONS" }'
```

Moments later:

```json
{"idempotency_key":"quickstart-2","state":"REJECTED", ... ,"rejection_reason":"AC04","events":5}
```

`AC04` is the ISO reason code (account closed) — carried through from the
pacs.002 exactly as a real receiving bank would send it.

## 4. The hard case: no answer at all

Amounts ending in `.33` make the simulator go silent — the payment settles
internally, but **no advice is ever pushed**. This is the case that loses
money in production when handled wrong (resend = double pay).

```sh
curl -s -X POST http://localhost:8090/payments \
  -H "content-type: application/json" \
  -H "Idempotency-Key: quickstart-3" \
  -d '{ "reference": "QS0003", "amount_cents": 125033,
    "debtor_name": "Jane Example",   "debtor_account": "123456789012",
    "creditor_name": "John Example", "creditor_account": "987654321000",
    "creditor_agent_routing_number": "091000019", "category_purpose": "CONS" }'
```

Now just watch:

```sh
watch -n 2 'curl -s http://localhost:8090/payments/quickstart-3'
```

The payment sits in `ACK_PENDING`, crosses the presumed timeout into
`TIMEOUT_UNRESOLVED`, and then the background reconciler sends a
**pacs.028 payment status request** — never a resend. The withheld advice
comes back on the queue, and about half a minute after submission:

```json
{"idempotency_key":"quickstart-3","state":"SETTLED", ... ,"queries_sent":1,"rejection_reason":null,"events":7}
```

`queries_sent: 1` and 7 events tell the whole story: submitted → published →
timeout declared → pacs.028 sent → advice received → settled. It had
settled all along; a blind retry would have paid twice. This flow is the
reason this project exists — the full write-up is the
[timeout reconciliation chapter](docs/handbook/02-timeout-reconciliation.md).

## 5. Send something invalid

The gateway validates against the real FedNow Release 1 profile **before**
anything reaches the wire:

```sh
# category_purpose must be CONS or BIZZ
curl -s -X POST http://localhost:8090/payments \
  -H "content-type: application/json" -H "Idempotency-Key: quickstart-4" \
  -d '{ "reference": "QS0004", "amount_cents": 125000, "category_purpose": "WRONG",
    "debtor_name": "Jane Example",   "debtor_account": "123456789012",
    "creditor_name": "John Example", "creditor_account": "987654321000",
    "creditor_agent_routing_number": "091000019" }'
```

```json
{"codes":["fednow.ctgypurp.known"],"error":"fednow_profile_violation"}
```

HTTP `422` with stable rule codes — every violation at once, not just the
first. The same validator (and the same codes) is available as a library
(`fednow-core`), a CLI (`fednow-conformance`), and a
[language-agnostic vector corpus](conformance/vectors/) your own
implementation can run against.

## Amount triggers cheat sheet

| Amount ends in | Scenario |
|---|---|
| anything else | settled (`ACSC`) |
| `.11` | rejected by the receiving bank (`RJCT`/`AC04`) |
| `.22` | accepted without posting (`ACWP`) |
| `.33` | **timeout** — no advice until a pacs.028 asks |
| `.44` | settled after a 2-second delay |
| `.55` | rejected by the service itself (`RJCT`/`E990`) |
| `.66` | `ACWP` now, receiver's `ACCC` pushed moments later |

Per-routing-number scenarios via a TOML file: see the
[simulator README](simulator/README.md).

## Use it from your language

The same flow, without hand-writing HTTP — both SDKs are integration-tested
against this exact stack in CI:

**Python** ([`sdk/python/`](sdk/python/), zero dependencies):

```python
from fednow_client import GatewayClient

gw = GatewayClient("http://localhost:8090")
gw.submit("order-1", reference="ORDER0001", amount_cents=125_000,
          debtor_name="Jane", debtor_account="123456789012",
          creditor_name="John", creditor_account="987654321000",
          creditor_agent_routing_number="091000019")
print(gw.wait_final("order-1").state)   # SETTLED
```

**Java 17** ([`sdk/java/`](sdk/java/)):

```java
var gw = new GatewayClient("http://localhost:8090");
gw.submit("order-1", SubmitPaymentRequest.builder()
    .reference("ORDER0001").amountCents(125_000)
    .debtorName("Jane").debtorAccount("123456789012")
    .creditorName("John").creditorAccount("987654321000")
    .creditorAgentRoutingNumber("091000019").build());
System.out.println(gw.waitFinal("order-1").state());   // SETTLED
```

## Operate it

`GET /ops/summary` is the operator's glance — counts by state, outbox
depth, and the age of the oldest unresolved payment (the number to page
on). Full endpoint and environment reference:
[gateway README](gateway/README.md).

## Where to go next

- **Understand the flow** you just ran: handbook
  [chapter 1 — the credit transfer flow](docs/handbook/01-credit-transfer-flow.md).
- **Point your own code at the sim**: HTTP dev mode (`POST /fednow/messages`,
  synchronous) or MQ mode (`/mq/participants/{rtn}/send` + `/receive`) —
  [simulator README](simulator/README.md).
- **Prepare for certification**: [from zero to the Customer Testing
  Program](docs/handbook/04-zero-to-ctp.md), and
  `cargo run -p fednow-conformance -- scenarios` runs the CTP arcs live.
- **Use the library**: [`fednow-core`](core/) parses, validates and builds
  all seven Release 1 credit-transfer-family message types.
