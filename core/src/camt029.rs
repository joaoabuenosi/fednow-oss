//! Typed model and parser for camt.029.001.09 (ResolutionOfInvestigationV09).
//!
//! In FedNow this is the **return request response**: the answer to a
//! camt.056. The `Sts/Conf` confirmation code tells the outcome — the
//! Release 1 profile uses `IPAY` (return will be paid), `RJCR` (request
//! rejected), `PDCR` (pending) and `PECR` (partially executed); rejections
//! carry a reason in `CxlStsRsnInf`.

use serde::Deserialize;

use crate::camt056::{CancellationReason, CaseAssignment};
use crate::error::ParseError;
use crate::pacs002::OriginalGroupInformation;

/// XML namespace of the message version this module targets.
pub const NAMESPACE: &str = "urn:iso:std:iso:20022:tech:xsd:camt.029.001.09";

/// Parse a camt.029.001.09 document from XML text.
pub fn parse(xml: &str) -> Result<Document, ParseError> {
    Ok(quick_xml::de::from_str(xml)?)
}

/// `<Document>` — root element.
#[derive(Debug, Clone, Deserialize)]
pub struct Document {
    #[serde(rename = "@xmlns")]
    pub xmlns: Option<String>,
    #[serde(rename = "RsltnOfInvstgtn")]
    pub resolution: ResolutionOfInvestigationV09,
}

/// `<RsltnOfInvstgtn>`
#[derive(Debug, Clone, Deserialize)]
pub struct ResolutionOfInvestigationV09 {
    #[serde(rename = "Assgnmt")]
    pub assignment: CaseAssignment,
    #[serde(rename = "RslvdCase")]
    pub resolved_case: Option<ResolvedCase>,
    #[serde(rename = "Sts")]
    pub status: InvestigationStatus,
    #[serde(rename = "CxlDtls", default)]
    pub cancellation_details: Vec<CancellationDetails>,
}

/// `<RslvdCase>`
#[derive(Debug, Clone, Deserialize)]
pub struct ResolvedCase {
    #[serde(rename = "Id")]
    pub identification: String,
}

/// `<Sts>` — the confirmation code.
#[derive(Debug, Clone, Deserialize)]
pub struct InvestigationStatus {
    #[serde(rename = "Conf")]
    pub confirmation: Option<String>,
}

/// `<CxlDtls>`
#[derive(Debug, Clone, Deserialize)]
pub struct CancellationDetails {
    #[serde(rename = "TxInfAndSts", default)]
    pub transaction_information: Vec<PaymentTransaction>,
}

/// `<TxInfAndSts>` — the underlying transaction and its cancellation status.
#[derive(Debug, Clone, Deserialize)]
pub struct PaymentTransaction {
    #[serde(rename = "OrgnlGrpInf")]
    pub original_group_information: Option<OriginalGroupInformation>,
    #[serde(rename = "OrgnlInstrId")]
    pub original_instruction_identification: Option<String>,
    #[serde(rename = "OrgnlEndToEndId")]
    pub original_end_to_end_identification: Option<String>,
    #[serde(rename = "OrgnlUETR")]
    pub original_uetr: Option<String>,
    #[serde(rename = "CxlStsRsnInf", default)]
    pub cancellation_status_reason_information: Vec<CancellationStatusReason>,
}

/// `<CxlStsRsnInf>`
#[derive(Debug, Clone, Deserialize)]
pub struct CancellationStatusReason {
    #[serde(rename = "Rsn")]
    pub reason: Option<CancellationReason>,
    #[serde(rename = "AddtlInf", default)]
    pub additional_information: Vec<String>,
}
