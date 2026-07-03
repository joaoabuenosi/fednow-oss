# fednow-core

ISO 20022 message library for **FedNow® Service send-side integrations**:
parsing, validation against the real Release 1 profiles, and message
construction — the foundation of the [fednow-oss](https://github.com/joaoabuenosi/fednow-oss)
toolchain (simulator, gateway, conformance suite).

## What it does

- **Typed models + parsers** for pacs.008.001.08, pacs.002.001.10 (both FedNow
  directions), pacs.028.001.03, pacs.004.001.10, camt.056.001.08,
  camt.029.001.09 and head.001.001.02 (Business Application Header).
- **Profile validation, all violations at once**: XSD facets, ISO 20022
  cross-field rules and the FedNow Release 1 profile (FedNow message-id shape,
  ABA checksums, `FDN`/`CLRG`/`SLEV`/`USABA`, USD cent amounts, service
  identifier `021150706`, direction-dependent BAH rules) — each issue with a
  stable machine-readable code. Calibrated against the 81 official Release 1
  sample messages.
- **Builders** (`Pacs008Builder`, `Pacs002Builder`, `Pacs028Builder`,
  `Head001Builder`): deterministic construction, amounts in cents (`u64`,
  never floats), no clocks or randomness in the domain.
- **MQ technical envelope** (`envelope`): byte-exact `split()`, typed
  `parse()` and `build()` for `FedNowIncoming`/`FedNowOutgoing` wrappers,
  plus cross-validation between BAH and Document.

## Example

```rust
use fednow_core::{pacs008, validate_pacs008};

let doc = pacs008::parse(&xml)?;
let issues = validate_pacs008(&doc);
for issue in &issues {
    println!("{} at {}: {}", issue.code, issue.path, issue.message);
}
```

## What it deliberately does not do (yet)

Message **signing**: the FedNow signature wire format lives in the Fed's
access-controlled Technical Specifications, distributed during participant
onboarding — tracked in
[issue #14](https://github.com/joaoabuenosi/fednow-oss/issues/14). The
envelope layer already preserves wire bytes exactly so signing can slot in
without re-serialization.

No telemetry, no phone-home, no credentials. Fixtures use fictitious data.

Apache-2.0.
