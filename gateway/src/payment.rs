//! The payment aggregate: immutable events, pure transitions, replayable state.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The lifecycle states of an outbound FedNow payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentState {
    /// Accepted from the northbound caller; nothing sent yet.
    Created,
    /// Passed fednow-core validation (profile-clean pacs.008).
    Validated,
    /// Written to the outbox; not yet confirmed on the wire.
    Submitted,
    /// On the wire; awaiting the service advice.
    AckPending,
    /// The service advised settlement (`ACSC`/`ACCC`), or `ACWP` — funds
    /// settled (posting may still be pending on the receiving side).
    Settled,
    /// The service advised rejection.
    Rejected,
    /// No advice within the timeout: unresolved, awaiting reconciliation.
    /// Resolved only by an advice obtained via pacs.028 (or manual ops).
    TimeoutUnresolved,
}

/// Transaction statuses an advice can carry, as the gateway interprets them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdviceStatus {
    /// Interim: accepted by the receiving participant.
    Actc,
    /// Settlement completed.
    Acsc,
    /// Creditor account credited (confirmation).
    Accc,
    /// Accepted without posting — settled, posting unresolved downstream.
    Acwp,
    /// Interim: pending at the receiving participant.
    Pdng,
    /// Funds blocked at the receiving participant (post-settlement).
    Blck,
    /// Rejected.
    Rjct,
}

impl AdviceStatus {
    /// Map a pacs.002 `TxSts` code.
    pub fn from_tx_sts(code: &str) -> Option<Self> {
        Some(match code {
            "ACTC" => Self::Actc,
            "ACSC" => Self::Acsc,
            "ACCC" => Self::Accc,
            "ACWP" => Self::Acwp,
            "PDNG" => Self::Pdng,
            "BLCK" => Self::Blck,
            "RJCT" => Self::Rjct,
            _ => return None,
        })
    }
}

/// Immutable facts about one payment, in order of occurrence.
///
/// Timestamps are caller-provided unix seconds — the domain never reads a
/// clock, which keeps replay deterministic. Serialized as JSON by durable
/// stores; the variant names are part of the storage format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymentEvent {
    Created {
        idempotency_key: String,
        message_identification: String,
        /// ISO 8601 creation date-time of the pacs.008 (needed later to
        /// identify the original message in a pacs.028).
        creation_date_time: String,
        end_to_end_identification: String,
        uetr: Option<String>,
        amount_cents: u64,
        at_unix: i64,
    },
    Validated {
        at_unix: i64,
    },
    /// Written durably to the outbox.
    Submitted {
        at_unix: i64,
    },
    /// The outbox publisher confirmed handoff to the transport.
    Published {
        at_unix: i64,
    },
    /// A pacs.002 advice arrived (pushed, or via pacs.028).
    AdviceReceived {
        status: AdviceStatus,
        reason: Option<String>,
        at_unix: i64,
    },
    /// The reconciler declared the presumed timeout elapsed.
    TimeoutDeclared {
        at_unix: i64,
    },
    /// The reconciler sent a payment status request (pacs.028).
    QuerySent {
        at_unix: i64,
    },
}

/// A transition the state machine refuses.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("illegal transition: {event:?} in state {state:?}")]
pub struct TransitionError {
    pub state: PaymentState,
    pub event: String,
}

/// The replayed aggregate.
#[derive(Debug, Clone)]
pub struct Payment {
    pub state: PaymentState,
    pub idempotency_key: String,
    pub message_identification: String,
    pub creation_date_time: String,
    pub end_to_end_identification: String,
    pub uetr: Option<String>,
    pub published_at_unix: Option<i64>,
    pub last_query_at_unix: Option<i64>,
    pub queries_sent: u32,
    /// Last advice status seen (interim ones included).
    pub last_advice: Option<AdviceStatus>,
    /// Reason carried by a rejection, if any.
    pub rejection_reason: Option<String>,
    pub events: Vec<PaymentEvent>,
}

impl Payment {
    /// Start an aggregate from its creation event.
    pub fn new(created: PaymentEvent) -> Result<Self, TransitionError> {
        match &created {
            PaymentEvent::Created {
                idempotency_key,
                message_identification,
                creation_date_time,
                end_to_end_identification,
                uetr,
                ..
            } => Ok(Self {
                state: PaymentState::Created,
                idempotency_key: idempotency_key.clone(),
                message_identification: message_identification.clone(),
                creation_date_time: creation_date_time.clone(),
                end_to_end_identification: end_to_end_identification.clone(),
                uetr: uetr.clone(),
                published_at_unix: None,
                last_query_at_unix: None,
                queries_sent: 0,
                last_advice: None,
                rejection_reason: None,
                events: vec![created],
            }),
            other => Err(TransitionError {
                state: PaymentState::Created,
                event: format!("{other:?} as first event"),
            }),
        }
    }

    /// Rebuild the aggregate from its full event history.
    pub fn replay(events: Vec<PaymentEvent>) -> Result<Self, TransitionError> {
        let mut iter = events.into_iter();
        let first = iter.next().ok_or(TransitionError {
            state: PaymentState::Created,
            event: "empty event history".to_string(),
        })?;
        let mut payment = Self::new(first)?;
        for event in iter {
            payment.apply(event)?;
        }
        Ok(payment)
    }

    /// Apply one event, enforcing the legal transitions.
    pub fn apply(&mut self, event: PaymentEvent) -> Result<(), TransitionError> {
        use PaymentEvent as E;
        use PaymentState as S;

        let next = match (&self.state, &event) {
            (S::Created, E::Validated { .. }) => S::Validated,
            (S::Validated, E::Submitted { .. }) => S::Submitted,
            (S::Submitted, E::Published { at_unix }) => {
                self.published_at_unix = Some(*at_unix);
                S::AckPending
            }
            // Advices resolve pending and unresolved payments alike (a late
            // advice or a pacs.028 answer is still the truth).
            (S::AckPending | S::TimeoutUnresolved, E::AdviceReceived { status, reason, .. }) => {
                self.last_advice = Some(*status);
                match status {
                    AdviceStatus::Acsc | AdviceStatus::Accc | AdviceStatus::Acwp => S::Settled,
                    AdviceStatus::Rjct => {
                        self.rejection_reason = reason.clone();
                        S::Rejected
                    }
                    // Interim statuses keep us waiting.
                    AdviceStatus::Actc | AdviceStatus::Pdng => self.state,
                    // Blocked after settlement talk is downstream information;
                    // funds-wise we are settled.
                    AdviceStatus::Blck => S::Settled,
                }
            }
            // Post-settlement confirmations/updates are recorded, state holds.
            (S::Settled, E::AdviceReceived { status, .. }) => {
                self.last_advice = Some(*status);
                S::Settled
            }
            (S::AckPending, E::TimeoutDeclared { .. }) => S::TimeoutUnresolved,
            (S::TimeoutUnresolved, E::QuerySent { at_unix }) => {
                self.queries_sent += 1;
                self.last_query_at_unix = Some(*at_unix);
                S::TimeoutUnresolved
            }
            (state, event) => {
                return Err(TransitionError {
                    state: *state,
                    event: format!("{event:?}"),
                })
            }
        };
        self.state = next;
        self.events.push(event);
        Ok(())
    }
}

/// Extract the gateway's view of a pacs.002: the advice status and, for
/// rejections, the reason (external code or proprietary).
pub fn advice_from_pacs002(
    doc: &fednow_core::pacs002::Document,
) -> Option<(AdviceStatus, Option<String>)> {
    let tx = doc
        .fi_to_fi_payment_status_report
        .transaction_information_and_status
        .first()?;
    let status = AdviceStatus::from_tx_sts(tx.transaction_status.as_deref()?)?;
    let reason = tx.status_reason_information.first().and_then(|s| {
        s.reason
            .as_ref()
            .and_then(|r| r.code.clone().or_else(|| r.proprietary.clone()))
    });
    Some((status, reason))
}
