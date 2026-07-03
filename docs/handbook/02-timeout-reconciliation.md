# Chapter 2 — Timeout reconciliation: the hard case

*Status: draft · sources: FedNow Service Release 1 usage guidelines
(pacs.028), FedNow Service Operating Procedures (June 2024), ISO 20022
Implementation Guide v1.8 — plus executable examples against `fednow-sim`.*

Most of the pain in operating a FedNow sender lives in one scenario: **you sent
a pacs.008 and no pacs.002 came back.** This chapter is about what that state
means, why the obvious reaction is the one that loses money, and how to resolve
it correctly.

## 1. Timeout is not failure — it is ignorance

When the advice does not arrive within the expected window, exactly three
things can be true, and *you cannot know which*:

1. The payment **settled** and the advice was lost or delayed on its way back.
2. The payment was **rejected** and the advice was lost or delayed.
3. The payment **never reached** the service.

The money moved in case 1, didn't move in cases 2–3 — and from the sender's
side those states are indistinguishable. Any design that treats "no answer" as
"failed" will eventually **double-pay** (retrying case 1) and any design that
treats it as "succeeded" will eventually **lose payments** (ignoring case 3).

That is why the payment state machine in this project has an explicit terminal
alarm state, distinct from success and failure:

```
CREATED → VALIDATED → SUBMITTED → ACK_PENDING → SETTLED
                                       │        → REJECTED
                                       └──────→ TIMEOUT_UNRESOLVED
```

`TIMEOUT_UNRESOLVED` is not an error code to log and forget. It is a work item:
something (the reconciler) must actively resolve it to `SETTLED` or `REJECTED`
before the books can close.

## 2. The rule: never resend blind

A pacs.008 carries no idempotency semantics on the wire. If the original
message settled (case 1) and you send it again, that is a *second payment* —
same amount, same parties, new money. FedNow is a real-time gross settlement
flow; there is no netting window in which a duplicate quietly cancels out.

The FedNow-native resolution primitive is the **payment status request
(pacs.028)**: "tell me the processing status of the message I identify here."
The service answers with a pacs.002 carrying the status of the *original*
payment — the advice you never received.

Constraints on its use, from the Release 1 usage guideline:

- You can query the service about a pacs.008/pacs.004/pacs.009 you previously
  sent, for the **current or prior calendar day** only.
- Requests **should not be sent before the presumed timeout** of the original
  instruction has elapsed (the service starts the payment timeout clock from a
  timestamp in the technical message header when it receives your message).
- One transaction per request, with the original message fully identified
  (`OrgnlGrpInf`: original MsgId, message name, creation date-time).

## 3. Resolving each case

| pacs.028 outcome | Meaning | State transition |
|---|---|---|
| pacs.002 with `ACSC` | Case 1: it settled; advice was lost | `TIMEOUT_UNRESOLVED → SETTLED` |
| pacs.002 with `RJCT` + reason | Case 2: it was rejected | `TIMEOUT_UNRESOLVED → REJECTED` |
| No record of the message | Case 3: it never arrived | Now — and only now — a resend is safe, as a **new** message with a new MsgId, same `EndToEndId`/UETR for traceability |
| Query itself times out | Still ignorant | Retry the query (queries are idempotent; resending a *question* is always safe), escalate to manual ops if the window closes |

Two invariants worth engraving:

- **Resending a question is safe; resending an instruction is not.** The
  pacs.028 can be retried forever without risk. The pacs.008 can be reissued
  only after the service confirms it has no record of the original.
- **Every `TIMEOUT_UNRESOLVED` must reach a terminal state before end-of-day
  reconciliation.** The current-or-prior-calendar-day query window is the hard
  deadline for automated resolution.

## 4. Hands-on: reproduce it against the simulator

`fednow-sim` decides a final outcome for *every* payment — including the ones
it deliberately never advises. Amounts ending in `.33` trigger the timeout
scenario.

```sh
# Terminal 1
cargo run -p fednow-sim

# Terminal 2 — a $1,250.33 payment: accepted (HTTP 202)… and then silence.
# (Build one with fednow-core's builder, amount_cents = 125_033.)
curl -si -X POST --data-binary @pacs008-timeout.xml \
  -H "content-type: application/xml" http://localhost:8080/fednow/messages
# HTTP/1.1 202 Accepted        ← no pacs.002. You are now in ACK_PENDING.

# Wait out your presumed timeout, then ask instead of resending.
# pacs028.xml identifies the original message id in OrgnlGrpInf/OrgnlMsgId:
curl -s -X POST --data-binary @pacs028.xml \
  -H "content-type: application/xml" http://localhost:8080/fednow/messages
# → a pacs.002 advice with TxSts ACSC: the payment settled all along.
```

Had you resent the pacs.008 instead, the beneficiary would have been paid
twice. Try the third case too: query an id you never sent —

```sh
curl -si -X POST --data-binary @pacs028-unknown.xml \
  -H "content-type: application/xml" http://localhost:8080/fednow/messages
# HTTP/1.1 404 — the service has no record; a fresh send is now legitimate.
```

The exact wire shapes of both messages are enforced by `fednow-core`
(`validate_pacs008`, `validate_pacs028`), and the integration test
`timeout_then_pacs028_reveals_the_settled_truth` in
`simulator/tests/http_tests.rs` runs this whole story on every commit.

## 5. Design corollaries for a production sender

- **Persist before you send.** The original MsgId, creation date-time and UETR
  must survive a crash, or you cannot even ask the question. (This is why the
  gateway uses event sourcing with an outbox — the send intent is durable
  before the wire sees anything.)
- **The reconciler is a first-class component**, not an ops script: it owns
  `ACK_PENDING → …` transitions past the timeout, drives pacs.028 with backoff
  inside the query window, and escalates to humans only when the window closes.
- **Timestamps matter operationally.** The service starts the payment timeout
  clock from the technical header timestamp of your message; your own presumed
  timeout must be calibrated against it, not against local send time
  (clock skew between you and the service is your problem, not the network's).
- **Alarm on age, not just count.** One `TIMEOUT_UNRESOLVED` older than the
  query window is an incident; a thousand younger than 30 seconds may be a
  normal burst.
