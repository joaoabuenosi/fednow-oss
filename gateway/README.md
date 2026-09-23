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
  whitespace or non-ASCII characters, a key listed in two tiers, or an
  operator entry without a valid, unique label. The error gives the entry's
  position in the list, never its value.
- **Three tiers.** Full-access keys (`FEDNOW_GW_API_KEYS`) submit and
  reconcile. Read-only keys (`FEDNOW_GW_READ_API_KEYS`) can call `GET` routes
  only; they are meant for monitoring `/ops/summary`. Operator keys
  (`FEDNOW_GW_OPERATOR_API_KEYS`, optional) can read, and are the **only**
  keys that can [release or cancel a held payment](#resolving-a-held-payment).
- **Separation of duties.** A full-access key cannot release a hold, and an
  operator key cannot submit. Otherwise the application that submits payments
  could release every payment the risk check holds, and the check would stop
  nothing. With no operator key configured, nobody can release a hold.
- **Operator keys are labelled.** Each entry is `label:key`, e.g.
  `ops-alice:<64 hex chars>`. The label is a short code
  (`[a-z][a-z0-9_.-]`, 1–32 characters, starting with a letter, no run of
  five digits) and is what the event history records as the actor,
  `operator:ops-alice`. It comes from the key, so a request cannot choose it.
  Keep the mapping from labels to people in your own directory; the gateway
  stores no names.
- **Status codes.** A missing, malformed or unknown key gets `401` with
  `WWW-Authenticate: Bearer realm="fednow-gateway"`. A key on a route outside
  its tier gets `403`. Neither response echoes the credential.
- **Rotation without downtime.** All three variables are comma-separated lists.
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
| `POST` | `/payments/{key}/release` | operator | Send a `HELD` payment's parked message. Body `{"reason": "<code>"}`. See [Resolving a held payment](#resolving-a-held-payment). |
| `POST` | `/payments/{key}/cancel` | operator | Cancel a `HELD` payment: `CANCELLED`, never sent. Body `{"reason": "<code>"}`. |
| `GET` | `/ops/summary` | read | Operational snapshot: counts by state, `outbox_pending`, `oldest_unresolved_age_secs`. |
| `GET` | `/healthz` | public | Liveness. The only route without a key, so probes need none. |

*full* = a `FEDNOW_GW_API_KEYS` key; *operator* = a `FEDNOW_GW_OPERATOR_API_KEYS`
key and no other; *read* = any tier; *public* = no key.

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
at `VALIDATED → HELD | REFUSED` without being sent, and a `HELD` payment then
goes on to `SUBMITTED` (released) or ends `CANCELLED` (cancelled or expired).

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
| `hold` | `HELD` | not yet. The built message is parked; an operator releases it (→ `SUBMITTED`) or cancels it (→ `CANCELLED`) |
| `refuse` | `REFUSED` | no. Terminal. Not the same as `REJECTED`, which the far side says *after* sending |

Invalid payments (`422`) never reach the check. A resubmission with the same
`Idempotency-Key` returns the payment as it stands. The provider is not asked
again and nothing is sent, before or after a release. `HELD`, `REFUSED` and
`CANCELLED` show up in `/ops/summary` under `by_state`, and both SDKs'
`wait_final` / `waitFinal` return on them instead of polling to their timeout.

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

### Resolving a held payment

A hold is where a person takes over. `POST /payments/{key}/release` sends the
payment; `POST /payments/{key}/cancel` ends it without sending. Both need an
[operator key](#authentication) and a body `{"reason": "<code>"}`, where the
reason follows the same short-code rule as risk reasons (e.g. `reviewed_ok`,
`confirmed_fraud`). Free text is refused with `400 invalid_reason`, not
rewritten: a note field is where account numbers and names end up.

**Release sends the message that was checked, not a rebuilt one.** When the
check says hold, the built pacs.008 is **parked** in its own table, in the same
transaction as the hold, and the event history records only its SHA-256
(`HoldParked`). A release moves those exact bytes from parking to the outbox in
one transaction (`HoldReleased`, recording the same digest), and the store
refuses if the parked bytes no longer match it. From the outbox on, the payment
is like any other: published, advised, reconciled. It cannot reach the outbox
twice. Parking is not the outbox, the move deletes the parked copy, the state
machine accepts one `HoldReleased` and only from `HELD`, and the outbox has a
unique index on the idempotency key. A second release, or two racing ones,
gets `409 not_held`, and the message is sent once. The idempotency key behaves
as before: resubmitting it returns the payment as it stands.

**Cancel** ends the payment in `CANCELLED` and deletes the parked message,
since a message that can never be sent is not kept. `CANCELLED` is its own
terminal state, and neither of the other two fits. `REFUSED` is the risk
check's verdict at submission time and `REJECTED` is the far side's verdict
after sending. Folding a person's decision into either would make the
provider's refusal rate and the service's rejection rate lie.

**Staleness: a hold expires.** A parked message keeps the creation date-time
and settlement date it was built with, and the gateway will not rebuild it. So
the age of a hold is also how stale those dates are. The rule is the simplest
one that bounds that. A hold is releasable for `FEDNOW_GW_HOLD_MAX_AGE_SECS`
(default 4 hours, a local choice, not a published figure), up to and including
that second. After that the background sweeper cancels it with actor `gateway`
and reason `hold_expired`. A release that arrives before the sweeper does the
same and gets `409 hold_expired`; nothing is sent. An operator can still cancel
a stale hold explicitly. To send the payment anyway, submit it again under a
new idempotency key. It is rebuilt with today's dates and checked again. A
re-check on release was considered and not chosen. It would ask the provider
about a message a person has already reviewed, and it would still send dates
from the day of the hold.

**Audit.** Every outcome is an event in the payment's history, and survives
restarts like every other event:

| Event | Records |
|---|---|
| `HoldParked` | `message_sha256`, `at_unix` |
| `HoldReleased` | `actor` (`operator:<label>`), `reason`, `message_sha256`, `at_unix` |
| `HoldCancelled` | `actor` (`operator:<label>`, or `gateway` on expiry), `reason` (`hold_expired` on expiry), `at_unix` |

None of them carries an account, a name, an amount or a message body, and the
gateway writes no log line per release or cancel. The REST view adds `hold`,
present only for a payment that was held:

```json
"hold":{"held_at_unix":1790110339,"releasable_until_unix":1790124739,"resolution":null}
"hold":{"held_at_unix":1790110339,"resolution":{"action":"released","actor":"operator:ops-demo","reason":"reviewed_ok","at_unix":1790110339}}
```

| Response | When |
|---|---|
| `200` + the payment | released (then `ACK_PENDING`/`SETTLED`/…) or cancelled (`CANCELLED`) |
| `400 invalid_reason` | the reason is not a short code |
| `401` / `403` | no key / a key that is not an operator key |
| `404` | unknown payment |
| `409 not_held` + `state` | not `HELD`: never held, already released, cancelled, refused |
| `409 hold_expired` | past the maximum age; the payment is now `CANCELLED` |
| `409 no_parked_message` | a hold recorded before parking existed (a gateway built from #96). It can be cancelled, not released |

Release needs no risk provider: a gateway restarted without one can still
release or cancel what an earlier run held.

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
| `FEDNOW_GW_OPERATOR_API_KEYS` | empty | Comma-separated `label:key` operator keys: the only keys that can [release or cancel a held payment](#resolving-a-held-payment). Empty → nobody can |
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
| `FEDNOW_GW_HOLD_MAX_AGE_SECS` | `14400` (4 h) | How long a held payment stays releasable; after that the sweeper cancels it (`hold_expired`). Positive integer |

The `FEDNOW_GW_RISK_*` variables other than `FEDNOW_GW_RISK_PROVIDER` are read
only when a provider is set. `FEDNOW_GW_HOLD_MAX_AGE_SECS` is always read, so a
bad value is caught before a provider is turned on. An invalid value stops the
gateway at startup with an error naming the variable. It is never ignored.

## Operating notes

- **Watch `/ops/summary`** with a read-only key. `outbox_pending` growing
  means the transport is down or refusing; `oldest_unresolved_age_secs` is
  the number to page on. Probes that only need liveness use `/healthz`,
  which needs no key.
- **Restart-safe by construction**: events and the outbox are SQLite rows
  written transactionally; reopening the database replays every payment.
- **With a risk check on, watch `HELD` in `/ops/summary`.** Under the default
  fail-closed policy, a provider outage shows up there, not as failed sends.
  The startup log states whether the check is on, with which policy, and how
  long a hold stays releasable. A rising `CANCELLED` count whose payments
  show `"actor":"gateway"` in `hold.resolution` means holds are expiring
  unreviewed: staff the review, or raise the maximum age knowingly.
- **Never resend.** There is deliberately no endpoint to re-emit a pacs.008.
  Ambiguity is resolved by pacs.028 — the handbook explains why the obvious
  alternative loses money. Releasing a held payment is not a resend: it
  sends, once, a message that never left.
