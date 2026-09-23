//! Resolving a held payment: release it, cancel it, or let it expire.
//!
//! A payment the pre-send risk check holds ([`crate::risk`]) has already been
//! built and validated. Its pacs.008 is **parked** outside the outbox, in the
//! same transaction as the hold, and only its SHA-256 goes to the event
//! history (`HoldParked`). From there it has three ways out:
//!
//! | Way out | Who | Event | State |
//! |---|---|---|---|
//! | release | an operator key | `HoldReleased` | `SUBMITTED`, then on as any payment |
//! | cancel | an operator key | `HoldCancelled` | `CANCELLED` |
//! | expiry | the gateway | `HoldCancelled` (`actor: gateway`, `reason: hold_expired`) | `CANCELLED` |
//!
//! **A release sends the parked bytes, never a rebuilt message.** The store
//! moves the parked message to the outbox in one transaction, and refuses when
//! its digest differs from the one `HoldParked` recorded. It also refuses a
//! second outbox entry for the same payment, so a double release, or two
//! racing ones, sends once.
//!
//! **Staleness.** A parked message carries the creation date-time and the
//! interbank settlement date it was built with. The gateway will not rebuild
//! it (that would send something nobody checked), so the age of a hold is
//! also how stale those dates are. The rule is the simplest one that bounds
//! it: past [`HoldPolicy::max_age_secs`], a hold can no longer be released.
//! The sweeper cancels it with `hold_expired`, and a release that arrives
//! first does the same instead of sending. To send the payment anyway,
//! submit it again under a new idempotency key: it is rebuilt with today's
//! dates and checked again.

use thiserror::Error;

/// Environment variable for [`HoldPolicy::max_age_secs`].
pub const HOLD_MAX_AGE_ENV: &str = "FEDNOW_GW_HOLD_MAX_AGE_SECS";
/// Default maximum age of a hold: 4 hours. A local choice, not a published
/// figure: long enough for a review within a working shift, short enough that
/// a released message still describes the day it was built on.
pub const DEFAULT_HOLD_MAX_AGE_SECS: i64 = 4 * 60 * 60;
/// The actor recorded when the gateway itself resolves a hold.
pub const GATEWAY_ACTOR: &str = "gateway";
/// The reason recorded when a hold outlives the policy.
pub const EXPIRED_REASON: &str = "hold_expired";

/// How long a held payment may wait for a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldPolicy {
    /// A hold older than this (in seconds, from the `RiskChecked` hold) can
    /// no longer be released. Positive.
    pub max_age_secs: i64,
}

impl Default for HoldPolicy {
    fn default() -> Self {
        Self {
            max_age_secs: DEFAULT_HOLD_MAX_AGE_SECS,
        }
    }
}

impl HoldPolicy {
    /// Whether a hold taken at `held_at_unix` has outlived the policy at
    /// `now_unix`. A hold is releasable up to and including its maximum age.
    pub fn is_expired(&self, held_at_unix: i64, now_unix: i64) -> bool {
        now_unix.saturating_sub(held_at_unix) > self.max_age_secs
    }

    /// The last second at which a hold taken at `held_at_unix` is releasable.
    pub fn expires_at(&self, held_at_unix: i64) -> i64 {
        held_at_unix.saturating_add(self.max_age_secs)
    }
}

/// Why the hold configuration was refused at startup.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HoldConfigError {
    #[error("{HOLD_MAX_AGE_ENV} must be a positive integer (seconds), found '{0}'")]
    NotPositive(String),
}

/// Build the policy from environment-style lookups (a closure, so tests need
/// no process environment).
pub fn hold_policy_from_lookup(
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<HoldPolicy, HoldConfigError> {
    match lookup(HOLD_MAX_AGE_ENV) {
        None => Ok(HoldPolicy::default()),
        Some(v) => match v.trim().parse::<i64>() {
            Ok(n) if n > 0 => Ok(HoldPolicy { max_age_secs: n }),
            _ => Err(HoldConfigError::NotPositive(v)),
        },
    }
}
