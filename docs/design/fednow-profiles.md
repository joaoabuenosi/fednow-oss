# FedNow Service Release 1 — profile facts implemented by fednow-core

*Interoperability facts derived from the FedNow Service Release 1 usage
guidelines (MyStandards). This file records only the minimal facts needed to
implement and never reproduces guideline content — see the licensing note in
`message-signing.md`.*

## Shared FedNow lexical types

| Concept | Rule |
|---|---|
| Message identification (participant-sent messages) | `CCYYMMDD` (8 digits) + connection party id (9 alphanumerics) + sender reference (1–18 alphanumerics); total 18–35 chars |
| Routing number (agents in payment messages) | exactly 9 digits (`RoutingNumber_FRS`), USABA scheme; ABA check digit applies |
| Connection party identifier (BAH `Fr`/`To`) | 9 uppercase alphanumerics (RTN, ETI, or FedNow-assigned) |
| Message name identifier | `aaaa.nnn.001.nn` (variant fixed at 001) |
| Market practice id (BAH `MktPrctc/Id`) | `frb.fednow.01` or `frb.fednow.<3 lowercase>.01` |

## head.001.001.02 (BAH) — implemented in PR #7

Removed: `CharSet`, `PssblDplct`, `Prty`, **`Sgntr`** (signature is out-of-band).
Mandatory: `MktPrctc` (fixed registry URL + FedNow id). `Fr`/`To`: `FIId` only,
`ClrSysMmbId` with `MmbId` only (no `ClrSysId`). `CpyDplct`: `DUPL` only.
`Rltd` max 1. `CreDt` normalised to UTC.

## pacs.008.001.08 (customer credit transfer, participant → service)

- `GrpHdr`: `MsgId` = FedNow message id pattern; `NbOfTxs` fixed `"1"`;
  `SttlmInf` = `SttlmMtd` (CLRG) + **`ClrSys/Cd` fixed `FDN` (mandatory)**.
- `CdtTrfTxInf` exactly 1. Mandatory: `PmtId` (`EndToEndId`; `UETR` optional),
  **`PmtTpInf`** (`LclInstrm/Prtry` + `CtgyPurp/Prtry` mandatory, `SvcLvl`
  optional), `IntrBkSttlmAmt`, **`IntrBkSttlmDt`**, `ChrgBr` (SLEV only),
  **`InstgAgt`**, **`InstdAgt`**, `Dbtr`, **`DbtrAcct`**, `DbtrAgt`, `CdtrAgt`,
  `Cdtr`, **`CdtrAcct`**.
- Amount: USD only; **max 14 total digits, max 2 fraction digits**, ≥ 0.
- Agents carry `ClrSysMmbId` with `ClrSysId/Cd` = `USABA` + 9-digit routing number.
- Optional survivors: `UltmtDbtr`, `InitgPty`, `UltmtCdtr`, `Purp`, `RltdRmtInf`, `RmtInf`.
- Code values (from the Release 1 sample set, uniform across all 31 samples):
  `LclInstrm/Prtry` = **`FDNA`**; `CtgyPurp/Prtry` ∈ {**`CONS`**, **`BIZZ`**}
  (consumer / business). The full code list in the Technical Specifications may
  extend the category purposes.
- **Conformance check (July 2026):** all 30 structurally valid Release 1
  pacs.008 samples parse and validate clean with fednow-core; the one
  intentionally malformed sample (MessageReject scenario) is correctly rejected
  at parse. Run locally with
  `cargo run -p fednow-core --example validate -- <file.xml>`.

## pacs.002.001.10 — TWO distinct profiles

Common shape: `GrpHdr` (`MsgId` + `CreDtTm` only) and **exactly one**
`TxInfAndSts` with mandatory `OrgnlGrpInf` (`OrgnlMsgId`, `OrgnlMsgNmId` FRS
pattern, **`OrgnlCreDtTm`**), mandatory `TxSts`, mandatory `InstgAgt`/`InstdAgt`
(USABA + routing number). No group-level `OrgnlGrpInfAndSts`.

**ParticipantPaymentStatus (participant → service):**
`MsgId` = FedNow message id pattern; `StsRsnInf` max 1, `Rsn` mandatory inside
it and restricted to `Cd` (no `Prtry`); no `AccptncDtTm` / `FctvIntrBkSttlmDt`.

**FedNowPaymentStatus (service → participant):**
`MsgId` = plain Max35Text; `StsRsnInf` unbounded, `Rsn/Cd` or `Rsn/Prtry`;
optional `AccptncDtTm` and `FctvIntrBkSttlmDt` (date-only choice).

## pacs.028.001.03 (payment status request, participant-sent)

`GrpHdr`: `MsgId` = FedNow message id pattern + `CreDtTm`. **Exactly one**
`TxInf`: mandatory `OrgnlGrpInf` (as above), optional `OrgnlInstrId` /
`OrgnlEndToEndId` / `OrgnlTxId` / `OrgnlUETR`, mandatory `InstgAgt`/`InstdAgt`
(USABA + routing). Usage: query only for current/prior calendar day, and not
before the presumed timeout of the original instruction.

## Open items

- Exact `LclInstrm`/`CtgyPurp` code values (FedNow code list).
- Message signing transport (detached signature outside the XML) — see
  `message-signing.md`.
- 2026 Enhanced Messages Release (Q4 2026) may bump message versions.
