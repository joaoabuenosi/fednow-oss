# Chapter 1 — The credit transfer flow (pacs.008 → pacs.002)

*Status: draft · sources: FedNow Service Release 1 usage guidelines, FedNow
Service Operating Procedures, Technical Overview and Planning Guide — plus
executable examples against `fednow-sim`.*

Everything the FedNow send side does is a variation of one exchange: you send
a **pacs.008** (customer credit transfer) and the service answers with a
**pacs.002** (payment status report). Master this pair — including the ways
the answer can *not* arrive — and every other flow (returns, status requests,
requests for payment) is a footnote to it.

## 1. Who is talking to whom

FedNow is a **participant-to-service** protocol, not participant-to-participant.
Your institution (or your service provider) holds a connection to the FedNow
Service; the receiving institution holds its own. You never talk to the
receiver directly:

```
Sender FI ──pacs.008──▶ FedNow Service ──pacs.008──▶ Receiver FI
Sender FI ◀─pacs.002─── FedNow Service ◀─pacs.002─── Receiver FI
```

Two consequences that shape everything downstream:

- **Two answers, one message.** The status you receive can originate from the
  *service* (it validated, settled, or rejected your message) or be relayed
  from the *receiving participant* (it accepted or refused the funds). The
  gateway must treat both as advices on the same payment.
- **Signing is point-to-point.** Each leg is signed between one party and the
  service; you verify the service's signature, never the receiver's
  (Readiness Guide, *Information Security*).

## 2. The wire shape: envelope, header, document

On the production connection (IBM MQ), a business message is three layers:

```
FedNowIncoming                          ← technical envelope (MQ wire)
└── FedNowCustomerCreditTransfer        ← one wrapper element per message type
    ├── AppHdr    (head.001.001.02)     ← who → whom, what, when
    └── Document  (pacs.008.001.08)     ← the business payload
```

The **Business Application Header** carries the connection party identifiers
(`Fr` = your routing number, `To` = the service application, `021150706`),
names the enclosed message (`MsgDefIdr`), and declares the market practice
(`frb.fednow.01`). The FedNow profile *removes* the BAH signature slot —
signatures travel outside the XML (see chapter 3, blocked on the Technical
Specifications).

`fednow-core` models all three layers; `envelope::build`/`parse` handle the
wrapper without ever re-serializing your document — what you signed is what
travels.

## 3. The pacs.008 in one table

The FedNow Release 1 profile is much stricter than the base ISO 20022 schema.
The rules that reject real messages in practice:

| Field | Rule |
|---|---|
| `GrpHdr/MsgId` | `CCYYMMDD` + your 9-char connection party id + 1–18 alphanumerics — and it is your **correlation key** for the payment's whole life |
| `NbOfTxs` | always `"1"` — one transaction per message, no batching |
| `SttlmInf` | `SttlmMtd` = `CLRG`, `ClrSys/Cd` = `FDN` |
| `IntrBkSttlmAmt` | USD only; ≤ 14 total digits, ≤ 2 fraction digits (work in cents, never floats) |
| `ChrgBr` | `SLEV` only |
| Agents | `ClrSysId/Cd` = `USABA` + 9-digit ABA routing number (checksum enforced) |
| `PmtTpInf` | mandatory: `LclInstrm/Prtry` = `FDNA`, `CtgyPurp/Prtry` ∈ {`CONS`, `BIZZ`} |
| Accounts | `DbtrAcct` and `CdtrAcct` mandatory |

`fednow-core`'s validator returns *all* violations with stable rule codes
(`fednow.aba.checksum`, `fednow.ccy.usd`, …) so one round trip gives the full
diagnosis. Validate **before** the wire, always: a profile-invalid message
costs you a round trip and comes back as a service rejection anyway.

## 4. The pacs.002 answers you will see

| `TxSts` | Meaning | Your state machine |
|---|---|---|
| `ACTC` | accepted, technical validation passed | keep waiting |
| `PDNG` | pending at the receiving participant | keep waiting |
| `ACSC` | **settled** — the money moved | `SETTLED` |
| `ACCC` | settled *and* credited to the creditor | `SETTLED` (confirmation) |
| `ACWP` | accepted without posting — settled, posting pending downstream | `SETTLED`, expect a follow-up |
| `BLCK` | funds blocked downstream (post-settlement) | `SETTLED`; downstream information |
| `RJCT` | rejected — by the service (proprietary reason, e.g. `E990`) or by the receiver (external code, e.g. `AC04`) | `REJECTED` |

Two subtleties worth engraving:

- **`ACSC` is service-only.** Participants never send it; if you are building
  the receive side, your accept is `ACTC`/`ACCC`.
- **`ACWP` is money-moved.** The settlement happened; what is pending is the
  receiver's posting. A later relay (`ACCC`, `BLCK`, or even `RJCT` followed
  by a pacs.004 return) completes the story — your ledger must already count
  the funds as gone.

## 5. The answer is asynchronous. Design for it

Over MQ there is no request/response: you PUT the pacs.008 on your send queue
and, some time later, a pacs.002 appears on your receive queue. Everything
between those two moments is a state your system must represent honestly:

```
CREATED → VALIDATED → SUBMITTED → ACK_PENDING → SETTLED | REJECTED
                                       └──────→ TIMEOUT_UNRESOLVED
```

Three disciplines make this survivable in production:

1. **Idempotency at the northbound edge.** Your API callers retry; the wire
   must not. Creation requires an idempotency key, and resubmitting one
   returns the payment as it stands.
2. **Outbox between decision and wire.** The `Submitted` event and the wire
   bytes become durable in one transaction; a publisher drains the outbox and
   records the handoff. A crash between the two never invents or loses a
   payment.
3. **No answer ≠ failure.** `ACK_PENDING` beyond the presumed timeout becomes
   `TIMEOUT_UNRESOLVED` — a work item resolved by a **pacs.028 status
   request**, never by resending the pacs.008. Chapter 2 is entirely about
   this, because it is where real money is lost.

## 6. Run it

The whole flow, locally, in three terminals:

```sh
# 1. The simulator (HTTP dev mode and MQ mode on the same port)
docker compose up --build          # sim on :8080, gateway on :8090 (MQ mode)

# 2. Submit a payment through the gateway's idempotent REST API
curl -s -X POST http://localhost:8090/payments \
  -H "content-type: application/json" \
  -H "Idempotency-Key: demo-0001" \
  -d '{ "sender_reference": "DEMO0001", "amount_cents": 125000,
        "creditor_agent_routing_number": "091000019",
        "debtor_name": "Jane", "debtor_account": "123456789012",
        "creditor_name": "John", "creditor_account": "987654321000",
        "category_purpose": "CONS" }'

# 3. Watch it settle (the advice arrives via the MQ receive queue)
curl -s http://localhost:8090/payments/demo-0001
```

Amount triggers steer the scenario (`.11` reject, `.33` timeout, `.66`
ACWP-then-ACCC, …) — see the [simulator README](../../simulator/README.md).
Chapter 4 maps these scenarios to the Customer Testing Program's test cases.

## 7. What this chapter deliberately skipped

- The no-answer case in depth — [chapter 2](02-timeout-reconciliation.md).
- Message signing and key management — chapter 3, blocked on the Technical
  Specifications ([#14](https://github.com/joaoabuenosi/fednow-oss/issues/14)).
- Returns (pacs.004) and return requests (camt.056/camt.029) — a future
  chapter; the messages are already implemented in `fednow-core`.
