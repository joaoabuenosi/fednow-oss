//! Pre-send risk check: a vendor-neutral extension point on the send path.
//!
//! FedNow settlement is final (handbook ch. 5), so the last moment a sender
//! can stop a payment is before it enters the outbox. [`RiskProvider`] is the
//! seam for whatever answers "should this go?" there — an in-house rule
//! engine, a vendor score, or, once its specification is obtainable, a client
//! for the Federal Reserve's Network Intelligence API (issue #95). **No such
//! client exists in this crate**, and nothing here models that API: its wire
//! contract is not public.
//!
//! The service calls [`RiskGate::check`] after profile validation and before
//! `submit_to_outbox`. The gate owns the parts every provider needs and none
//! should re-implement:
//!
//! - a **timeout** (the latency budget of one check) enforced from outside the
//!   provider, so a hung provider cannot stall a submission;
//! - a **concurrency budget** bounding provider calls in flight, so a hung
//!   provider cannot pile up threads either;
//! - the **failure policy** ([`OnUnavailable`]) deciding what a timeout, an
//!   error or an exhausted budget means;
//! - **reason sanitisation**, so what reaches the audit trail is a short code
//!   and never an account number or a provider payload.
//!
//! A gate with no provider ([`RiskGate::disabled`], the default) runs no check
//! and records nothing: the payment's event history is exactly what it was
//! before this module existed.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// What a provider sees about a payment about to be sent.
///
/// The gateway hands this to the provider, never to the audit trail. A
/// provider should forward only what its backend needs (data minimisation):
/// the bundled [`SimRiskProvider`] sends just the amount and the creditor
/// agent's routing number.
#[derive(Debug, Clone)]
pub struct RiskCheckInput {
    pub idempotency_key: String,
    pub message_identification: String,
    pub end_to_end_identification: String,
    pub amount_cents: u64,
    pub creditor_agent_routing_number: String,
    pub creditor_account: String,
    pub category_purpose: String,
}

/// A provider's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskDecision {
    /// Send the payment.
    Allow,
    /// Do not send; a person must review it. `reason` is a short code.
    Hold { reason: String },
    /// Do not send, ever. `reason` is a short code.
    Refuse { reason: String },
}

/// Why a provider could not answer. The detail never reaches the audit trail
/// (it may carry URLs or payload fragments); only the fact that the provider
/// failed does, as [`RiskSource::Error`].
#[derive(Debug, Error)]
pub enum RiskError {
    /// The provider could not be reached or refused to answer.
    #[error("risk provider unavailable: {0}")]
    Unavailable(String),
    /// The provider answered with something that is not a decision.
    #[error("risk provider answered with an invalid response: {0}")]
    InvalidResponse(String),
}

/// A pre-send risk provider.
///
/// Implementations are blocking (the service layer is); the gate runs each
/// call on its own thread and stops waiting at the configured timeout. A
/// provider that retries internally must do so within `timeout`, which is
/// passed in so it can size its own I/O deadlines and release the thread
/// promptly after the gate has given up.
pub trait RiskProvider: Send + Sync {
    /// A short, stable name recorded in the audit trail (e.g. `sim`).
    fn name(&self) -> &'static str;
    fn check(&self, input: &RiskCheckInput, timeout: Duration) -> Result<RiskDecision, RiskError>;
}

/// What to do when no decision arrives: timeout, provider error, or the
/// concurrency budget is exhausted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnUnavailable {
    /// Fail closed, recoverably: the payment is not sent and waits for a
    /// person. **The default** — see the gateway README for the argument.
    Hold,
    /// Fail closed, terminally: the payment is not sent.
    Refuse,
    /// Fail open: the payment is sent unchecked. The audit trail still
    /// records that the check did not happen and why.
    Allow,
}

impl OnUnavailable {
    /// Parse the `FEDNOW_GW_RISK_ON_UNAVAILABLE` value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "hold" => Some(Self::Hold),
            "refuse" => Some(Self::Refuse),
            "allow" => Some(Self::Allow),
            _ => None,
        }
    }

    /// The configuration spelling (`hold`, `refuse`, `allow`).
    pub fn name(self) -> &'static str {
        match self {
            Self::Hold => "hold",
            Self::Refuse => "refuse",
            Self::Allow => "allow",
        }
    }
}

/// Policy knobs of a [`RiskGate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiskPolicy {
    /// Latency budget of one check, end to end. Past it, `on_unavailable`
    /// decides.
    pub timeout: Duration,
    /// Provider calls allowed in flight at once. A call the gate stopped
    /// waiting for still counts until the provider actually returns, so a hung
    /// provider exhausts the budget instead of accumulating threads.
    pub max_in_flight: usize,
    pub on_unavailable: OnUnavailable,
}

impl Default for RiskPolicy {
    fn default() -> Self {
        Self {
            timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS),
            max_in_flight: DEFAULT_MAX_IN_FLIGHT,
            on_unavailable: OnUnavailable::Hold,
        }
    }
}

/// Default latency budget. A local choice, not a figure from any Federal
/// Reserve document: the check sits in the synchronous submit path, so it must
/// stay small next to the gateway's presumed timeout (20 s by default).
pub const DEFAULT_TIMEOUT_MS: u64 = 1_000;
/// Default concurrency budget.
pub const DEFAULT_MAX_IN_FLIGHT: usize = 32;

/// The recorded outcome of a check, as it appears in the audit trail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskOutcome {
    Allow,
    Hold,
    Refuse,
}

impl RiskOutcome {
    pub fn name(self) -> &'static str {
        match self {
            RiskOutcome::Allow => "allow",
            RiskOutcome::Hold => "hold",
            RiskOutcome::Refuse => "refuse",
        }
    }
}

/// Who produced the outcome: the provider, or the failure policy and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskSource {
    /// The provider answered within the timeout.
    Provider,
    /// No answer within the timeout; the policy decided.
    Timeout,
    /// The provider failed; the policy decided.
    Error,
    /// Too many calls in flight; the provider was not called and the policy
    /// decided.
    BudgetExhausted,
}

impl RiskSource {
    pub fn name(self) -> &'static str {
        match self {
            RiskSource::Provider => "provider",
            RiskSource::Timeout => "timeout",
            RiskSource::Error => "error",
            RiskSource::BudgetExhausted => "budget_exhausted",
        }
    }
}

/// The gate's verdict: everything the audit event records, nothing more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskVerdict {
    pub outcome: RiskOutcome,
    /// Sanitised reason code (see [`sanitize_reason`]); `None` on allow.
    pub reason: Option<String>,
    pub source: RiskSource,
    pub provider: &'static str,
    pub elapsed_ms: u64,
}

/// The pre-send check as the service sees it: an optional provider wrapped in
/// the policy.
#[derive(Clone)]
pub struct RiskGate {
    provider: Option<Arc<dyn RiskProvider>>,
    policy: RiskPolicy,
    in_flight: Arc<AtomicUsize>,
}

impl Default for RiskGate {
    fn default() -> Self {
        Self::disabled()
    }
}

impl std::fmt::Debug for RiskGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RiskGate")
            .field("provider", &self.provider.as_ref().map(|p| p.name()))
            .field("policy", &self.policy)
            .finish()
    }
}

impl RiskGate {
    /// No provider: no check runs, nothing is recorded.
    pub fn disabled() -> Self {
        Self {
            provider: None,
            policy: RiskPolicy::default(),
            in_flight: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn new(provider: Arc<dyn RiskProvider>, policy: RiskPolicy) -> Self {
        Self {
            provider: Some(provider),
            policy,
            in_flight: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.provider.is_some()
    }

    /// The configured provider's name, if any.
    pub fn provider_name(&self) -> Option<&'static str> {
        self.provider.as_ref().map(|p| p.name())
    }

    pub fn policy(&self) -> RiskPolicy {
        self.policy
    }

    /// Run the check. `None` when no provider is configured.
    pub fn check(&self, input: &RiskCheckInput) -> Option<RiskVerdict> {
        let provider = self.provider.as_ref()?;
        let name = provider.name();
        let started = Instant::now();

        // Reserve a slot in the concurrency budget, or do not call at all.
        let reserved = self
            .in_flight
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < self.policy.max_in_flight).then_some(n + 1)
            })
            .is_ok();
        if !reserved {
            return Some(self.policy_verdict(RiskSource::BudgetExhausted, name, started));
        }

        let (tx, rx) = mpsc::channel();
        let worker_provider = Arc::clone(provider);
        let worker_input = input.clone();
        let timeout = self.policy.timeout;
        let slot = InFlightSlot(Arc::clone(&self.in_flight));
        let spawned = std::thread::Builder::new()
            .name("risk-check".to_string())
            .spawn(move || {
                // Released when the provider returns, not when the gate stops
                // waiting: that is what makes the budget bound real threads.
                let _slot = slot;
                let result = worker_provider.check(&worker_input, timeout);
                if tx.send(result).is_err() {
                    // The gate already stopped waiting (timeout). A late
                    // answer is discarded by design: the policy decided.
                }
            });
        if spawned.is_err() {
            // The closure (and with it the slot) was dropped, so the budget
            // is already released. No thread, no check: the policy decides.
            return Some(self.policy_verdict(RiskSource::Error, name, started));
        }

        Some(match rx.recv_timeout(timeout) {
            Ok(Ok(decision)) => {
                let (outcome, reason) = match decision {
                    RiskDecision::Allow => (RiskOutcome::Allow, None),
                    RiskDecision::Hold { reason } => {
                        (RiskOutcome::Hold, Some(sanitize_reason(&reason)))
                    }
                    RiskDecision::Refuse { reason } => {
                        (RiskOutcome::Refuse, Some(sanitize_reason(&reason)))
                    }
                };
                RiskVerdict {
                    outcome,
                    reason,
                    source: RiskSource::Provider,
                    provider: name,
                    elapsed_ms: elapsed_ms(started),
                }
            }
            Ok(Err(_)) => self.policy_verdict(RiskSource::Error, name, started),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.policy_verdict(RiskSource::Timeout, name, started)
            }
            // The worker died without answering (a provider panic).
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.policy_verdict(RiskSource::Error, name, started)
            }
        })
    }

    fn policy_verdict(
        &self,
        source: RiskSource,
        provider: &'static str,
        started: Instant,
    ) -> RiskVerdict {
        let outcome = match self.policy.on_unavailable {
            OnUnavailable::Hold => RiskOutcome::Hold,
            OnUnavailable::Refuse => RiskOutcome::Refuse,
            OnUnavailable::Allow => RiskOutcome::Allow,
        };
        RiskVerdict {
            outcome,
            // The reason is the source itself: a fixed code, never the error
            // text.
            reason: (outcome != RiskOutcome::Allow)
                .then(|| format!("risk_check_{}", source.name())),
            source,
            provider,
            elapsed_ms: elapsed_ms(started),
        }
    }
}

/// Decrements the in-flight counter when dropped.
struct InFlightSlot(Arc<AtomicUsize>);

impl Drop for InFlightSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// The code recorded when a provider's reason fails [`sanitize_reason`].
pub const UNSPECIFIED_REASON: &str = "unspecified";

/// Reduce a provider's reason to something safe to persist.
///
/// Accepted: 1–64 characters of `[a-z0-9_.-]`, starting with a letter, with no
/// run of five or more digits. The digit rule is what keeps an account
/// number, a routing number or a phone number out of the audit trail even if
/// a provider puts one in its reason; the charset rule keeps out names, free
/// text and payload fragments. Anything else becomes [`UNSPECIFIED_REASON`].
pub fn sanitize_reason(reason: &str) -> String {
    let well_formed = !reason.is_empty()
        && reason.len() <= 64
        && reason.starts_with(|c: char| c.is_ascii_lowercase())
        && reason
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_.-".contains(c));
    let mut digit_run = 0usize;
    let long_digit_run = reason.chars().any(|c| {
        digit_run = if c.is_ascii_digit() { digit_run + 1 } else { 0 };
        digit_run >= 5
    });
    if well_formed && !long_digit_run {
        reason.to_string()
    } else {
        UNSPECIFIED_REASON.to_string()
    }
}

/// A risk provider backed by `fednow-sim`'s **demo** endpoint
/// (`POST /demo/risk-check`), so the quickstart can show hold, refuse and
/// timeout.
///
/// The endpoint's request and response are this project's own invention for
/// demonstration. They are **not** the Network Intelligence API's contract,
/// which is not public, and this provider is not a client for it.
pub struct SimRiskProvider {
    base_url: String,
}

impl SimRiskProvider {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }
}

/// How long past the gate's timeout [`SimRiskProvider`] keeps its connection.
const TRANSPORT_GRACE: Duration = Duration::from_millis(250);

#[derive(Serialize)]
struct SimRiskRequest<'a> {
    amount_cents: u64,
    creditor_agent_routing_number: &'a str,
}

#[derive(Deserialize)]
struct SimRiskResponse {
    decision: String,
    reason: Option<String>,
}

impl RiskProvider for SimRiskProvider {
    fn name(&self) -> &'static str {
        "sim"
    }

    fn check(&self, input: &RiskCheckInput, timeout: Duration) -> Result<RiskDecision, RiskError> {
        let url = format!("{}/demo/risk-check", self.base_url);
        // Only what the demo endpoint needs; the account stays here.
        let body = serde_json::to_string(&SimRiskRequest {
            amount_cents: input.amount_cents,
            creditor_agent_routing_number: &input.creditor_agent_routing_number,
        })
        .map_err(|e| RiskError::InvalidResponse(e.to_string()))?;
        let mut resp = ureq::post(&url)
            .config()
            .http_status_as_error(false)
            // Our own deadline too, so the worker thread (and its budget
            // slot) is released soon after the gate stops waiting. A little
            // past the gate's, so a slow answer is reported as a timeout by
            // the gate rather than as a transport error by us.
            .timeout_global(Some(timeout + TRANSPORT_GRACE))
            .build()
            .header("content-type", "application/json")
            .send(body.as_str())
            .map_err(|e| RiskError::Unavailable(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| RiskError::Unavailable(e.to_string()))?;
        if status != 200 {
            return Err(RiskError::Unavailable(format!("HTTP {status}")));
        }
        let parsed: SimRiskResponse =
            serde_json::from_str(&text).map_err(|e| RiskError::InvalidResponse(e.to_string()))?;
        let reason = parsed.reason.unwrap_or_default();
        match parsed.decision.as_str() {
            "allow" => Ok(RiskDecision::Allow),
            "hold" => Ok(RiskDecision::Hold { reason }),
            "refuse" => Ok(RiskDecision::Refuse { reason }),
            other => Err(RiskError::InvalidResponse(format!(
                "unknown decision '{}'",
                other.chars().take(16).collect::<String>()
            ))),
        }
    }
}

/// Why the risk configuration was refused at startup. The gateway exits
/// rather than run with a check the operator did not intend.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RiskConfigError {
    #[error("FEDNOW_GW_RISK_PROVIDER must be 'none' or 'sim', found '{0}'")]
    UnknownProvider(String),
    #[error("FEDNOW_GW_RISK_ON_UNAVAILABLE must be 'hold', 'refuse' or 'allow', found '{0}'")]
    UnknownPolicy(String),
    #[error("{0} must be a positive integer, found '{1}'")]
    NotPositive(&'static str, String),
}

/// Build the gate from environment-style lookups (a closure, so tests need no
/// process environment). `sim_url` is the default for the sim provider's base
/// URL.
pub fn gate_from_lookup(
    lookup: impl Fn(&str) -> Option<String>,
    sim_url: &str,
) -> Result<RiskGate, RiskConfigError> {
    let provider = lookup("FEDNOW_GW_RISK_PROVIDER").unwrap_or_default();
    let provider: Arc<dyn RiskProvider> = match provider.trim() {
        "" | "none" => return Ok(RiskGate::disabled()),
        "sim" => Arc::new(SimRiskProvider::new(
            lookup("FEDNOW_GW_RISK_SIM_URL").unwrap_or_else(|| sim_url.to_string()),
        )),
        other => return Err(RiskConfigError::UnknownProvider(other.to_string())),
    };

    let on_unavailable = match lookup("FEDNOW_GW_RISK_ON_UNAVAILABLE") {
        None => OnUnavailable::Hold,
        Some(v) => OnUnavailable::parse(v.trim()).ok_or(RiskConfigError::UnknownPolicy(v))?,
    };
    let positive = |name: &'static str, default: u64| match lookup(name) {
        None => Ok(default),
        Some(v) => match v.trim().parse::<u64>() {
            Ok(n) if n > 0 => Ok(n),
            _ => Err(RiskConfigError::NotPositive(name, v)),
        },
    };
    let timeout_ms = positive("FEDNOW_GW_RISK_TIMEOUT_MS", DEFAULT_TIMEOUT_MS)?;
    let max_in_flight = positive("FEDNOW_GW_RISK_MAX_IN_FLIGHT", DEFAULT_MAX_IN_FLIGHT as u64)?;

    Ok(RiskGate::new(
        provider,
        RiskPolicy {
            timeout: Duration::from_millis(timeout_ms),
            max_in_flight: usize::try_from(max_in_flight).unwrap_or(usize::MAX),
            on_unavailable,
        },
    ))
}
