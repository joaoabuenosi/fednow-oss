//! fednow-core — ISO 20022 message parsing, validation, construction and signing
//! for FedNow send-side integrations.
//!
//! Current scope: parse and validate pacs.008.001.08
//! (FIToFICustomerCreditTransferV08), pacs.002.001.10
//! (FIToFIPaymentStatusReportV10) and head.001.001.02 (Business Application
//! Header — carries sender/receiver routing and the message signature envelope).
//!
//! Validation happens in two layers:
//! 1. **Structural** — [`pacs008::parse`] fails if required elements are missing or
//!    the XML is malformed (the typed model mirrors the schema's required/optional
//!    cardinality).
//! 2. **Rules** — [`validate::validate_pacs008`] / [`validate::validate_pacs002`]
//!    check XSD facets (lengths, patterns, enumerations), ISO 20022 cross-field
//!    rules and FedNow profile rules (USD only, ABA routing-number checksums,
//!    settlement method CLRG, mandatory reject reasons, ...). They return *all*
//!    violations, not just the first, so callers can report a complete diagnosis.
//!
//! Full XSD validation against the official schema runs in CI via `xmllint` when the
//! schema is vendored — see `core/schemas/README.md`.

pub mod builder;
pub mod error;
pub mod head001;
pub mod pacs002;
pub mod pacs008;
pub mod validate;

pub use error::ParseError;
pub use validate::{
    validate_head001, validate_pacs002, validate_pacs008, RuleSource, ValidationIssue,
};
