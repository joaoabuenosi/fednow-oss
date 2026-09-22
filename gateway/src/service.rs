//! The application service: idempotent submission and reconciliation.
//!
//! Orchestrates the domain core (events, state machine), fednow-core (message
//! construction and validation) and the southbound port. Owns no clocks —
//! `now_unix` and calendar dates come from the caller. (The optional pre-send
//! risk check measures its own timeout on a monotonic clock; the elapsed time
//! it records is stored in the event, so replay stays deterministic.)
//!
//! Uses the outbox pattern: the `Submitted` event and the wire message become
//! durable in one transaction; [`PaymentService::publish_pending`] drains the
//! outbox and records `Published` only after confirmed handoff. Ambiguous
//! failures resolve via pacs.028 (the reconciler) — never a resend.

use fednow_core::builder::{fednow_message_id, Pacs008Builder, Pacs028Builder};
use fednow_core::envelope::{self, EnvelopedDocument};
use fednow_core::validate::validate_pacs008;
use fednow_core::{pacs002, pacs008};
use thiserror::Error;

use crate::hold::{HoldPolicy, EXPIRED_REASON, GATEWAY_ACTOR};
use crate::payment::{
    advice_from_pacs002, message_sha256, Payment, PaymentEvent, PaymentState, TransitionError,
};
use crate::reconciler::{reconciliation_action, ReconciliationAction};
use crate::risk::{sanitize_reason, RiskCheckInput, RiskGate, RiskOutcome};
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
    /// Release or cancel of a payment that is not `HELD` (never held, already
    /// released, already cancelled).
    #[error("payment is not held (state {})", .0.name())]
    NotHeld(PaymentState),
    /// The hold outlived [`HoldPolicy::max_age_secs`]; the payment was
    /// cancelled with `hold_expired` instead of being sent.
    #[error("the hold expired; the payment was cancelled, not sent")]
    HoldExpired,
    /// The payment is held but no parked message is on record (a hold
    /// recorded before parking existed). It can be cancelled, not released.
    #[error("no parked message for this held payment; cancel it and resubmit")]
    NoParkedMessage,
    /// A release/cancel reason must be a short code (see
    /// [`crate::risk::sanitize_reason`]); free text is refused, not rewritten.
    #[error("reason must be a short code: [a-z][a-z0-9_.-]{{0,63}}, no run of five digits")]
    InvalidReason,
}

/// The gateway service: a store, a port, and the sending institution's
/// connection party identifier (its routing number on the FedNow connection).
pub struct PaymentService<S, P> {
    store: S,
    port: P,
    sender_routing_number: String,
    /// Pre-send risk check. Disabled unless [`Self::with_risk_gate`] is
    /// called: no check runs and no event is recorded.
    risk: RiskGate,
    /// How long a held payment stays releasable.
    hold: HoldPolicy,
}

/// The FedNow Service application identifier (`To` of every outbound query).
const FEDNOW_SERVICE_RTN: &str = "993000007";

impl<S: PaymentStore, P: FedNowPort> PaymentService<S, P> {
    pub fn new(store: S, port: P, sender_routing_number: impl Into<String>) -> Self {
        Self {
            store,
            port,
            sender_routing_number: sender_routing_number.into(),
            risk: RiskGate::disabled(),
            hold: HoldPolicy::default(),
        }
    }

    /// Set how long a held payment stays releasable (see [`crate::hold`]).
    pub fn with_hold_policy(mut self, policy: HoldPolicy) -> Self {
        self.hold = policy;
        self
    }

    pub fn hold_policy(&self) -> HoldPolicy {
        self.hold
    }

    /// Put a pre-send risk check on the send path (see [`crate::risk`]).
    pub fn with_risk_gate(mut self, gate: RiskGate) -> Self {
        self.risk = gate;
        self
    }

    pub fn load(&self, idempotency_key: &str) -> Option<Payment> {
        self.store.load(idempotency_key)
    }

    /// The underlying store, for inspection (ops tooling, tests). Writing
    /// through it bypasses the service: the store still enforces the state
    /// machine and one outbox entry per payment, but nothing else.
    pub fn store(&self) -> &S {
        &self.store
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
        let validated = self
            .store
            .append(key, PaymentEvent::Validated { at_unix: now_unix })?;

        // Pre-send risk check: after validation (an invalid message costs no
        // risk budget), before the outbox (nothing leaves unchecked). With no
        // provider configured this is a no-op and records nothing.
        if let Some(verdict) = self.risk.check(&RiskCheckInput {
            idempotency_key: key.clone(),
            message_identification: validated.message_identification.clone(),
            end_to_end_identification: req.end_to_end_identification.clone(),
            amount_cents: req.amount_cents,
            creditor_agent_routing_number: req.creditor_agent_routing_number.clone(),
            creditor_account: req.creditor_account.clone(),
            category_purpose: req.category_purpose.clone(),
        }) {
            let checked = PaymentEvent::RiskChecked {
                outcome: verdict.outcome,
                reason: verdict.reason,
                source: verdict.source,
                provider: verdict.provider.to_string(),
                elapsed_ms: verdict.elapsed_ms,
                at_unix: now_unix,
            };
            match verdict.outcome {
                RiskOutcome::Allow => {
                    self.store.append(key, checked)?;
                }
                // HELD: the checked message is parked, not queued, in the
                // same transaction as the hold. Only a release moves it.
                RiskOutcome::Hold => {
                    let parked = PaymentEvent::HoldParked {
                        message_sha256: message_sha256(&xml),
                        at_unix: now_unix,
                    };
                    return Ok(self.store.park(key, vec![checked, parked], xml)?);
                }
                // REFUSED: terminal; the message is dropped.
                RiskOutcome::Refuse => return Ok(self.store.append(key, checked)?),
            }
        }

        // The outbox pattern's atomic step: the Submitted event and the wire
        // message become durable together — either both or neither.
        self.store
            .submit_to_outbox(key, PaymentEvent::Submitted { at_unix: now_unix }, xml)?;

        // Drain inline for the synchronous UX; the sweeper retries anything
        // a transport failure leaves behind.
        self.publish_pending(now_unix);
        self.store
            .load(key)
            .ok_or_else(|| ServiceError::UnknownPayment(key.clone()))
    }

    /// Publish outbox entries until empty or the transport fails.
    ///
    /// `Published` is recorded only after confirmed handoff — before that, a
    /// crash or transport failure leaves the payment in `SUBMITTED` with its
    /// outbox entry intact, and the next pass retries. A transport-level
    /// rejection marks the entry consumed (retrying an actively refused
    /// message is pointless); the payment stays visibly in `SUBMITTED` for
    /// operators. Returns the number of entries published.
    pub fn publish_pending(&self, now_unix: i64) -> usize {
        let mut published = 0;
        while let Some(entry) = self.store.next_unpublished() {
            match self.port.submit(&entry.message_xml) {
                Ok(outcome) => {
                    self.store.mark_published(entry.id);
                    let _ = self.store.append(
                        &entry.idempotency_key,
                        PaymentEvent::Published { at_unix: now_unix },
                    );
                    published += 1;
                    if let SubmitOutcome::Advice(advice_xml) = outcome {
                        if let Some(event) = advice_event(&advice_xml, now_unix) {
                            let _ = self.store.append(&entry.idempotency_key, event);
                        }
                    }
                }
                Err(PortError::Rejected { .. }) => {
                    self.store.mark_published(entry.id);
                }
                Err(PortError::Transport(_)) => break,
            }
        }
        published
    }

    /// Drain asynchronously delivered advices (MQ-style transports) and apply
    /// each to its payment. Returns how many advices changed a payment.
    ///
    /// Unknown or unusable envelopes are skipped, not fatal: an advice for a
    /// payment this gateway does not know (e.g. after a wipe) must not stall
    /// the queue behind it.
    pub fn pump_advices(&self, now_unix: i64) -> usize {
        let mut applied = 0;
        while let Ok(Some(envelope_xml)) = self.port.poll_advice() {
            if let Ok(Some(_)) = self.apply_advice_envelope(&envelope_xml, now_unix) {
                applied += 1;
            }
        }
        applied
    }

    /// Apply one received `FedNowOutgoing` envelope: correlate the pacs.002
    /// advice to a payment by its original message id and advance the state
    /// machine. Returns the updated payment, or `None` when the envelope is
    /// not an advice / references no known payment.
    pub fn apply_advice_envelope(
        &self,
        envelope_xml: &str,
        now_unix: i64,
    ) -> Result<Option<Payment>, ServiceError> {
        let env = envelope::parse(envelope_xml).map_err(|e| ServiceError::Build(e.to_string()))?;
        let EnvelopedDocument::PaymentStatus(doc) = &env.document else {
            return Ok(None);
        };
        let Some(original_message_id) = doc
            .fi_to_fi_payment_status_report
            .transaction_information_and_status
            .first()
            .and_then(|tx| tx.original_group_information.as_ref())
            .map(|o| o.original_message_identification.as_str())
        else {
            return Ok(None);
        };
        let Some(payment) = self.find_by_message_identification(original_message_id) else {
            return Ok(None);
        };

        let Some((status, reason)) = advice_from_pacs002(doc) else {
            return Ok(None);
        };
        let updated = self.store.append(
            &payment.idempotency_key,
            PaymentEvent::AdviceReceived {
                status,
                reason,
                at_unix: now_unix,
            },
        )?;
        Ok(Some(updated))
    }

    /// Operational snapshot: what a 24x7 operator (or a probe) needs at a
    /// glance. Linear over the store — fine at development scale; a
    /// production store can answer from indexes behind the same trait.
    pub fn summary(&self, now_unix: i64) -> OpsSummary {
        let mut by_state: std::collections::BTreeMap<&'static str, usize> =
            std::collections::BTreeMap::new();
        let mut total = 0usize;
        let mut oldest_unresolved_age_secs: Option<i64> = None;
        for key in self.store.keys() {
            let Some(p) = self.store.load(&key) else {
                continue;
            };
            total += 1;
            *by_state.entry(p.state.name()).or_insert(0) += 1;
            if p.state == crate::payment::PaymentState::TimeoutUnresolved {
                if let Some(published) = p.published_at_unix {
                    let age = (now_unix - published).max(0);
                    oldest_unresolved_age_secs =
                        Some(oldest_unresolved_age_secs.map_or(age, |o| o.max(age)));
                }
            }
        }
        OpsSummary {
            payments_total: total,
            by_state,
            outbox_pending: self.store.unpublished_count(),
            oldest_unresolved_age_secs,
        }
    }

    fn find_by_message_identification(&self, message_identification: &str) -> Option<Payment> {
        // Linear over the key set: fine for the development stores; a
        // production store can grow an indexed lookup behind the same trait.
        self.store
            .keys()
            .into_iter()
            .filter_map(|k| self.store.load(&k))
            .find(|p| p.message_identification == message_identification)
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

impl<S: PaymentStore, P: FedNowPort> PaymentService<S, P> {
    /// Release a held payment: send exactly the message that was parked when
    /// it was held, then drain the outbox as `submit` does.
    ///
    /// `actor` names who released it (`operator:<label>`, from the caller's
    /// key, never from the request body); `reason` must be a short code. A
    /// hold past [`HoldPolicy::max_age_secs`] is cancelled with
    /// `hold_expired` instead, and [`ServiceError::HoldExpired`] is returned.
    pub fn release(
        &self,
        idempotency_key: &str,
        actor: &str,
        reason: &str,
        now_unix: i64,
    ) -> Result<Payment, ServiceError> {
        let payment = self.held(idempotency_key, reason)?;
        if payment
            .held_at_unix
            .is_some_and(|at| self.hold.is_expired(at, now_unix))
        {
            self.expire(idempotency_key, now_unix)?;
            return Err(ServiceError::HoldExpired);
        }
        let Some(expected) = payment.held_message_sha256.clone() else {
            return Err(ServiceError::NoParkedMessage);
        };
        let event = PaymentEvent::HoldReleased {
            actor: actor.to_string(),
            reason: reason.to_string(),
            message_sha256: expected,
            at_unix: now_unix,
        };
        if let Err(e) = self.store.release_parked(idempotency_key, event) {
            return Err(self.explain(idempotency_key, e));
        }
        self.publish_pending(now_unix);
        self.store
            .load(idempotency_key)
            .ok_or_else(|| ServiceError::UnknownPayment(idempotency_key.to_string()))
    }

    /// Cancel a held payment: it ends `CANCELLED`, never sent, and its parked
    /// message is deleted. Allowed at any age.
    pub fn cancel(
        &self,
        idempotency_key: &str,
        actor: &str,
        reason: &str,
        now_unix: i64,
    ) -> Result<Payment, ServiceError> {
        self.held(idempotency_key, reason)?;
        let event = PaymentEvent::HoldCancelled {
            actor: actor.to_string(),
            reason: reason.to_string(),
            at_unix: now_unix,
        };
        self.store
            .discard_parked(idempotency_key, event)
            .map_err(|e| self.explain(idempotency_key, e))
    }

    /// Cancel every hold that outlived the policy (the sweeper calls this).
    /// Returns how many were cancelled. With no risk provider configured
    /// there are no holds, and this changes nothing.
    pub fn expire_holds(&self, now_unix: i64) -> usize {
        let mut expired = 0;
        for key in self.store.keys() {
            let Some(p) = self.store.load(&key) else {
                continue;
            };
            let due = p.state == PaymentState::Held
                && p.held_at_unix
                    .is_some_and(|at| self.hold.is_expired(at, now_unix));
            if due && self.expire(&key, now_unix).is_ok() {
                expired += 1;
            }
        }
        expired
    }

    fn expire(&self, idempotency_key: &str, now_unix: i64) -> Result<Payment, ServiceError> {
        self.store
            .discard_parked(
                idempotency_key,
                PaymentEvent::HoldCancelled {
                    actor: GATEWAY_ACTOR.to_string(),
                    reason: EXPIRED_REASON.to_string(),
                    at_unix: now_unix,
                },
            )
            .map_err(|e| self.explain(idempotency_key, e))
    }

    /// The payment, if it exists and is `HELD`, and the reason is a code.
    fn held(&self, idempotency_key: &str, reason: &str) -> Result<Payment, ServiceError> {
        if sanitize_reason(reason) != reason {
            return Err(ServiceError::InvalidReason);
        }
        let payment = self
            .store
            .load(idempotency_key)
            .ok_or_else(|| ServiceError::UnknownPayment(idempotency_key.to_string()))?;
        if payment.state != PaymentState::Held {
            return Err(ServiceError::NotHeld(payment.state));
        }
        Ok(payment)
    }

    /// Turn a store refusal into what the caller can act on: if the payment
    /// left `HELD` meanwhile (a concurrent release or cancel won), say so.
    fn explain(&self, idempotency_key: &str, e: TransitionError) -> ServiceError {
        match self.store.load(idempotency_key) {
            Some(p) if p.state != PaymentState::Held => ServiceError::NotHeld(p.state),
            _ => ServiceError::Transition(e),
        }
    }

    /// Sweep every known payment through one reconciliation pass. Errors on
    /// individual payments are collected, not fatal — one stuck payment must
    /// not stop the sweep.
    pub fn reconcile_all(
        &self,
        date_yyyymmdd: &str,
        now_unix: i64,
        timeout_secs: i64,
        backoff_secs: i64,
    ) -> Vec<(String, ServiceError)> {
        let mut errors = Vec::new();
        for key in self.store.keys() {
            if let Err(e) =
                self.reconcile(&key, date_yyyymmdd, now_unix, timeout_secs, backoff_secs)
            {
                errors.push((key, e));
            }
        }
        errors
    }
}

/// Snapshot returned by [`PaymentService::summary`].
#[derive(Debug, Clone)]
pub struct OpsSummary {
    pub payments_total: usize,
    /// Counts keyed by state name (`SETTLED`, `TIMEOUT_UNRESOLVED`, …).
    pub by_state: std::collections::BTreeMap<&'static str, usize>,
    /// Outbox entries awaiting publication — growth means the transport is
    /// down or refusing.
    pub outbox_pending: usize,
    /// Age of the longest-waiting `TIMEOUT_UNRESOLVED` payment, if any —
    /// the number an operator pages on.
    pub oldest_unresolved_age_secs: Option<i64>,
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
