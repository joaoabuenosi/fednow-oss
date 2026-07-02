//! Typed model and parser for pacs.008.001.08 (FIToFICustomerCreditTransferV08).
//!
//! The model covers the subset of the schema that FedNow customer credit transfers
//! use in practice; unknown elements are ignored by the deserializer, so richer
//! messages still parse. Field names follow the ISO 20022 long names; `serde`
//! renames map them to the XML short tags.
//!
//! Required vs optional fields mirror the schema cardinality: a missing required
//! element is a [`crate::ParseError`], not a validation issue.

use serde::{Deserialize, Serialize};

use crate::error::ParseError;

/// XML namespace of the message version this module targets.
pub const NAMESPACE: &str = "urn:iso:std:iso:20022:tech:xsd:pacs.008.001.08";

/// Parse a pacs.008.001.08 document from XML text.
pub fn parse(xml: &str) -> Result<Document, ParseError> {
    Ok(quick_xml::de::from_str(xml)?)
}

/// `<Document>` — root element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Captured so validation can check the message version (`@xmlns`).
    #[serde(rename = "@xmlns", skip_serializing_if = "Option::is_none")]
    pub xmlns: Option<String>,
    #[serde(rename = "FIToFICstmrCdtTrf")]
    pub fi_to_fi_customer_credit_transfer: FIToFICustomerCreditTransferV08,
}

/// `<FIToFICstmrCdtTrf>` — the credit transfer message body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FIToFICustomerCreditTransferV08 {
    #[serde(rename = "GrpHdr")]
    pub group_header: GroupHeader,
    #[serde(rename = "CdtTrfTxInf")]
    pub credit_transfer_transaction_information: Vec<CreditTransferTransaction>,
}

/// `<GrpHdr>`
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementInstruction {
    #[serde(rename = "SttlmMtd")]
    pub settlement_method: String,
    #[serde(rename = "ClrSys", skip_serializing_if = "Option::is_none")]
    pub clearing_system: Option<ClearingSystemIdentificationChoice>,
}

/// `<ClrSys>`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearingSystemIdentificationChoice {
    #[serde(rename = "Cd", skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// `<CdtTrfTxInf>` — one credit transfer transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditTransferTransaction {
    #[serde(rename = "PmtId")]
    pub payment_identification: PaymentIdentification,
    #[serde(rename = "IntrBkSttlmAmt")]
    pub interbank_settlement_amount: ActiveCurrencyAndAmount,
    #[serde(rename = "IntrBkSttlmDt", skip_serializing_if = "Option::is_none")]
    pub interbank_settlement_date: Option<String>,
    #[serde(rename = "ChrgBr")]
    pub charge_bearer: String,
    #[serde(rename = "Dbtr")]
    pub debtor: PartyIdentification,
    #[serde(rename = "DbtrAcct", skip_serializing_if = "Option::is_none")]
    pub debtor_account: Option<CashAccount>,
    #[serde(rename = "DbtrAgt")]
    pub debtor_agent: BranchAndFinancialInstitutionIdentification,
    #[serde(rename = "CdtrAgt")]
    pub creditor_agent: BranchAndFinancialInstitutionIdentification,
    #[serde(rename = "Cdtr")]
    pub creditor: PartyIdentification,
    #[serde(rename = "CdtrAcct", skip_serializing_if = "Option::is_none")]
    pub creditor_account: Option<CashAccount>,
}

/// `<PmtId>`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentIdentification {
    #[serde(rename = "InstrId", skip_serializing_if = "Option::is_none")]
    pub instruction_identification: Option<String>,
    #[serde(rename = "EndToEndId")]
    pub end_to_end_identification: String,
    #[serde(rename = "UETR", skip_serializing_if = "Option::is_none")]
    pub uetr: Option<String>,
}

/// `<IntrBkSttlmAmt Ccy="...">` — amount kept as text; numeric facets are checked
/// during validation so the original lexical form is preserved for signing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveCurrencyAndAmount {
    #[serde(rename = "@Ccy")]
    pub currency: String,
    #[serde(rename = "$text")]
    pub value: String,
}

/// `<Dbtr>` / `<Cdtr>`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartyIdentification {
    #[serde(rename = "Nm", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `<DbtrAgt>` / `<CdtrAgt>`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchAndFinancialInstitutionIdentification {
    #[serde(rename = "FinInstnId")]
    pub financial_institution_identification: FinancialInstitutionIdentification,
}

/// `<FinInstnId>`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialInstitutionIdentification {
    #[serde(rename = "BICFI", skip_serializing_if = "Option::is_none")]
    pub bicfi: Option<String>,
    #[serde(rename = "ClrSysMmbId", skip_serializing_if = "Option::is_none")]
    pub clearing_system_member_identification: Option<ClearingSystemMemberIdentification>,
}

/// `<ClrSysMmbId>` — for FedNow this carries the ABA routing number (`MmbId`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearingSystemMemberIdentification {
    #[serde(rename = "ClrSysId", skip_serializing_if = "Option::is_none")]
    pub clearing_system_identification: Option<ClearingSystemIdentification>,
    #[serde(rename = "MmbId")]
    pub member_identification: String,
}

/// `<ClrSysId>`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearingSystemIdentification {
    #[serde(rename = "Cd", skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// `<DbtrAcct>` / `<CdtrAcct>`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashAccount {
    #[serde(rename = "Id")]
    pub identification: AccountIdentification,
}

/// `<Id>` under a cash account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountIdentification {
    #[serde(rename = "Othr", skip_serializing_if = "Option::is_none")]
    pub other: Option<GenericAccountIdentification>,
}

/// `<Othr>`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericAccountIdentification {
    #[serde(rename = "Id")]
    pub identification: String,
}
