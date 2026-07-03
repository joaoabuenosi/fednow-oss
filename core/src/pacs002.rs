//! Typed model and parser for pacs.002.001.10 (FIToFIPaymentStatusReportV10).
//!
//! In the FedNow flow this message travels in two directions: the receiving
//! participant answers a pacs.008 with an accept/reject pacs.002, and the FedNow
//! Service advises the sender of the final outcome (settled / rejected). The
//! model covers the fields those flows use; unknown elements are ignored.
//!
//! As in [`crate::pacs008`], required-vs-optional mirrors the schema cardinality,
//! and everything the schema marks optional stays `Option` here even when the
//! FedNow profile requires it — profile requirements are reported by
//! [`crate::validate::validate_pacs002`] instead, so structurally complete but
//! profile-deficient messages can still be inspected.

use serde::{Deserialize, Serialize};

use crate::error::ParseError;

/// XML namespace of the message version this module targets.
pub const NAMESPACE: &str = "urn:iso:std:iso:20022:tech:xsd:pacs.002.001.10";

/// Parse a pacs.002.001.10 document from XML text.
pub fn parse(xml: &str) -> Result<Document, ParseError> {
    Ok(quick_xml::de::from_str(xml)?)
}

/// `<Document>` — root element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Captured so validation can check the message version (`@xmlns`).
    #[serde(rename = "@xmlns", skip_serializing_if = "Option::is_none")]
    pub xmlns: Option<String>,
    #[serde(rename = "FIToFIPmtStsRpt")]
    pub fi_to_fi_payment_status_report: FIToFIPaymentStatusReportV10,
}

/// `<FIToFIPmtStsRpt>` — the status report body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FIToFIPaymentStatusReportV10 {
    #[serde(rename = "GrpHdr")]
    pub group_header: GroupHeader,
    #[serde(rename = "OrgnlGrpInfAndSts", skip_serializing_if = "Option::is_none")]
    pub original_group_information_and_status: Option<OriginalGroupHeader>,
    #[serde(rename = "TxInfAndSts", default, skip_serializing_if = "Vec::is_empty")]
    pub transaction_information_and_status: Vec<PaymentTransaction>,
}

/// `<GrpHdr>`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupHeader {
    #[serde(rename = "MsgId")]
    pub message_identification: String,
    #[serde(rename = "CreDtTm")]
    pub creation_date_time: String,
}

/// `<OrgnlGrpInfAndSts>` — identifies the original message being reported on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OriginalGroupHeader {
    #[serde(rename = "OrgnlMsgId")]
    pub original_message_identification: String,
    #[serde(rename = "OrgnlMsgNmId")]
    pub original_message_name_identification: String,
    #[serde(rename = "GrpSts", skip_serializing_if = "Option::is_none")]
    pub group_status: Option<String>,
}

/// `<TxInfAndSts>` — status of one original transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentTransaction {
    /// Optional in the base schema; the FedNow profiles require it.
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
    /// Optional in the schema; the FedNow profile requires it (see validation).
    #[serde(rename = "TxSts", skip_serializing_if = "Option::is_none")]
    pub transaction_status: Option<String>,
    #[serde(rename = "StsRsnInf", default, skip_serializing_if = "Vec::is_empty")]
    pub status_reason_information: Vec<StatusReasonInformation>,
    #[serde(rename = "AccptncDtTm", skip_serializing_if = "Option::is_none")]
    pub acceptance_date_time: Option<String>,
    /// Date (`Dt`) choice; used by the FedNow service advice when settled.
    #[serde(rename = "FctvIntrBkSttlmDt", skip_serializing_if = "Option::is_none")]
    pub effective_interbank_settlement_date: Option<DateChoice>,
    #[serde(rename = "ClrSysRef", skip_serializing_if = "Option::is_none")]
    pub clearing_system_reference: Option<String>,
    /// Optional in the base schema; the FedNow profiles require it.
    #[serde(rename = "InstgAgt", skip_serializing_if = "Option::is_none")]
    pub instructing_agent: Option<crate::pacs008::BranchAndFinancialInstitutionIdentification>,
    /// Optional in the base schema; the FedNow profiles require it.
    #[serde(rename = "InstdAgt", skip_serializing_if = "Option::is_none")]
    pub instructed_agent: Option<crate::pacs008::BranchAndFinancialInstitutionIdentification>,
}

/// `<OrgnlGrpInf>` — identifies the original message inside a transaction entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OriginalGroupInformation {
    #[serde(rename = "OrgnlMsgId")]
    pub original_message_identification: String,
    #[serde(rename = "OrgnlMsgNmId")]
    pub original_message_name_identification: String,
    /// Optional in the base schema; the FedNow profiles require it.
    #[serde(rename = "OrgnlCreDtTm", skip_serializing_if = "Option::is_none")]
    pub original_creation_date_time: Option<String>,
}

/// `<Dt>`-only date choice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateChoice {
    #[serde(rename = "Dt", skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

/// `<StsRsnInf>` — why a transaction has its status (mandatory context on rejects).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusReasonInformation {
    #[serde(rename = "Rsn", skip_serializing_if = "Option::is_none")]
    pub reason: Option<StatusReason>,
    #[serde(rename = "AddtlInf", default, skip_serializing_if = "Vec::is_empty")]
    pub additional_information: Vec<String>,
}

/// `<Rsn>` — external code or proprietary reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusReason {
    #[serde(rename = "Cd", skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(rename = "Prtry", skip_serializing_if = "Option::is_none")]
    pub proprietary: Option<String>,
}

impl StatusReasonInformation {
    /// True when the entry actually names a reason (code or proprietary).
    pub fn has_reason(&self) -> bool {
        self.reason
            .as_ref()
            .is_some_and(|r| r.code.is_some() || r.proprietary.is_some())
    }
}
