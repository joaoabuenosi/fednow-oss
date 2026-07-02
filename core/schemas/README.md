# XML Schemas (XSD)

This directory holds the ISO 20022 schemas used for full XSD validation of test
fixtures in CI (`xsd-validate` job — it activates automatically when a schema file
is present here, and skips politely when it is not).

## Status

No schema is vendored yet. `fednow-core` currently enforces XSD *facets*
programmatically (see `core/src/validate.rs`), which covers the milestone-M1 rules
but is not a substitute for validating against the official schema.

## How to obtain the schemas

1. **ISO 20022 base schemas** (e.g. `pacs.008.001.08.xsd`): free download from
   [iso20022.org](https://www.iso20022.org/iso-20022-message-definitions) — the
   2019-era versions used by FedNow live in the *messages archive* section.
   Download the message set zip, extract the XSD, and drop it in this directory
   with its canonical filename (`pacs.008.001.08.xsd`).
2. **FedNow-specific schema variants**: the Federal Reserve distributes its
   restricted message specifications through its MyStandards portal to onboarding
   participants. **Those files must not be committed here** unless their license
   explicitly allows redistribution — check before vendoring anything from that
   portal.

## Target message set

| Message | Version (FedNow, ISO 2019 set) |
|---|---|
| FI to FI Customer Credit Transfer | pacs.008.001.08 |
| Payment Status Report | pacs.002.001.10 |
| Payment Status Request | pacs.028.001.03 |
| Creditor Payment Activation Request (RFP) | pain.013.001.07 |
| Creditor Payment Activation Request Status | pain.014.001.07 |
| FI to FI Payment Cancellation Request | camt.056.001.08 |
| Resolution of Investigation | camt.029.001.09 |
