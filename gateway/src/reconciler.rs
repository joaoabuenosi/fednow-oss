//! Reconciliation policy: what to do about a payment that has not resolved.
//!
//! Pure decision function — the caller owns clocks, transports and retries.
//! The rule it encodes is the handbook's: resolve `ACK_PENDING` past the
//! presumed timeout with payment status requests (pacs.028), never with blind
//! resends.

use crate::payment::{Payment, PaymentState};

/// What the reconciler should do next for a payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconciliationAction {
    /// Nothing to do (not pending, or still inside the timeout window).
    None,
    /// The presumed timeout elapsed: record `TimeoutDeclared`.
    DeclareTimeout,
    /// Unresolved and due (first query, or backoff elapsed): send a pacs.028
    /// and record `QuerySent`.
    SendQuery,
}

/// Decide the next reconciliation action.
///
/// `timeout_secs` is the presumed timeout after publication; `backoff_secs`
/// the minimum interval between queries. Note the FedNow query window: the
/// service only answers for the current or prior calendar day — escalate to
/// manual operations before that window closes.
pub fn reconciliation_action(
    payment: &Payment,
    now_unix: i64,
    timeout_secs: i64,
    backoff_secs: i64,
) -> ReconciliationAction {
    match payment.state {
        PaymentState::AckPending => match payment.published_at_unix {
            Some(published) if now_unix - published >= timeout_secs => {
                ReconciliationAction::DeclareTimeout
            }
            _ => ReconciliationAction::None,
        },
        PaymentState::TimeoutUnresolved => match payment.last_query_at_unix {
            None => ReconciliationAction::SendQuery,
            Some(last) if now_unix - last >= backoff_secs => ReconciliationAction::SendQuery,
            Some(_) => ReconciliationAction::None,
        },
        _ => ReconciliationAction::None,
    }
}
