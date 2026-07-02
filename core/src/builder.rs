//! Construction of FedNow-profile pacs.008 messages.
//!
//! [`Pacs008Builder`] produces a [`pacs008::Document`] that already satisfies the
//! FedNow profile rules enforced by [`crate::validate::validate_pacs008`]:
//! settlement method CLRG, charge bearer SLEV, USD with cent precision, and a
//! single credit-transfer transaction per message (the FedNow flow is one
//! transaction per pacs.008). Amounts are taken in cents (`u64`) so no floating
//! point ever touches money.
//!
//! The builder does not invent data: message id, end-to-end id, creation
//! date-time and both routing numbers are explicit inputs. Nothing is defaulted
//! from clocks or randomness, which keeps construction deterministic and
//! testable; generating UETRs and timestamps is the caller's concern (the
//! gateway will own that).

use crate::pacs008::{
    AccountIdentification, ActiveCurrencyAndAmount, BranchAndFinancialInstitutionIdentification,
    CashAccount, ClearingSystemIdentification, ClearingSystemMemberIdentification,
    CreditTransferTransaction, Document, FIToFICustomerCreditTransferV08,
    FinancialInstitutionIdentification, GenericAccountIdentification, GroupHeader,
    PartyIdentification, PaymentIdentification, SettlementInstruction, NAMESPACE,
};
use thiserror::Error;

/// Errors produced while serializing a built document to XML.
#[derive(Debug, Error)]
pub enum BuildError {
    #[error("XML serialization failed: {0}")]
    Serialize(#[from] quick_xml::SeError),
}

/// Builder for a FedNow customer credit transfer (pacs.008.001.08).
///
/// Required inputs come in through [`Pacs008Builder::new`]; everything else is
/// optional and added with the fluent setters. Call [`Pacs008Builder::build`]
/// for the typed document or [`Pacs008Builder::to_xml`] for the wire form.
#[derive(Debug, Clone)]
pub struct Pacs008Builder {
    message_identification: String,
    creation_date_time: String,
    end_to_end_identification: String,
    amount_cents: u64,
    debtor_agent_routing_number: String,
    creditor_agent_routing_number: String,
    instruction_identification: Option<String>,
    uetr: Option<String>,
    interbank_settlement_date: Option<String>,
    debtor_name: Option<String>,
    debtor_account: Option<String>,
    creditor_name: Option<String>,
    creditor_account: Option<String>,
}

impl Pacs008Builder {
    /// Start a builder with the fields every FedNow credit transfer needs.
    ///
    /// `creation_date_time` is an ISO 8601 date-time (e.g.
    /// `2026-07-02T15:30:00Z`); `amount_cents` is the interbank settlement
    /// amount in USD cents (e.g. `125000` for $1,250.00).
    pub fn new(
        message_identification: impl Into<String>,
        creation_date_time: impl Into<String>,
        end_to_end_identification: impl Into<String>,
        amount_cents: u64,
        debtor_agent_routing_number: impl Into<String>,
        creditor_agent_routing_number: impl Into<String>,
    ) -> Self {
        Self {
            message_identification: message_identification.into(),
            creation_date_time: creation_date_time.into(),
            end_to_end_identification: end_to_end_identification.into(),
            amount_cents,
            debtor_agent_routing_number: debtor_agent_routing_number.into(),
            creditor_agent_routing_number: creditor_agent_routing_number.into(),
            instruction_identification: None,
            uetr: None,
            interbank_settlement_date: None,
            debtor_name: None,
            debtor_account: None,
            creditor_name: None,
            creditor_account: None,
        }
    }

    pub fn instruction_identification(mut self, v: impl Into<String>) -> Self {
        self.instruction_identification = Some(v.into());
        self
    }

    pub fn uetr(mut self, v: impl Into<String>) -> Self {
        self.uetr = Some(v.into());
        self
    }

    /// Interbank settlement date (`YYYY-MM-DD`).
    pub fn interbank_settlement_date(mut self, v: impl Into<String>) -> Self {
        self.interbank_settlement_date = Some(v.into());
        self
    }

    pub fn debtor_name(mut self, v: impl Into<String>) -> Self {
        self.debtor_name = Some(v.into());
        self
    }

    /// Debtor account number (carried as `Othr/Id`).
    pub fn debtor_account(mut self, v: impl Into<String>) -> Self {
        self.debtor_account = Some(v.into());
        self
    }

    pub fn creditor_name(mut self, v: impl Into<String>) -> Self {
        self.creditor_name = Some(v.into());
        self
    }

    /// Creditor account number (carried as `Othr/Id`).
    pub fn creditor_account(mut self, v: impl Into<String>) -> Self {
        self.creditor_account = Some(v.into());
        self
    }

    /// Build the typed document.
    ///
    /// The result is not implicitly validated — run
    /// [`crate::validate::validate_pacs008`] on it (the builder's own tests do),
    /// so callers get the same diagnosis path for built and parsed messages.
    pub fn build(&self) -> Document {
        Document {
            xmlns: Some(NAMESPACE.to_string()),
            fi_to_fi_customer_credit_transfer: FIToFICustomerCreditTransferV08 {
                group_header: GroupHeader {
                    message_identification: self.message_identification.clone(),
                    creation_date_time: self.creation_date_time.clone(),
                    number_of_transactions: "1".to_string(),
                    settlement_information: SettlementInstruction {
                        settlement_method: "CLRG".to_string(),
                        clearing_system: None,
                    },
                },
                credit_transfer_transaction_information: vec![CreditTransferTransaction {
                    payment_identification: PaymentIdentification {
                        instruction_identification: self.instruction_identification.clone(),
                        end_to_end_identification: self.end_to_end_identification.clone(),
                        uetr: self.uetr.clone(),
                    },
                    interbank_settlement_amount: ActiveCurrencyAndAmount {
                        currency: "USD".to_string(),
                        value: format_cents(self.amount_cents),
                    },
                    interbank_settlement_date: self.interbank_settlement_date.clone(),
                    charge_bearer: "SLEV".to_string(),
                    debtor: PartyIdentification {
                        name: self.debtor_name.clone(),
                    },
                    debtor_account: self.debtor_account.as_ref().map(|id| account(id)),
                    debtor_agent: agent(&self.debtor_agent_routing_number),
                    creditor_agent: agent(&self.creditor_agent_routing_number),
                    creditor: PartyIdentification {
                        name: self.creditor_name.clone(),
                    },
                    creditor_account: self.creditor_account.as_ref().map(|id| account(id)),
                }],
            },
        }
    }

    /// Build and serialize to the XML wire form (with XML declaration).
    pub fn to_xml(&self) -> Result<String, BuildError> {
        let body = quick_xml::se::to_string(&self.build())?;
        Ok(format!(r#"<?xml version="1.0" encoding="UTF-8"?>{body}"#))
    }
}

/// `1250.00`-style lexical form from cents; never floating point.
fn format_cents(cents: u64) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
}

fn agent(routing_number: &str) -> BranchAndFinancialInstitutionIdentification {
    BranchAndFinancialInstitutionIdentification {
        financial_institution_identification: FinancialInstitutionIdentification {
            bicfi: None,
            clearing_system_member_identification: Some(ClearingSystemMemberIdentification {
                clearing_system_identification: Some(ClearingSystemIdentification {
                    code: Some("USABA".to_string()),
                }),
                member_identification: routing_number.to_string(),
            }),
        },
    }
}

fn account(id: &str) -> CashAccount {
    CashAccount {
        identification: AccountIdentification {
            other: Some(GenericAccountIdentification {
                identification: id.to_string(),
            }),
        },
    }
}
