//! Typed model and parser for head.001.001.02 (Business Application Header, "BAH").
//!
//! Every business message exchanged with the FedNow Service carries a BAH: it
//! identifies sender and receiver (connection party identifiers under `FIId`)
//! and names the enclosed message (`MsgDefIdr`, e.g. `pacs.008.001.08`).
//!
//! On signatures: the *base* ISO schema has a `Sgntr` envelope (constrained to
//! the W3C XMLDSig namespace), but the FedNow profile removes it — FedNow
//! message signatures travel outside the XML business message (see
//! `docs/design/message-signing.md`). The model still parses `Sgntr` as a
//! presence marker so validation can flag it, and [`sgntr_raw`] can extract its
//! raw bytes for generic ISO 20022 tooling; re-serialization is avoided on
//! purpose (drift breaks digests).

use serde::Deserialize;

use crate::error::ParseError;
use crate::pacs008::BranchAndFinancialInstitutionIdentification;

/// XML namespace of the header version this module targets.
pub const NAMESPACE: &str = "urn:iso:std:iso:20022:tech:xsd:head.001.001.02";

/// Parse a head.001.001.02 `<AppHdr>` from XML text.
pub fn parse(xml: &str) -> Result<AppHdr, ParseError> {
    Ok(quick_xml::de::from_str(xml)?)
}

/// Extract the raw inner XML of the top-level `<Sgntr>` element, if present.
///
/// Returns the exact byte slice between `<Sgntr>` and `</Sgntr>` as it appears
/// on the wire — no re-serialization, so digests computed over it are stable.
/// Only a `Sgntr` that is a direct child of the root element is considered.
pub fn sgntr_raw(xml: &str) -> Option<&str> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(xml);
    let mut depth = 0usize;
    let mut inner_start: Option<usize> = None;
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                if depth == 1 && inner_start.is_none() && e.local_name().as_ref() == b"Sgntr" {
                    inner_start = Some(reader.buffer_position() as usize);
                }
                depth += 1;
            }
            Ok(Event::End(e)) => {
                depth = depth.saturating_sub(1);
                if depth == 1 && e.local_name().as_ref() == b"Sgntr" {
                    if let Some(start) = inner_start {
                        return Some(&xml[start..pos_before]);
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

/// `<AppHdr>` — root element (BusinessApplicationHeaderV02).
#[derive(Debug, Clone, Deserialize)]
pub struct AppHdr {
    /// Captured so validation can check the header version (`@xmlns`).
    #[serde(rename = "@xmlns")]
    pub xmlns: Option<String>,
    #[serde(rename = "Fr")]
    pub from: Party44Choice,
    #[serde(rename = "To")]
    pub to: Party44Choice,
    #[serde(rename = "BizMsgIdr")]
    pub business_message_identifier: String,
    #[serde(rename = "MsgDefIdr")]
    pub message_definition_identifier: String,
    #[serde(rename = "BizSvc")]
    pub business_service: Option<String>,
    /// Optional in the base schema; the FedNow profile requires it with a fixed
    /// registry and a `frb.fednow[.xxx].01` identifier (see validation).
    #[serde(rename = "MktPrctc")]
    pub market_practice: Option<ImplementationSpecification>,
    #[serde(rename = "CreDt")]
    pub creation_date: String,
    #[serde(rename = "CpyDplct")]
    pub copy_duplicate: Option<String>,
    #[serde(rename = "PssblDplct")]
    pub possible_duplicate: Option<String>,
    /// Presence of the signature envelope. Content is intentionally not modeled —
    /// use [`sgntr_raw`] on the wire text to obtain the signature bytes.
    #[serde(rename = "Sgntr")]
    pub signature: Option<SignatureEnvelope>,
}

/// `<MktPrctc>` — implementation specification the message conforms to.
#[derive(Debug, Clone, Deserialize)]
pub struct ImplementationSpecification {
    #[serde(rename = "Regy")]
    pub registry: String,
    #[serde(rename = "Id")]
    pub identification: String,
}

/// `<Sgntr>` — presence marker; inner XMLDSig content is skipped by the
/// deserializer on purpose (see module docs).
#[derive(Debug, Clone, Deserialize)]
pub struct SignatureEnvelope {}

/// `<Fr>` / `<To>` — Party44Choice: exactly one of `OrgId` or `FIId`.
///
/// Modeled as two options because serde has no native XSD-choice; the validator
/// enforces the exactly-one rule.
#[derive(Debug, Clone, Deserialize)]
pub struct Party44Choice {
    #[serde(rename = "OrgId")]
    pub organisation: Option<PartyIdentification135>,
    /// FedNow participants identify themselves here, with the routing number in
    /// `FinInstnId/ClrSysMmbId/MmbId`. Shares the shape used by pacs.008 agents
    /// (the underlying ISO type is the same).
    #[serde(rename = "FIId")]
    pub financial_institution: Option<BranchAndFinancialInstitutionIdentification>,
}

/// `<OrgId>` under Party44Choice (subset).
#[derive(Debug, Clone, Deserialize)]
pub struct PartyIdentification135 {
    #[serde(rename = "Nm")]
    pub name: Option<String>,
}
