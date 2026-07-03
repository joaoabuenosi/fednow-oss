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

    /// Override `PmtTpInf/LclInstrm/Prtry` (defaults to `FDNA`, the value used
    /// by every Release 1 customer-credit-transfer sample).
    pub fn local_instrument(mut self, v: impl Into<String>) -> Self {
        self.local_instrument = Some(v.into());
        self
    }

    /// `PmtTpInf/CtgyPurp/Prtry`. FedNow-mandatory: `CONS` (consumer) or
    /// `BIZZ` (business) in the Release 1 sample set.
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
        let payment_type_information = Some(PaymentTypeInformation {
            service_level: None,
            local_instrument: Some(proprietary(
                self.local_instrument.as_deref().unwrap_or("FDNA"),
            )),
            category_purpose: self.category_purpose.as_ref().map(|v| proprietary(v)),
        });

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

/// Builder for a pacs.002 payment status report (both FedNow directions).
///
/// Constructs the service advice (`ACSC`/`RJCT`/`ACWP`/...) that `fednow-sim`
/// answers with, and equally the participant accept/reject response. Which
/// direction the result satisfies is decided by what you put in it — run
/// [`crate::validate::validate_pacs002_direction`] with the intended direction
/// (the builder does not guess).
#[derive(Debug, Clone)]
pub struct Pacs002Builder {
    message_identification: String,
    creation_date_time: String,
    original_message_identification: String,
    original_message_name_identification: String,
    original_creation_date_time: String,
    transaction_status: String,
    instructing_agent_routing_number: String,
    instructed_agent_routing_number: String,
    original_instruction_identification: Option<String>,
    original_end_to_end_identification: Option<String>,
    original_uetr: Option<String>,
    reason_code: Option<String>,
    reason_proprietary: Option<String>,
    additional_information: Option<String>,
    acceptance_date_time: Option<String>,
    effective_interbank_settlement_date: Option<String>,
}

impl Pacs002Builder {
    /// Start a status report answering the original message identified by
    /// `original_message_identification` / `original_creation_date_time`
    /// (a pacs.008 unless overridden with
    /// [`Pacs002Builder::original_message_name_identification`]).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        message_identification: impl Into<String>,
        creation_date_time: impl Into<String>,
        original_message_identification: impl Into<String>,
        original_creation_date_time: impl Into<String>,
        transaction_status: impl Into<String>,
        instructing_agent_routing_number: impl Into<String>,
        instructed_agent_routing_number: impl Into<String>,
    ) -> Self {
        Self {
            message_identification: message_identification.into(),
            creation_date_time: creation_date_time.into(),
            original_message_identification: original_message_identification.into(),
            original_message_name_identification: "pacs.008.001.08".to_string(),
            original_creation_date_time: original_creation_date_time.into(),
            transaction_status: transaction_status.into(),
            instructing_agent_routing_number: instructing_agent_routing_number.into(),
            instructed_agent_routing_number: instructed_agent_routing_number.into(),
            original_instruction_identification: None,
            original_end_to_end_identification: None,
            original_uetr: None,
            reason_code: None,
            reason_proprietary: None,
            additional_information: None,
            acceptance_date_time: None,
            effective_interbank_settlement_date: None,
        }
    }

    /// Override the original message name (defaults to `pacs.008.001.08`).
    pub fn original_message_name_identification(mut self, v: impl Into<String>) -> Self {
        self.original_message_name_identification = v.into();
        self
    }

    pub fn original_instruction_identification(mut self, v: impl Into<String>) -> Self {
        self.original_instruction_identification = Some(v.into());
        self
    }

    pub fn original_end_to_end_identification(mut self, v: impl Into<String>) -> Self {
        self.original_end_to_end_identification = Some(v.into());
        self
    }

    pub fn original_uetr(mut self, v: impl Into<String>) -> Self {
        self.original_uetr = Some(v.into());
        self
    }

    /// External status reason code (e.g. `AC04`). Mandatory context on rejects.
    pub fn reason_code(mut self, v: impl Into<String>) -> Self {
        self.reason_code = Some(v.into());
        self
    }

    /// Proprietary status reason (service advices only, e.g. `E000`).
    pub fn reason_proprietary(mut self, v: impl Into<String>) -> Self {
        self.reason_proprietary = Some(v.into());
        self
    }

    pub fn additional_information(mut self, v: impl Into<String>) -> Self {
        self.additional_information = Some(v.into());
        self
    }

    /// Acceptance timestamp (service advices only).
    pub fn acceptance_date_time(mut self, v: impl Into<String>) -> Self {
        self.acceptance_date_time = Some(v.into());
        self
    }

    /// Effective settlement date, `YYYY-MM-DD` (service advices only).
    pub fn effective_interbank_settlement_date(mut self, v: impl Into<String>) -> Self {
        self.effective_interbank_settlement_date = Some(v.into());
        self
    }

    /// Build the typed document.
    pub fn build(&self) -> crate::pacs002::Document {
        use crate::pacs002::{
            DateChoice, Document as P2Document, FIToFIPaymentStatusReportV10, GroupHeader,
            OriginalGroupInformation, PaymentTransaction, StatusReason, StatusReasonInformation,
        };

        let status_reason_information = if self.reason_code.is_some()
            || self.reason_proprietary.is_some()
        {
            vec![StatusReasonInformation {
                reason: Some(StatusReason {
                    code: self.reason_code.clone(),
                    proprietary: self.reason_proprietary.clone(),
                }),
                additional_information: self.additional_information.clone().into_iter().collect(),
            }]
        } else {
            Vec::new()
        };

        P2Document {
            xmlns: Some(crate::pacs002::NAMESPACE.to_string()),
            fi_to_fi_payment_status_report: FIToFIPaymentStatusReportV10 {
                group_header: GroupHeader {
                    message_identification: self.message_identification.clone(),
                    creation_date_time: self.creation_date_time.clone(),
                },
                original_group_information_and_status: None,
                transaction_information_and_status: vec![PaymentTransaction {
                    original_group_information: Some(OriginalGroupInformation {
                        original_message_identification: self
                            .original_message_identification
                            .clone(),
                        original_message_name_identification: self
                            .original_message_name_identification
                            .clone(),
                        original_creation_date_time: Some(self.original_creation_date_time.clone()),
                    }),
                    original_instruction_identification: self
                        .original_instruction_identification
                        .clone(),
                    original_end_to_end_identification: self
                        .original_end_to_end_identification
                        .clone(),
                    original_transaction_identification: None,
                    original_uetr: self.original_uetr.clone(),
                    transaction_status: Some(self.transaction_status.clone()),
                    status_reason_information,
                    acceptance_date_time: self.acceptance_date_time.clone(),
                    effective_interbank_settlement_date: self
                        .effective_interbank_settlement_date
                        .as_ref()
                        .map(|d| DateChoice {
                            date: Some(d.clone()),
                        }),
                    clearing_system_reference: None,
                    instructing_agent: Some(agent(&self.instructing_agent_routing_number)),
                    instructed_agent: Some(agent(&self.instructed_agent_routing_number)),
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

/// Builder for a pacs.028 payment status request — the reconciliation
/// primitive: ask about a previously sent payment instead of resending it.
#[derive(Debug, Clone)]
pub struct Pacs028Builder {
    message_identification: String,
    creation_date_time: String,
    original_message_identification: String,
    original_message_name_identification: String,
    original_creation_date_time: String,
    instructing_agent_routing_number: String,
    instructed_agent_routing_number: String,
    original_instruction_identification: Option<String>,
    original_end_to_end_identification: Option<String>,
    original_uetr: Option<String>,
}

impl Pacs028Builder {
    /// Start a status request for the original message identified by
    /// `original_message_identification` / `original_creation_date_time`
    /// (a pacs.008 unless overridden).
    pub fn new(
        message_identification: impl Into<String>,
        creation_date_time: impl Into<String>,
        original_message_identification: impl Into<String>,
        original_creation_date_time: impl Into<String>,
        instructing_agent_routing_number: impl Into<String>,
        instructed_agent_routing_number: impl Into<String>,
    ) -> Self {
        Self {
            message_identification: message_identification.into(),
            creation_date_time: creation_date_time.into(),
            original_message_identification: original_message_identification.into(),
            original_message_name_identification: "pacs.008.001.08".to_string(),
            original_creation_date_time: original_creation_date_time.into(),
            instructing_agent_routing_number: instructing_agent_routing_number.into(),
            instructed_agent_routing_number: instructed_agent_routing_number.into(),
            original_instruction_identification: None,
            original_end_to_end_identification: None,
            original_uetr: None,
        }
    }

    /// Override the original message name (defaults to `pacs.008.001.08`).
    pub fn original_message_name_identification(mut self, v: impl Into<String>) -> Self {
        self.original_message_name_identification = v.into();
        self
    }

    pub fn original_instruction_identification(mut self, v: impl Into<String>) -> Self {
        self.original_instruction_identification = Some(v.into());
        self
    }

    pub fn original_end_to_end_identification(mut self, v: impl Into<String>) -> Self {
        self.original_end_to_end_identification = Some(v.into());
        self
    }

    pub fn original_uetr(mut self, v: impl Into<String>) -> Self {
        self.original_uetr = Some(v.into());
        self
    }

    /// Build the typed document.
    pub fn build(&self) -> crate::pacs028::Document {
        use crate::pacs002::OriginalGroupInformation;
        use crate::pacs028::{
            Document as P28Document, FIToFIPaymentStatusRequestV03, GroupHeader, PaymentTransaction,
        };

        P28Document {
            xmlns: Some(crate::pacs028::NAMESPACE.to_string()),
            fi_to_fi_payment_status_request: FIToFIPaymentStatusRequestV03 {
                group_header: GroupHeader {
                    message_identification: self.message_identification.clone(),
                    creation_date_time: self.creation_date_time.clone(),
                },
                transaction_information: vec![PaymentTransaction {
                    original_group_information: Some(OriginalGroupInformation {
                        original_message_identification: self
                            .original_message_identification
                            .clone(),
                        original_message_name_identification: self
                            .original_message_name_identification
                            .clone(),
                        original_creation_date_time: Some(self.original_creation_date_time.clone()),
                    }),
                    original_instruction_identification: self
                        .original_instruction_identification
                        .clone(),
                    original_end_to_end_identification: self
                        .original_end_to_end_identification
                        .clone(),
                    original_transaction_identification: None,
                    original_uetr: self.original_uetr.clone(),
                    instructing_agent: Some(agent(&self.instructing_agent_routing_number)),
                    instructed_agent: Some(agent(&self.instructed_agent_routing_number)),
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
