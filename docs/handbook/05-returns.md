# Chapter 5 — Returns: when settled money has to come back

*Status: draft · sources: FedNow Service Release 1 usage guidelines
(PaymentReturn, ReturnRequest, ReturnRequestResponse), Operating
Procedures — plus the message implementations in `fednow-core`.*

FedNow settlement is **final and irrevocable**. There is no "cancel": once
the pacs.002 says `ACSC`, the money moved. Everything in this chapter is
therefore a *new* movement of funds in the opposite direction — with its own
messages, its own settlement, and the receiving side's consent in the loop.

Three messages implement the whole flow, all in `fednow-core`:

| Message | Role |
|---|---|
| `camt.056.001.08` | **Return request** — "please send that payment back" |
| `camt.029.001.09` | **Response** — paid / refused / pending / partially |
| `pacs.004.001.10` | **Payment return** — the actual funds moving back |

## 1. The two seats at the table

As a **send-side** institution (this project's focus) you sit in both seats
at different times:

- **You want money back** (duplicate submission, wrong beneficiary,
  fraud discovered after settlement): you send a **camt.056** and wait for
  a **camt.029** — and, if the other side agrees, a **pacs.004** arrives
  returning the funds.
- **Someone wants money back from you** — or your earlier payment bounces
  *after* settlement (the post-`ACWP` rejection): a **pacs.004** simply
  arrives, and your ledger must absorb an inbound return tied to the
  original payment.

The second case is the sneaky one: `ACWP` meant "settled, posting pending" —
if the receiving bank later rejects the posting (closed account, compliance
hold), the money you already count as gone comes back through a pacs.004
referencing your original pacs.008. A gateway that doesn't correlate returns
to originals leaks reconciliation breaks.

## 2. camt.056 — asking

FedNow-profile facts that matter in practice (calibrated against the real
Release 1 guidelines — see `docs/design/fednow-profiles.md`):

- The request rides a **case assignment**: `Assgnmt/Id` in the FedNow
  message-id shape (unique per calendar day), assigner and assignee as
  routing-number agents.
- One transaction per request, identifying the original by `OrgnlGrpInf`
  (message id, `pacs.008.001.08`, creation date-time) plus the original
  amount and settlement date.
- **`CxlRsnInf/Rsn/Cd` is mandatory** — a coded reason (`DUPL` duplicate,
  `TECH` technical, `FRAD` fraud, `CUST` customer request, …). Free text
  goes in `AddtlInf`, but the code drives the receiving side's workflow.

A return request is a *request*. Ten days can pass; the answer can be no.
Model it like the timeout case: a work item with a deadline, not a state
your payment flow blocks on.

## 3. camt.029 — the answer

The response resolves the case with **`Sts/Conf`** restricted to four codes
in the FedNow profile:

| `Conf` | Meaning | What follows |
|---|---|---|
| `IPAY` | agreed, will pay | a **pacs.004** with the funds |
| `RJCR` | refused | nothing — the reason is mandatory (`CxlStsRsnInf`, e.g. `LEGL`, `NOAS`, `AC04`) |
| `PDCR` | pending decision | wait; a further camt.029 comes |
| `PECR` | partially executed | a pacs.004 for *part* of the amount |

Note the BAH detail that trips implementations: camt.029 responses use the
**3-letter market-practice contexts** (`frb.fednow.rrr.01` and friends), not
the plain `frb.fednow.01` — `validate_envelope`/`validate_head001` enforce
exactly that.

## 4. pacs.004 — the money actually moving

The return is a first-class settlement message, not an annotation:

- Its own FedNow message id, `NbOfTxs = 1`, `CLRG`/`FDN`, `ChrgBr = SLEV` —
  the same skeleton as a pacs.008.
- **`RtrdIntrBkSttlmAmt`** may be less than the original (partial return —
  matches `PECR`); the original amount and settlement date are mandatory
  context.
- **`RtrChain`** carries the parties of the *return* movement — debtor and
  creditor swap roles relative to the original. Getting this backwards is
  the classic implementation bug: the original creditor is now the debtor.
- **`RtrRsnInf`** is exactly one, code-only in the FedNow profile (`AC04`,
  `DUPL`, …), and **`OrgnlTxRef`** echoes the original payment-type
  information (`FDNA`, `CONS`/`BIZZ`).
- A return is itself acknowledged by a pacs.002 — it can settle, be
  rejected, or time out like any payment. The reconciliation discipline of
  [chapter 2](02-timeout-reconciliation.md) applies unchanged.

## 5. Try the pieces today

The messages validate end to end right now:

```sh
# The conformance corpus includes valid and deliberately broken
# camt.056 / camt.029 / pacs.004 vectors (24 vectors total):
cargo run -p fednow-conformance -- vectors

# Validate your own files against the FedNow Release 1 profiles:
cargo run -p fednow-conformance -- validate my-camt056.xml
```

What is **not** built yet — and tracked for a future milestone: return
scenarios in `fednow-sim` (an amount trigger that answers camt.056 with
each `Conf` outcome) and the gateway's inbound-return correlation. The
message layer they will stand on is done and calibrated.

## 6. Design rules worth stealing

1. **Returns are payments.** Same state machine shape, same outbox, same
   reconciler — a `pacs.004` you send deserves the same durability as a
   `pacs.008`.
2. **Correlate by original message id, always.** Every message in this
   chapter points back to the original `MsgId`; your store must answer
   "what happened around payment X?" with the full thread — the event
   sourcing in the gateway exists for exactly that question.
3. **Coded reasons in, coded reasons out.** `DUPL` from your ops tooling is
   actionable by the counterparty's machine; prose is not.
