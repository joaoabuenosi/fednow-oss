//! The payment aggregate: immutable events, pure transitions, replayable state.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::risk::{RiskOutcome, RiskSource};

/// The lifecycle states of an outbound FedNow payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentState {
    /// Accepted from the northbound caller; nothing sent yet.
    Created,
    /// Passed fednow-core validation (profile-clean pacs.008).
    Validated,
    /// The pre-send risk check said hold (or failed under the default
    /// fail-closed policy). Not sent; awaiting a person, who releases it
    /// (→ `Submitted`) or cancels it (→ `Cancelled`). Only reachable when a
    /// risk provider is configured.
    Held,
    /// The pre-send risk check refused it. Not sent, terminal. Distinct from
    /// `Rejected`, which is a verdict from the far side after sending.
    Refused,
    /// Written to the outbox; not yet confirmed on the wire.
    Submitted,
    /// On the wire; awaiting the service advice.
    AckPending,
    /// The service advised settlement (`ACSC`/`ACCC`), or `ACWP` — funds
    /// settled (posting may still be pending on the receiving side).
    Settled,
    /// The service advised rejection.
    Rejected,
    /// A held payment that was never sent: an operator cancelled it, or it
    /// outlived the hold policy's maximum age. Terminal. Distinct from
    /// `Refused` (the risk check's own verdict at submission) and from
    /// `Rejected` (the far side's verdict after sending).
    Cancelled,
    /// No advice within the timeout: unresolved, awaiting reconciliation.
    /// Resolved only by an advice obtained via pacs.028 (or manual ops).
    TimeoutUnresolved,
}

impl PaymentState {
    /// Wire/display name, as exposed by the REST API and ops views.
    pub fn name(self) -> &'static str {
        match self {
            PaymentState::Created => "CREATED",
            PaymentState::Validated => "VALIDATED",
            PaymentState::Held => "HELD",
            PaymentState::Refused => "REFUSED",
            PaymentState::Submitted => "SUBMITTED",
            PaymentState::AckPending => "ACK_PENDING",
            PaymentState::Settled => "SETTLED",
            PaymentState::Rejected => "REJECTED",
            PaymentState::Cancelled => "CANCELLED",
            PaymentState::TimeoutUnresolved => "TIMEOUT_UNRESOLVED",
        }
    }
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
    /// The pre-send risk check ran (see [`crate::risk`]). Recorded only when
    /// a provider is configured. Carries no payment data: an outcome, a
    /// sanitised reason code, who decided, and how long it took.
    RiskChecked {
        outcome: RiskOutcome,
        reason: Option<String>,
        source: RiskSource,
        provider: String,
        elapsed_ms: u64,
        at_unix: i64,
    },
    /// The built pacs.008 of a held payment was parked outside the outbox,
    /// in the same transaction as the `RiskChecked` hold. Records only the
    /// message's SHA-256 (lowercase hex), so the audit trail can prove that a
    /// release sent exactly the message that was checked.
    HoldParked {
        message_sha256: String,
        at_unix: i64,
    },
    /// A held payment was released: its parked message moved to the outbox,
    /// in the same transaction. Takes the payment to `Submitted`, as the
    /// `Submitted` event does for a payment that was never held.
    HoldReleased {
        /// Who released it: `operator:<label>` (see [`crate::auth`]).
        actor: String,
        /// A short code (the same rule as risk reasons), never free text.
        reason: String,
        /// SHA-256 of the message that went to the outbox. Equal to the
        /// `HoldParked` digest, or the store refuses the release.
        message_sha256: String,
        at_unix: i64,
    },
    /// A held payment was cancelled and its parked message discarded.
    HoldCancelled {
        /// `operator:<label>`, or `gateway` when the hold expired.
        actor: String,
        /// A short code; `hold_expired` when the gateway cancelled it.
        reason: String,
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
    /// The pre-send risk check's outcome, when one ran.
    pub risk_outcome: Option<RiskOutcome>,
    /// Its sanitised reason code (hold / refuse).
    pub risk_reason: Option<String>,
    /// Who produced the outcome: the provider or the failure policy.
    pub risk_source: Option<RiskSource>,
    /// When the payment was held, if it was.
    pub held_at_unix: Option<i64>,
    /// SHA-256 of the parked message, while one is on record.
    pub held_message_sha256: Option<String>,
    /// How a hold ended: released or cancelled, by whom, why, when.
    pub hold_resolution: Option<HoldResolution>,
    pub events: Vec<PaymentEvent>,
}

/// What ended a hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldAction {
    Released,
    Cancelled,
}

impl HoldAction {
    pub fn name(self) -> &'static str {
        match self {
            HoldAction::Released => "released",
            HoldAction::Cancelled => "cancelled",
        }
    }
}

/// The end of a hold, as the event history records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldResolution {
    pub action: HoldAction,
    pub actor: String,
    pub reason: String,
    pub at_unix: i64,
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
                risk_outcome: None,
                risk_reason: None,
                risk_source: None,
                held_at_unix: None,
                held_message_sha256: None,
                hold_resolution: None,
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
            // One check per payment, between validation and the outbox.
            (
                S::Validated,
                E::RiskChecked {
                    outcome,
                    reason,
                    source,
                    at_unix,
                    ..
                },
            ) if self.risk_outcome.is_none() => {
                self.risk_outcome = Some(*outcome);
                self.risk_reason = reason.clone();
                self.risk_source = Some(*source);
                match outcome {
                    RiskOutcome::Allow => S::Validated,
                    RiskOutcome::Hold => {
                        self.held_at_unix = Some(*at_unix);
                        S::Held
                    }
                    RiskOutcome::Refuse => S::Refused,
                }
            }
            // One parked message per hold.
            (S::Held, E::HoldParked { message_sha256, .. })
                if self.held_message_sha256.is_none() =>
            {
                self.held_message_sha256 = Some(message_sha256.clone());
                S::Held
            }
            // A release sends the parked message and nothing else: the digest
            // it records must be the one recorded when the message was parked.
            (
                S::Held,
                E::HoldReleased {
                    actor,
                    reason,
                    message_sha256,
                    at_unix,
                },
            ) if self.held_message_sha256.as_deref() == Some(message_sha256.as_str()) => {
                self.hold_resolution = Some(HoldResolution {
                    action: HoldAction::Released,
                    actor: actor.clone(),
                    reason: reason.clone(),
                    at_unix: *at_unix,
                });
                S::Submitted
            }
            (
                S::Held,
                E::HoldCancelled {
                    actor,
                    reason,
                    at_unix,
                },
            ) => {
                self.held_message_sha256 = None;
                self.hold_resolution = Some(HoldResolution {
                    action: HoldAction::Cancelled,
                    actor: actor.clone(),
                    reason: reason.clone(),
                    at_unix: *at_unix,
                });
                S::Cancelled
            }
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

/// SHA-256 of a message, as lowercase hex: the digest `HoldParked` and
/// `HoldReleased` record.
pub fn message_sha256(message_xml: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, message_xml.as_bytes());
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
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
