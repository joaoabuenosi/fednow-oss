//! fednow-core — ISO 20022 message parsing, validation, construction and signing
//! for FedNow send-side integrations.
//!
//! Current scope (milestone M1): parse and validate pacs.008.001.08
//! (FIToFICustomerCreditTransferV08).
//!
//! Validation happens in two layers:
//! 1. **Structural** — [`pacs008::parse`] fails if required elements are missing or
//!    the XML is malformed (the typed model mirrors the schema's required/optional
//!    cardinality).
//! 2. **Rules** — [`validate::validate_pacs008`] checks XSD facets (lengths, patterns,
//!    enumerations), ISO 20022 cross-field rules and FedNow profile rules (USD only,
//!    ABA routing-number checksums, settlement method CLRG, ...). It returns *all*
//!    violations, not just the first, so callers can report a complete diagnosis.
//!
//! Full XSD validation against the official schema runs in CI via `xmllint` when the
//! schema is vendored — see `core/schemas/README.md`.

pub mod error;
pub mod pacs008;
pub mod validate;

pub use error::ParseError;
pub use validate::{validate_pacs008, RuleSource, ValidationIssue};
