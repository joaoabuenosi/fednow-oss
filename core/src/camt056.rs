//! Typed model and parser for camt.056.001.08 (FIToFIPaymentCancellationRequestV08).
//!
//! In FedNow this is the **return request**: "please give the money back" for
//! a payment that already settled. The receiving side answers with a
//! camt.029 (return request response) and, when honoring it, a pacs.004
//! (payment return). Profile facts derived from the ReturnRequest usage
//! guideline export.

use serde::Deserialize;

use crate::error::ParseError;
use crate::pacs002::OriginalGroupInformation;
use crate::pacs008::{ActiveCurrencyAndAmount, BranchAndFinancialInstitutionIdentification};

/// XML namespace of the message version this module targets.
pub const NAMESPACE: &str = "urn:iso:std:iso:20022:tech:xsd:camt.056.001.08";

/// Parse a camt.056.001.08 document from XML text.
pub fn parse(xml: &str) -> Result<Document, ParseError> {
    Ok(quick_xml::de::from_str(xml)?)
}

/// `<Document>` — root element.
#[derive(Debug, Clone, Deserialize)]
pub struct Document {
    #[serde(rename = "@xmlns")]
    pub xmlns: Option<String>,
    #[serde(rename = "FIToFIPmtCxlReq")]
    pub cancellation_request: FIToFIPaymentCancellationRequestV08,
}

/// `<FIToFIPmtCxlReq>`
#[derive(Debug, Clone, Deserialize)]
pub struct FIToFIPaymentCancellationRequestV08 {
    #[serde(rename = "Assgnmt")]
    pub assignment: CaseAssignment,
    #[serde(rename = "Case")]
    pub case: Option<Case>,
    #[serde(rename = "Undrlyg", default)]
    pub underlying: Vec<UnderlyingTransaction>,
}

/// `<Assgnmt>` — who is asking whom, when.
#[derive(Debug, Clone, Deserialize)]
pub struct CaseAssignment {
    #[serde(rename = "Id")]
    pub identification: String,
    #[serde(rename = "Assgnr")]
    pub assigner: Party40Choice,
    #[serde(rename = "Assgne")]
    pub assignee: Party40Choice,
    #[serde(rename = "CreDtTm")]
    pub creation_date_time: String,
}

/// `<Case>` — the investigation case identity.
#[derive(Debug, Clone, Deserialize)]
pub struct Case {
    #[serde(rename = "Id")]
    pub identification: String,
}

/// `Agt`/`Pty` choice; FedNow uses agents (routing numbers).
#[derive(Debug, Clone, Deserialize)]
pub struct Party40Choice {
    #[serde(rename = "Agt")]
    pub agent: Option<BranchAndFinancialInstitutionIdentification>,
}

/// `<Undrlyg>`
#[derive(Debug, Clone, Deserialize)]
pub struct UnderlyingTransaction {
    #[serde(rename = "TxInf", default)]
    pub transaction_information: Vec<PaymentTransaction>,
}

/// `<TxInf>` — the transaction whose return is requested.
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
    #[serde(rename = "OrgnlIntrBkSttlmAmt")]
    pub original_interbank_settlement_amount: Option<ActiveCurrencyAndAmount>,
    #[serde(rename = "OrgnlIntrBkSttlmDt")]
    pub original_interbank_settlement_date: Option<String>,
    #[serde(rename = "CxlRsnInf", default)]
    pub cancellation_reason_information: Vec<CancellationReasonInformation>,
}

/// `<CxlRsnInf>` — why the return is requested (e.g. DUPL, FRAD, TECH).
#[derive(Debug, Clone, Deserialize)]
pub struct CancellationReasonInformation {
    #[serde(rename = "Rsn")]
    pub reason: Option<CancellationReason>,
    #[serde(rename = "AddtlInf", default)]
    pub additional_information: Vec<String>,
}

/// `<Rsn>` — Cd-only in the FedNow profile.
#[derive(Debug, Clone, Deserialize)]
pub struct CancellationReason {
    #[serde(rename = "Cd")]
    pub code: Option<String>,
    #[serde(rename = "Prtry")]
    pub proprietary: Option<String>,
}
