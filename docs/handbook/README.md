# FedNow Integration Handbook

Practical, production-oriented guidance for building **send** capability on the
FedNow Service — written alongside the code in this repository, and runnable
against [`fednow-sim`](../../simulator/).

Documentation is a product here: every claim is either sourced from public
Federal Reserve documentation, derived from the official Release 1 usage
guidelines, or demonstrated by an executable example.

## Chapters

| # | Chapter | Status |
|---|---|---|
| 1 | [The credit transfer flow](01-credit-transfer-flow.md) (pacs.008 → pacs.002) | draft |
| 2 | [Timeout reconciliation](02-timeout-reconciliation.md) — the hard case | draft |
| 3 | Message signing and key management | blocked on [#14](https://github.com/joaoabuenosi/fednow-oss/issues/14) |
| 4 | [From zero to the Customer Testing Program](04-zero-to-ctp.md) | draft (credit transfers) |
| 5 | [Returns](05-returns.md) — camt.056 / camt.029 / pacs.004 | draft |
