//! Typed model and parser for head.001.001.02 (Business Application Header, "BAH").
//!
//! Every business message exchanged with the FedNow Service carries a BAH: it
//! identifies sender and receiver (routing numbers under `FIId`), names the
//! enclosed message (`MsgDefIdr`, e.g. `pacs.008.001.08`) and carries the
//! message signature in the `Sgntr` envelope. Note that the schema constrains
//! `Sgntr` content to the W3C XMLDSig namespace (`xs:any` over
//! `http://www.w3.org/2000/09/xmldsig#`, lax).
//!
//! The typed model records *whether* a signature is present; the signature
//! bytes themselves must never be round-tripped through a model (re-serialization
//! drift breaks digests), so [`sgntr_raw`] extracts the raw inner XML of the
//! `Sgntr` element straight from the wire text for signing/verification work.

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
