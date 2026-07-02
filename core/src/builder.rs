//! Construction of FedNow-profile pacs.008 messages.
//!
//! [`Pacs008Builder`] produces a [`pacs008::Document`](crate::pacs008::Document)
//! that satisfies the FedNow profile rules enforced by
//! [`crate::validate::validate_pacs008`]: settlement method CLRG over clearing
//! system FDN, charge bearer SLEV, USD with cent precision, one transaction per
//! message, USABA-schemed agents. Amounts are taken in cents (`u64`) so no
//! floating point ever touches money.
//!
//! The builder does not invent data: message id, end-to-end id, creation
//! date-time and both routing numbers are explicit inputs. Nothing is defaulted
//! from clocks or randomness, which keeps construction deterministic and
//! testable; generating UETRs and timestamps is the caller's concern (the
//! gateway will own that). Instructing/instructed agents default to the
//! debtor/creditor agents (the direct-participant case) and can be overridden.

use crate::pacs008::{
    AccountIdentification, ActiveCurrencyAndAmount, BranchAndFinancialInstitutionIdentification,
    CashAccount, ClearingSystemIdentification, ClearingSystemIdentificationChoice,
    ClearingSystemMemberIdentification, CodeOrProprietaryChoice, CreditTransferTransaction,
    Document, FIToFICustomerCreditTransferV08, FinancialInstitutionIdentification,
    GenericAccountIdentification, GroupHeader, PartyIdentification, PaymentIdentification,
    PaymentTypeInformation, SettlementInstruction, NAMESPACE,
};
use thiserror::Error;

/// Compose a FedNow message identification: `CCYYMMDD` + 9-character connection
/// party id + sender reference (1..18 alphanumerics).
pub fn fednow_message_id(
    date_yyyymmdd: &str,
    connection_party_id: &str,
    reference: &str,
) -> String {
    format!("{date_yyyymmdd}{connection_party_id}{reference}")
}

/// Errors produced while serializing a built document to XML.
#[derive(Debug, Error)]
pub enum BuildError {
    #[error("XML serialization failed: {0}")]
    Serialize(#[from] quick_xml::SeError),
}

/// Builder for a FedNow customer credit transfer (pacs.008.001.08).
///
/// Required inputs come in through [`Pacs008Builder::new`]; the remaining
/// FedNow-mandatory fields (`interbank_settlement_date`, accounts,
/// `local_instrument`, `category_purpose`) and the optional ones are added with
/// the fluent setters. Call [`Pacs008Builder::build`] for the typed document or
/// [`Pacs008Builder::to_xml`] for the wire form; run
/// [`crate::validate::validate_pacs008`] on the result to check completeness —
/// built and parsed messages share one diagnosis path.
#[derive(Debug, Clone)]
pub struct Pacs008Builder {
    message_identification: String,
    creation_date_time: String,
    end_to_end_identification: String,
    amount_cents: u64,
    debtor_agent_routing_number: String,
    creditor_agent_routing_number: String,
    instructing_agent_routing_number: Option<String>,
    instructed_agent_routing_number: Option<String>,
    instruction_identification: Option<String>,
    uetr: Option<String>,
    interbank_settlement_date: Option<String>,
    local_instrument: Option<String>,
    category_purpose: Option<String>,
    debtor_name: Option<String>,
    debtor_account: Option<String>,
    creditor_name: Option<String>,
    creditor_account: Option<String>,
}

impl Pacs008Builder {
    /// Start a builder with the identification core of a FedNow credit transfer.
    ///
    /// `message_identification` must follow the FedNow pattern — see
    /// [`fednow_message_id`]. `creation_date_time` is an ISO 8601 date-time
    /// (e.g. `2026-07-02T15:30:00Z`); `amount_cents` is the interbank settlement
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
            instructing_agent_routing_number: None,
            instructed_agent_routing_number: None,
            instruction_identification: None,
            uetr: None,
            interbank_settlement_date: None,
            local_instrument: None,
            category_purpose: None,
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

    /// Interbank settlement date (`YYYY-MM-DD`). FedNow-mandatory.
    pub fn interbank_settlement_date(mut self, v: impl Into<String>) -> Self {
        self.interbank_settlement_date = Some(v.into());
        self
    }

    /// `PmtTpInf/LclInstrm/Prtry`. FedNow-mandatory; the concrete code values
    /// come from the FedNow code list (Technical Specifications).
    pub fn local_instrument(mut self, v: impl Into<String>) -> Self {
        self.local_instrument = Some(v.into());
        self
    }

    /// `PmtTpInf/CtgyPurp/Prtry`. FedNow-mandatory; the concrete code values
    /// come from the FedNow code list (Technical Specifications).
    pub fn category_purpose(mut self, v: impl Into<String>) -> Self {
        self.category_purpose = Some(v.into());
        self
    }

    /// Override the instructing agent (defaults to the debtor agent).
    pub fn instructing_agent_routing_number(mut self, v: impl Into<String>) -> Self {
        self.instructing_agent_routing_number = Some(v.into());
        self
    }

    /// Override the instructed agent (defaults to the creditor agent).
    pub fn instructed_agent_routing_number(mut self, v: impl Into<String>) -> Self {
        self.instructed_agent_routing_number = Some(v.into());
        self
    }

    pub fn debtor_name(mut self, v: impl Into<String>) -> Self {
        self.debtor_name = Some(v.into());
        self
    }

    /// Debtor account number (carried as `Othr/Id`). FedNow-mandatory.
    pub fn debtor_account(mut self, v: impl Into<String>) -> Self {
        self.debtor_account = Some(v.into());
        self
    }

    pub fn creditor_name(mut self, v: impl Into<String>) -> Self {
        self.creditor_name = Some(v.into());
        self
    }

    /// Creditor account number (carried as `Othr/Id`). FedNow-mandatory.
    pub fn creditor_account(mut self, v: impl Into<String>) -> Self {
        self.creditor_account = Some(v.into());
        self
    }

    /// Build the typed document.
    pub fn build(&self) -> Document {
        let payment_type_information =
            if self.local_instrument.is_some() || self.category_purpose.is_some() {
                Some(PaymentTypeInformation {
                    service_level: None,
                    local_instrument: self.local_instrument.as_ref().map(|v| proprietary(v)),
                    category_purpose: self.category_purpose.as_ref().map(|v| proprietary(v)),
                })
            } else {
                None
            };

        let instructing = self
            .instructing_agent_routing_number
            .as_deref()
            .unwrap_or(&self.debtor_agent_routing_number);
        let instructed = self
            .instructed_agent_routing_number
            .as_deref()
            .unwrap_or(&self.creditor_agent_routing_number);

        Document {
            xmlns: Some(NAMESPACE.to_string()),
            fi_to_fi_customer_credit_transfer: FIToFICustomerCreditTransferV08 {
                group_header: GroupHeader {
                    message_identification: self.message_identification.clone(),
                    creation_date_time: self.creation_date_time.clone(),
                    number_of_transactions: "1".to_string(),
                    settlement_information: SettlementInstruction {
                        settlement_method: "CLRG".to_string(),
                        clearing_system: Some(ClearingSystemIdentificationChoice {
                            code: Some("FDN".to_string()),
                        }),
                    },
                },
                credit_transfer_transaction_information: vec![CreditTransferTransaction {
                    payment_identification: PaymentIdentification {
                        instruction_identification: self.instruction_identification.clone(),
                        end_to_end_identification: self.end_to_end_identification.clone(),
                        uetr: self.uetr.clone(),
                    },
                    payment_type_information,
                    interbank_settlement_amount: ActiveCurrencyAndAmount {
                        currency: "USD".to_string(),
                        value: format_cents(self.amount_cents),
                    },
                    interbank_settlement_date: self.interbank_settlement_date.clone(),
                    charge_bearer: "SLEV".to_string(),
                    instructing_agent: Some(agent(instructing)),
                    instructed_agent: Some(agent(instructed)),
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

fn proprietary(v: &str) -> CodeOrProprietaryChoice {
    CodeOrProprietaryChoice {
        code: None,
        proprietary: Some(v.to_string()),
    }
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
