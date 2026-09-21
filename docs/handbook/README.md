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

## A note on the example identifiers

**Every routing number in this project is fictitious and cannot route.** They all
begin with `99`, which is outside every block the ABA assigns to real institutions
(`01`–`12` Federal Reserve districts, `21`–`32` thrifts, `61`–`72` electronic, `80`
traveler's cheques), so none of them can ever belong to a bank:

| Example value | Stands for | ABA check digit |
|---|---|---|
| `991000009` | the sending institution — "you" | passes |
| `992000008` | the receiving institution — the creditor agent | passes |
| `993000007` | the service application the messages are addressed to | passes |
| `991000008`, `992000007` | the same values with the last digit altered | **fails, on purpose** |

The first three pass the check digit deliberately: the FedNow profile validator
enforces it (`fednow.aba.checksum`), so a number that failed would be rejected before
any example could demonstrate anything. The unassignable `99` prefix is what makes them
safe, not the check digit. The last two exist precisely so the negative tests have
something that fails.

The same rule covers every other identifier here: account numbers, names (`Jane
Example`, `John Example`), message ids and UETRs are invented. Nothing in this
repository is taken from a real institution, a real payment, or an access-restricted
specification — see the repository's `AGENTS.md`.
