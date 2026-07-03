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

## head.001.001.02 (BAH)

Removed: `CharSet`, `PssblDplct`, `Prty`, **`Sgntr`** (signature is out-of-band).
Mandatory: `MktPrctc` (fixed registry URL + FedNow id). `Fr`/`To`: `FIId` only,
`ClrSysMmbId` with `MmbId` only (no `ClrSysId`). `CpyDplct`: `DUPL` only.
`Rltd` max 1.

From the guideline PDF (textual rules):
- **FedNow Service application identifier: `021150706`.** Participant-sent
  messages carry it in `To` (and the service uses it in `Fr`; `To` may be
  `PINGREPLY` in admi.011 ping responses).
- **Service-only elements** (must not be sent by participants): `BizPrcgDt`,
  `CpyDplct` (retrieved-message deliveries), `Rltd` (retrieval responses).
- **`BizSvc` must not be used in Release 1** (no codes defined).
- **`CreDt`**: UTC or local time with UTC offset — a timezone is required, but
  `Z` is not (the earlier UTC-only reading was too strict).
- **`MktPrctc/Id` must match the enclosed message**: `frb.fednow.01` for all,
  except camt.029.001.09 (`rrr`/`irr`/`rcr`) and camt.052.001.08
  (`aat`/`aad`/`aba`).

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

**Transaction statuses (from the Release 1 sample set, 50 pacs.002 samples):**
ACTC, ACCC, ACWP, PDNG, BLCK, RJCT in both directions; **ACSC is
service-advice-only** (never sent by participants). Proprietary reason codes
(`Rsn/Prtry`, e.g. E000/E990) appear only in service advices, confirming the
Cd-only participant rule. **Conformance check (July 2026):** all 50 samples
validate clean for their direction with fednow-core.

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
