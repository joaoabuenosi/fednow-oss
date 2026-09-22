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
  startup it logs a fixed line saying authentication is on. Nothing derived
  from the keys goes to the log, not even how many there are.
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
[timeout chapter](../docs/handbook/02-timeout-reconciliation.md)). With a
[pre-send risk check](#pre-send-risk-check) configured, a payment can also stop
at `VALIDATED → HELD | REFUSED` without being sent.

## Pre-send risk check

An optional check between validation and the outbox. It is the last point where
a sender can stop a payment, because FedNow settlement is final
([handbook ch. 5](../docs/handbook/05-returns.md)). **It is off unless you turn
it on.** With `FEDNOW_GW_RISK_PROVIDER` unset, no check runs, no event is
recorded, and responses are byte-for-byte what they were before.

**What this is and is not.** It is this project's own vendor-neutral extension
point: a `RiskProvider` trait (`fednow_gateway::risk`) that an in-house rule
engine or a vendor score can implement. The Federal Reserve announced a Network
Intelligence API for FedNow® Service participants in April 2026, which returns
receiver account-level data before a payment is sent. **This gateway does not
integrate with that API, is not compatible with it, and ships no client for
it.** Its wire contract (endpoints, authentication, fields) is not public.
[Issue #95](https://github.com/joaoabuenosi/fednow-oss/issues/95) tracks what
is known, what is not, and what is blocked on the specification.

**Flow.** `CREATED → VALIDATED → [risk check] → SUBMITTED → …`

| Outcome | State | Sent? |
|---|---|---|
| `allow` | stays `VALIDATED`, then continues to `SUBMITTED` | yes |
| `hold` | `HELD` | no. The payment waits for a person |
| `refuse` | `REFUSED` | no. Terminal. Not the same as `REJECTED`, which the far side says *after* sending |

Invalid payments (`422`) never reach the check. A resubmission with the same
`Idempotency-Key` returns the payment as it stands. The provider is not asked
again and nothing is sent. This version has **no route that releases a `HELD`
payment**, because releasing it needs the built message parked durably outside
the outbox (a follow-up in #95). To send it after review, submit again under a
new idempotency key. `HELD` and `REFUSED` show up in `/ops/summary` under
`by_state`, and both SDKs treat them as final in `wait_final` / `waitFinal`.

**When the provider cannot answer.** A provider can time out, fail (an error,
a malformed answer, a panic), or the concurrency budget can be exhausted. In
each case `FEDNOW_GW_RISK_ON_UNAVAILABLE` decides the outcome:

- **`hold` (default): fail closed, recoverably.** Settlement is final, so a check
  that lets payments through whenever its provider is down protects nothing
  during exactly the outage an attacker would wait for. A hold is preferred
  over a refusal because a hold can be undone after review and a refusal
  cannot. The cost is availability: while the provider is down, payments queue
  as `HELD`. Alert on it.
- `refuse`: fail closed, terminally. For institutions that would rather the
  customer retry than have an operator review a backlog.
- `allow`: fail open. The payment is sent unchecked. This is a legitimate choice
  when availability outranks the risk signal, but it is explicit, never a
  default, and every unchecked send is still recorded with the reason the
  check did not happen.

**Timeout and budget.** `FEDNOW_GW_RISK_TIMEOUT_MS` is the latency budget of one
check, end to end. The gateway enforces it from outside the provider: it runs
the call on its own thread and stops waiting when the budget runs out, so a hung
provider cannot stall a submission. The gateway does not retry. A provider that
retries must do so within the budget, which it receives. The 1000 ms default is
this project's choice, not a published figure. The check runs inside the
synchronous `POST /payments`, so it has to stay small next to the presumed
timeout. `FEDNOW_GW_RISK_MAX_IN_FLIGHT` bounds provider calls in flight. A call
the gateway stopped waiting for keeps its slot until the provider actually
returns, so a hung provider exhausts the budget (and the policy decides) instead
of piling up threads.

**Audit trail.** Every check that runs appends one `RiskChecked` event to the
payment's event stream: `outcome`, `reason`, `source`
(`provider` / `timeout` / `error` / `budget_exhausted`), `provider` (a name such
as `sim`), `elapsed_ms`, `at_unix`. It contains no account numbers, names,
amounts, routing numbers, provider payloads or error text. A provider's reason is
kept only if it is a short code (`[a-z][a-z0-9_.-]{0,63}` with no run of five or
more digits), so an account number cannot slip in. Anything else is recorded as
`unspecified`. When the policy decides, the reason is fixed:
`risk_check_timeout`, `risk_check_error` or `risk_check_budget_exhausted`. The
REST view reports the same thing as `risk`, a field that is present only when a
check ran:

```json
{"idempotency_key":"order-9","state":"HELD", ... ,"risk":{"outcome":"hold","reason":"sim.hold","source":"provider"}}
```

**Trying it.** `FEDNOW_GW_RISK_PROVIDER=sim` uses `fednow-sim`'s demo endpoint
(`POST /demo/risk-check`). Amounts ending in `.77` are held, `.88` refused,
`.98` fail with HTTP 503, and `.99` answer too late. Its request and response
are this project's invention, not a model of any Federal Reserve API. See
[QUICKSTART step 6](../QUICKSTART.md#6-optional-a-pre-send-risk-check).

**Your own provider.** Implement `RiskProvider` (`name` + a blocking `check`
returning `Allow`, `Hold { reason }` or `Refuse { reason }`), wrap it in
`RiskGate::new(provider, RiskPolicy { .. })`, and pass it to
`PaymentService::with_risk_gate`. The binary only knows `none` and `sim`. Wiring
another provider into it is a code change.

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
| `FEDNOW_GW_RISK_PROVIDER` | unset (= `none`) | [Pre-send risk check](#pre-send-risk-check): `none` = off, `sim` = fednow-sim's demo endpoint. Any other value → the gateway refuses to start |
| `FEDNOW_GW_RISK_ON_UNAVAILABLE` | `hold` | What a timeout, provider error or exhausted budget means: `hold` (fail closed), `refuse` (fail closed, terminal), `allow` (fail open) |
| `FEDNOW_GW_RISK_TIMEOUT_MS` | `1000` | Latency budget of one check. Positive integer |
| `FEDNOW_GW_RISK_MAX_IN_FLIGHT` | `32` | Provider calls allowed in flight at once. Positive integer |
| `FEDNOW_GW_RISK_SIM_URL` | `FEDNOW_GW_SIM_URL` | Base URL of the demo risk endpoint (`sim` provider only) |

The `FEDNOW_GW_RISK_*` variables other than `FEDNOW_GW_RISK_PROVIDER` are read
only when a provider is set. An invalid value stops the gateway at startup with
an error naming the variable. It is never ignored.

## Operating notes

- **Watch `/ops/summary`** with a read-only key. `outbox_pending` growing
  means the transport is down or refusing; `oldest_unresolved_age_secs` is
  the number to page on. Probes that only need liveness use `/healthz`,
  which needs no key.
- **Restart-safe by construction**: events and the outbox are SQLite rows
  written transactionally; reopening the database replays every payment.
- **With a risk check on, watch `HELD` in `/ops/summary`.** Under the default
  fail-closed policy, a provider outage shows up there, not as failed sends.
  The startup log states whether the check is on and with which policy.
- **Never resend.** There is deliberately no endpoint to re-emit a pacs.008.
  Ambiguity is resolved by pacs.028 — the handbook explains why the obvious
  alternative loses money.
