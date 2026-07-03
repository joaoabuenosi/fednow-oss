//! Typed model and parser for pacs.028.001.03 (FIToFIPaymentStatusRequestV03).
//!
//! In the FedNow flow this is the reconciliation primitive: a participant asks
//! the service (or another participant) to (re)send the processing status of a
//! previously sent payment message. The gateway's reconciler resolves
//! ACK_PENDING payments with this message — never with blind resends.
//!
//! FedNow usage constraints (enforced by
//! [`crate::validate::validate_pacs028`]): one transaction per request, the
//! original message fully identified (`OrgnlGrpInf` incl. creation date-time),
//! instructing/instructed agents present. The service only answers for the
//! current or prior calendar day, and requests should not be sent before the
//! presumed timeout of the original instruction.

use serde::{Deserialize, Serialize};

use crate::error::ParseError;
use crate::pacs002::OriginalGroupInformation;
use crate::pacs008::BranchAndFinancialInstitutionIdentification;

/// XML namespace of the message version this module targets.
pub const NAMESPACE: &str = "urn:iso:std:iso:20022:tech:xsd:pacs.028.001.03";

/// Parse a pacs.028.001.03 document from XML text.
pub fn parse(xml: &str) -> Result<Document, ParseError> {
    Ok(quick_xml::de::from_str(xml)?)
}

/// `<Document>` — root element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Captured so validation can check the message version (`@xmlns`).
    #[serde(rename = "@xmlns", skip_serializing_if = "Option::is_none")]
    pub xmlns: Option<String>,
    #[serde(rename = "FIToFIPmtStsReq")]
    pub fi_to_fi_payment_status_request: FIToFIPaymentStatusRequestV03,
}

/// `<FIToFIPmtStsReq>` — the status request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FIToFIPaymentStatusRequestV03 {
    #[serde(rename = "GrpHdr")]
    pub group_header: GroupHeader,
    #[serde(rename = "TxInf", default, skip_serializing_if = "Vec::is_empty")]
    pub transaction_information: Vec<PaymentTransaction>,
}

/// `<GrpHdr>`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupHeader {
    #[serde(rename = "MsgId")]
    pub message_identification: String,
    #[serde(rename = "CreDtTm")]
    pub creation_date_time: String,
}

/// `<TxInf>` — identifies one original transaction being queried.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentTransaction {
    /// Optional in the base schema; the FedNow profile requires it.
    #[serde(rename = "OrgnlGrpInf", skip_serializing_if = "Option::is_none")]
    pub original_group_information: Option<OriginalGroupInformation>,
    #[serde(rename = "OrgnlInstrId", skip_serializing_if = "Option::is_none")]
    pub original_instruction_identification: Option<String>,
    #[serde(rename = "OrgnlEndToEndId", skip_serializing_if = "Option::is_none")]
    pub original_end_to_end_identification: Option<String>,
    #[serde(rename = "OrgnlTxId", skip_serializing_if = "Option::is_none")]
    pub original_transaction_identification: Option<String>,
    #[serde(rename = "OrgnlUETR", skip_serializing_if = "Option::is_none")]
    pub original_uetr: Option<String>,
    /// Optional in the base schema; the FedNow profile requires it.
    #[serde(rename = "InstgAgt", skip_serializing_if = "Option::is_none")]
    pub instructing_agent: Option<BranchAndFinancialInstitutionIdentification>,
    /// Optional in the base schema; the FedNow profile requires it.
    #[serde(rename = "InstdAgt", skip_serializing_if = "Option::is_none")]
    pub instructed_agent: Option<BranchAndFinancialInstitutionIdentification>,
}
