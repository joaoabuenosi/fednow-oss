//! Typed model and parser for pacs.004.001.10 (PaymentReturnV10).
//!
//! A payment return moves previously settled funds back — the receiving side
//! returns a customer credit transfer it cannot keep (e.g. after a post-ACWP
//! rejection, or in answer to a return request, camt.056).
//!
//! The model covers the subset the FedNow flows use. FedNow-shared lexical
//! rules (message id shape, USABA agents, FDN clearing system, USD cent
//! amounts) are enforced by [`crate::validate::validate_pacs004`]; the exact
//! Release 1 profile (mandatory fields beyond the base schema, return reason
//! code set) is pending calibration against the PaymentReturn usage-guideline
//! export — rules derived so far are marked accordingly.

use serde::Deserialize;

use crate::error::ParseError;
use crate::pacs002::OriginalGroupInformation;
use crate::pacs008::{
    ActiveCurrencyAndAmount, BranchAndFinancialInstitutionIdentification,
    ClearingSystemIdentificationChoice,
};

/// XML namespace of the message version this module targets.
pub const NAMESPACE: &str = "urn:iso:std:iso:20022:tech:xsd:pacs.004.001.10";

/// Parse a pacs.004.001.10 document from XML text.
pub fn parse(xml: &str) -> Result<Document, ParseError> {
    Ok(quick_xml::de::from_str(xml)?)
}

/// `<Document>` — root element.
#[derive(Debug, Clone, Deserialize)]
pub struct Document {
    /// Captured so validation can check the message version (`@xmlns`).
    #[serde(rename = "@xmlns")]
    pub xmlns: Option<String>,
    #[serde(rename = "PmtRtr")]
    pub payment_return: PaymentReturnV10,
}

/// `<PmtRtr>` — the payment return body.
#[derive(Debug, Clone, Deserialize)]
pub struct PaymentReturnV10 {
    #[serde(rename = "GrpHdr")]
    pub group_header: GroupHeader,
    #[serde(rename = "TxInf", default)]
    pub transaction_information: Vec<PaymentTransaction>,
}

/// `<GrpHdr>`
#[derive(Debug, Clone, Deserialize)]
pub struct GroupHeader {
    #[serde(rename = "MsgId")]
    pub message_identification: String,
    #[serde(rename = "CreDtTm")]
    pub creation_date_time: String,
    #[serde(rename = "NbOfTxs")]
    pub number_of_transactions: String,
    #[serde(rename = "SttlmInf")]
    pub settlement_information: SettlementInstruction,
}

/// `<SttlmInf>`
#[derive(Debug, Clone, Deserialize)]
pub struct SettlementInstruction {
    #[serde(rename = "SttlmMtd")]
    pub settlement_method: String,
    #[serde(rename = "ClrSys")]
    pub clearing_system: Option<ClearingSystemIdentificationChoice>,
}

/// `<TxInf>` — one returned transaction.
#[derive(Debug, Clone, Deserialize)]
pub struct PaymentTransaction {
    #[serde(rename = "RtrId")]
    pub return_identification: Option<String>,
    /// Optional in the base schema; the FedNow flows identify the original
    /// message here.
    #[serde(rename = "OrgnlGrpInf")]
    pub original_group_information: Option<OriginalGroupInformation>,
    #[serde(rename = "OrgnlInstrId")]
    pub original_instruction_identification: Option<String>,
    #[serde(rename = "OrgnlEndToEndId")]
    pub original_end_to_end_identification: Option<String>,
    #[serde(rename = "OrgnlUETR")]
    pub original_uetr: Option<String>,
    #[serde(rename = "RtrdIntrBkSttlmAmt")]
    pub returned_interbank_settlement_amount: ActiveCurrencyAndAmount,
    #[serde(rename = "IntrBkSttlmDt")]
    pub interbank_settlement_date: Option<String>,
    #[serde(rename = "ChrgBr")]
    pub charge_bearer: Option<String>,
    #[serde(rename = "InstgAgt")]
    pub instructing_agent: Option<BranchAndFinancialInstitutionIdentification>,
    #[serde(rename = "InstdAgt")]
    pub instructed_agent: Option<BranchAndFinancialInstitutionIdentification>,
    #[serde(rename = "RtrRsnInf", default)]
    pub return_reason_information: Vec<ReturnReasonInformation>,
}

/// `<RtrRsnInf>` — why the payment is being returned.
#[derive(Debug, Clone, Deserialize)]
pub struct ReturnReasonInformation {
    #[serde(rename = "Rsn")]
    pub reason: Option<ReturnReason>,
    #[serde(rename = "AddtlInf", default)]
    pub additional_information: Vec<String>,
}

/// `<Rsn>` — external code or proprietary reason.
#[derive(Debug, Clone, Deserialize)]
pub struct ReturnReason {
    #[serde(rename = "Cd")]
    pub code: Option<String>,
    #[serde(rename = "Prtry")]
    pub proprietary: Option<String>,
}
