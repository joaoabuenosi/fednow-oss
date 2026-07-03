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

use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppHdr {
    /// Captured so validation can check the header version (`@xmlns`).
    #[serde(rename = "@xmlns", skip_serializing_if = "Option::is_none")]
    pub xmlns: Option<String>,
    #[serde(rename = "Fr")]
    pub from: Party44Choice,
    #[serde(rename = "To")]
    pub to: Party44Choice,
    #[serde(rename = "BizMsgIdr")]
    pub business_message_identifier: String,
    #[serde(rename = "MsgDefIdr")]
    pub message_definition_identifier: String,
    #[serde(rename = "BizSvc", skip_serializing_if = "Option::is_none")]
    pub business_service: Option<String>,
    /// Optional in the base schema; the FedNow profile requires it with a fixed
    /// registry and a `frb.fednow[.xxx].01` identifier (see validation).
    #[serde(rename = "MktPrctc", skip_serializing_if = "Option::is_none")]
    pub market_practice: Option<ImplementationSpecification>,
    #[serde(rename = "CreDt")]
    pub creation_date: String,
    /// Service-delivered messages only; participants must not send it.
    #[serde(rename = "BizPrcgDt", skip_serializing_if = "Option::is_none")]
    pub business_processing_date: Option<String>,
    #[serde(rename = "CpyDplct", skip_serializing_if = "Option::is_none")]
    pub copy_duplicate: Option<String>,
    #[serde(rename = "PssblDplct", skip_serializing_if = "Option::is_none")]
    pub possible_duplicate: Option<String>,
    /// Presence of the signature envelope. Content is intentionally not modeled —
    /// use [`sgntr_raw`] on the wire text to obtain the signature bytes.
    /// Never serialized: the FedNow profile removes `Sgntr`, and this crate
    /// must not emit an empty envelope where a signature belongs.
    #[serde(rename = "Sgntr", skip_serializing)]
    pub signature: Option<SignatureEnvelope>,
    /// Related header(s); only present in messages the FedNow Service delivers
    /// in response to retrieval/status requests. Content not modeled.
    #[serde(rename = "Rltd", default, skip_serializing)]
    pub related: Vec<RelatedHeader>,
}

/// `<Rltd>` — presence marker (inner BAH content is skipped by the deserializer).
#[derive(Debug, Clone, Deserialize)]
pub struct RelatedHeader {}

/// `<MktPrctc>` — implementation specification the message conforms to.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Party44Choice {
    #[serde(rename = "OrgId", skip_serializing_if = "Option::is_none")]
    pub organisation: Option<PartyIdentification135>,
    /// FedNow participants identify themselves here, with the routing number in
    /// `FinInstnId/ClrSysMmbId/MmbId`. Shares the shape used by pacs.008 agents
    /// (the underlying ISO type is the same).
    #[serde(rename = "FIId", skip_serializing_if = "Option::is_none")]
    pub financial_institution: Option<BranchAndFinancialInstitutionIdentification>,
}

/// `<OrgId>` under Party44Choice (subset).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartyIdentification135 {
    #[serde(rename = "Nm", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}
