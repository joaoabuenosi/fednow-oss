# Chapter 4 — From zero to the Customer Testing Program

*Status: draft (credit transfer family) · sources: FedNow Service ISO 20022
Readiness Portal Guide v1.4 (chapter 3), Release 1 sample set.*

Direct FedNow participants and service providers must pass the **Customer
Testing Program (CTP)** before going live. The Fed's Readiness Portal Guide
defines the message test scenarios family by family; this chapter maps each
official scenario to a `fednow-sim` behavior, so you can rehearse the exact
choreography locally — before certification windows, credentials or
connectivity exist.

The simulator plays **everything on the far side of your sender**: the FedNow
Service and the receiving participant (Bank B) combined. Your system under
test plays Bank A.

## Customer credit transfers (Readiness Portal Guide, chapter 3)

| Official scenario | What happens on the wire | Rehearse with `fednow-sim` | What your sender must do |
|---|---|---|---|
| **1 — Happy Path** | Bank B accepts (`ACTC`), the service settles and advises both sides (`ACSC`), Bank B confirms crediting (`ACCC`) | default amounts → `ACSC` advice | `ACK_PENDING → SETTLED`; treat a later `ACCC` as confirmation, not as a state change |
| **2 — Rejection by the FedNow Service** | The service itself rejects, reason is a **proprietary code** (`Rsn/Prtry`) | amount ending `.55` → `RJCT` with `Prtry E990` | `ACK_PENDING → REJECTED`; the proprietary code namespace is the service's, log it verbatim |
| **3 — Rejection by the FedNow participant** | Bank B rejects with an **external code** (`Rsn/Cd`, e.g. `AC04` account closed); the service relays it | amount ending `.11` → `RJCT` with `Cd AC04` | `ACK_PENDING → REJECTED` with the business reason surfaced to the originator |
| **4 — Accept without Posting** | Bank B answers `ACWP` (needs time); the service settles and advises `ACWP`; later Bank B follows up with `PDNG`, `ACCC`, `BLCK` or `RJCT` | amount ending `.66` → `ACWP` now, `ACCC` follow-up retrievable with a pacs.028 (config `follow_up` chooses `accc`/`blck`/`rjct`/`pdng`); `.22` → plain `ACWP` | Money **has settled** — but posting is unresolved; track the follow-up statuses, a late `RJCT` here means a payment return (pacs.004) is coming |
| **5 — Payment Status Request (to the service)** | No advice arrived; Bank A sends pacs.028; the service answers with the pacs.002 of the original | amount ending `.33` (timeout), then POST a pacs.028 | The whole of [chapter 2](02-timeout-reconciliation.md) |
| **6 — Payment Status Request (to a participant)** | Bank A queries Bank B (via the service) about a payment Bank B received | same pacs.028 flow — the simulator answers as the far side either way | Same reconciliation discipline, applied to inbound flows |

Notes:

- The official sample messages for every step of these scenarios are on the
  Fed's MyStandards Readiness Portal (free account) — `fednow-core` parses and
  validates all 81 Release 1 credit-transfer samples clean, so what you build
  against the simulator is what the portal validator expects.
- Scenario 4's follow-up advice is queued in the simulator's ledger and
  retrieved by polling with a pacs.028 (the HTTP dev mode cannot push);
  MQ mode will deliver it unsolicited, as the real service does.
- Scenario 2's real proprietary reason codes are defined in the
  access-controlled Technical Specifications
  ([#14](https://github.com/joaoabuenosi/fednow-oss/issues/14)); the simulator
  uses `E990` (observed in the official sample set) as a stand-in.

## Other message families

The Readiness Portal Guide covers, in the same style: payment returns
(chapter 4), liquidity management transfers (5), request for payment (6),
information requests (7), system messages (8) and account reporting (9). The
mapping tables for those will land here as the corresponding messages arrive
in `fednow-core` and scenarios in `fednow-sim`.

## The path itself

For orientation, the full onboarding journey a direct participant walks
(Operating Procedures / Readiness Guide): Operating Circular 8 paperwork →
electronic connection profiles → key pairs established → **CTP test scripts
executed per message family** → operational readiness certification → go-live.
This handbook's goal is that by the time you touch the real test environment,
every scenario above is boring — you have run it a hundred times locally.
