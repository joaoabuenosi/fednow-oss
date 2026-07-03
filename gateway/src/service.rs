//! The application service: idempotent submission and reconciliation.
//!
//! Orchestrates the domain core (events, state machine), fednow-core (message
//! construction and validation) and the southbound port. Owns no clocks —
//! `now_unix` and calendar dates come from the caller.
//!
//! Ordering note (v0): `Published` is recorded *before* the wire call, so an
//! ambiguous transport failure leaves the payment in `ACK_PENDING`, where the
//! reconciler resolves it with a pacs.028 — never a resend. A real outbox
//! publisher replaces this seam later without changing the states.

use fednow_core::builder::{fednow_message_id, Pacs008Builder, Pacs028Builder};
use fednow_core::validate::validate_pacs008;
use fednow_core::{pacs002, pacs008};
use thiserror::Error;

use crate::payment::{advice_from_pacs002, Payment, PaymentEvent, TransitionError};
use crate::reconciler::{reconciliation_action, ReconciliationAction};
use crate::southbound::{FedNowPort, PortError, SubmitOutcome};
use crate::store::{CreateOutcome, PaymentStore};

/// A northbound submission. The idempotency key is mandatory by design.
#[derive(Debug, Clone)]
pub struct SubmitRequest {
    pub idempotency_key: String,
    /// Calendar date `CCYYMMDD` used in the FedNow message id.
    pub date_yyyymmdd: String,
    /// Sender reference (1..18 alphanumerics) completing the message id.
    pub sender_reference: String,
    /// ISO 8601 creation date-time of the message.
    pub creation_date_time: String,
    pub end_to_end_identification: String,
    pub uetr: Option<String>,
    pub amount_cents: u64,
    pub debtor_name: String,
    pub debtor_account: String,
    pub creditor_name: String,
    pub creditor_account: String,
    pub creditor_agent_routing_number: String,
    /// `CONS` or `BIZZ`.
    pub category_purpose: String,
    /// Interbank settlement date `YYYY-MM-DD`.
    pub settlement_date: String,
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("payment fails the FedNow profile: {0:?}")]
    Validation(Vec<&'static str>),
    #[error(transparent)]
    Transition(#[from] TransitionError),
    #[error("message construction failed: {0}")]
    Build(String),
    #[error("unknown payment '{0}'")]
    UnknownPayment(String),
}

/// The gateway service: a store, a port, and the sending institution's
/// connection party identifier (its routing number on the FedNow connection).
pub struct PaymentService<S, P> {
    store: S,
    port: P,
    sender_routing_number: String,
}

/// The FedNow Service application identifier (`To` of every outbound query).
const FEDNOW_SERVICE_RTN: &str = "021150706";

impl<S: PaymentStore, P: FedNowPort> PaymentService<S, P> {
    pub fn new(store: S, port: P, sender_routing_number: impl Into<String>) -> Self {
        Self {
            store,
            port,
            sender_routing_number: sender_routing_number.into(),
        }
    }

    pub fn load(&self, idempotency_key: &str) -> Option<Payment> {
        self.store.load(idempotency_key)
    }

    /// Submit a payment, idempotently: resubmitting an existing key returns
    /// the payment as it stands, without touching the wire.
    pub fn submit(&self, req: &SubmitRequest, now_unix: i64) -> Result<Payment, ServiceError> {
        let message_identification = fednow_message_id(
            &req.date_yyyymmdd,
            &self.sender_routing_number,
            &req.sender_reference,
        );

        match self.store.create(PaymentEvent::Created {
            idempotency_key: req.idempotency_key.clone(),
            message_identification: message_identification.clone(),
            creation_date_time: req.creation_date_time.clone(),
            end_to_end_identification: req.end_to_end_identification.clone(),
            uetr: req.uetr.clone(),
            amount_cents: req.amount_cents,
            at_unix: now_unix,
        })? {
            CreateOutcome::Existing(payment) => return Ok(payment),
            CreateOutcome::Created(_) => {}
        }

        // Build and validate before anything reaches the wire.
        let mut builder = Pacs008Builder::new(
            message_identification,
            req.creation_date_time.clone(),
            req.end_to_end_identification.clone(),
            req.amount_cents,
            self.sender_routing_number.clone(),
            req.creditor_agent_routing_number.clone(),
        )
        .interbank_settlement_date(req.settlement_date.clone())
        .category_purpose(req.category_purpose.clone())
        .debtor_name(req.debtor_name.clone())
        .debtor_account(req.debtor_account.clone())
        .creditor_name(req.creditor_name.clone())
        .creditor_account(req.creditor_account.clone());
        if let Some(uetr) = &req.uetr {
            builder = builder.uetr(uetr.clone());
        }
        let xml = builder
            .to_xml()
            .map_err(|e| ServiceError::Build(e.to_string()))?;

        let doc = pacs008::parse(&xml).map_err(|e| ServiceError::Build(e.to_string()))?;
        let issues = validate_pacs008(&doc);
        if !issues.is_empty() {
            // Stays in Created; the caller sees exactly which rules failed.
            return Err(ServiceError::Validation(
                issues.into_iter().map(|i| i.code).collect(),
            ));
        }

        let key = &req.idempotency_key;
        self.store
            .append(key, PaymentEvent::Validated { at_unix: now_unix })?;
        self.store
            .append(key, PaymentEvent::Submitted { at_unix: now_unix })?;
        let mut payment = self
            .store
            .append(key, PaymentEvent::Published { at_unix: now_unix })?;

        match self.port.submit(&xml) {
            Ok(SubmitOutcome::Advice(advice_xml)) => {
                if let Some(event) = advice_event(&advice_xml, now_unix) {
                    payment = self.store.append(key, event)?;
                }
            }
            // No advice yet, or ambiguous transport failure: AckPending is
            // correct either way — the reconciler owns it from here.
            Ok(SubmitOutcome::Accepted) | Err(PortError::Transport(_)) => {}
            Err(PortError::Rejected { .. }) => {
                // Transport-level reject: message never entered processing.
                // Recorded as an advice-less pending for now; the reconciler's
                // query will confirm the service has no record of it.
            }
        }
        Ok(payment)
    }

    /// Drive reconciliation for one payment: declare the timeout when due,
    /// send a pacs.028 when due, apply whatever advice comes back.
    pub fn reconcile(
        &self,
        idempotency_key: &str,
        date_yyyymmdd: &str,
        now_unix: i64,
        timeout_secs: i64,
        backoff_secs: i64,
    ) -> Result<Payment, ServiceError> {
        let payment = self
            .store
            .load(idempotency_key)
            .ok_or_else(|| ServiceError::UnknownPayment(idempotency_key.to_string()))?;

        match reconciliation_action(&payment, now_unix, timeout_secs, backoff_secs) {
            ReconciliationAction::None => Ok(payment),
            ReconciliationAction::DeclareTimeout => Ok(self.store.append(
                idempotency_key,
                PaymentEvent::TimeoutDeclared { at_unix: now_unix },
            )?),
            ReconciliationAction::SendQuery => {
                let mut updated = self.store.append(
                    idempotency_key,
                    PaymentEvent::QuerySent { at_unix: now_unix },
                )?;

                let mut builder = Pacs028Builder::new(
                    fednow_message_id(
                        date_yyyymmdd,
                        &self.sender_routing_number,
                        &format!("Q{}", updated.queries_sent),
                    ),
                    payment.creation_date_time.clone(),
                    payment.message_identification.clone(),
                    payment.creation_date_time.clone(),
                    self.sender_routing_number.clone(),
                    FEDNOW_SERVICE_RTN,
                )
                .original_end_to_end_identification(payment.end_to_end_identification.clone());
                if let Some(uetr) = &payment.uetr {
                    builder = builder.original_uetr(uetr.clone());
                }
                let xml = builder
                    .to_xml()
                    .map_err(|e| ServiceError::Build(e.to_string()))?;

                // Queries are idempotent: any failure just means we ask again
                // after the backoff.
                if let Ok(SubmitOutcome::Advice(advice_xml)) = self.port.query(&xml) {
                    if let Some(event) = advice_event(&advice_xml, now_unix) {
                        updated = self.store.append(idempotency_key, event)?;
                    }
                }
                Ok(updated)
            }
        }
    }
}

fn advice_event(advice_xml: &str, now_unix: i64) -> Option<PaymentEvent> {
    let doc = pacs002::parse(advice_xml).ok()?;
    let (status, reason) = advice_from_pacs002(&doc)?;
    Some(PaymentEvent::AdviceReceived {
        status,
        reason,
        at_unix: now_unix,
    })
}
