//! fednow-core — ISO 20022 message parsing, validation, construction and signing
//! for FedNow send-side integrations.
//!
//! Current scope: parse and validate pacs.008.001.08
//! (FIToFICustomerCreditTransferV08), pacs.002.001.10
//! (FIToFIPaymentStatusReportV10, both FedNow directions), pacs.028.001.03
//! (FIToFIPaymentStatusRequestV03) and head.001.001.02 (Business Application
//! Header); build FedNow-conformant pacs.008.
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
pub mod camt029;
pub mod camt056;
pub mod error;
pub mod head001;
pub mod pacs002;
pub mod pacs004;
pub mod pacs008;
pub mod pacs028;
pub mod validate;

pub use error::ParseError;
pub use validate::{
    validate_camt029, validate_camt056, validate_head001, validate_pacs002,
    validate_pacs002_direction, validate_pacs004, validate_pacs008, validate_pacs028,
    Pacs002Direction, RuleSource, ValidationIssue,
};
