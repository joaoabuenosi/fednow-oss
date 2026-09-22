//! fednow-gateway — send middleware for the FedNow Service.
//!
//! v0 is the domain core: the per-payment state machine from the project
//! design, persisted as immutable events —
//!
//! ```text
//! CREATED → VALIDATED → SUBMITTED → ACK_PENDING → SETTLED
//!               │                        │        → REJECTED
//!               │                        └──────→ TIMEOUT_UNRESOLVED
//!               └─(pre-send risk check)─→ REFUSED            (never sent)
//!                                        → HELD ─(release)─→ SUBMITTED → …
//!                                               └(cancel / expiry)─→ CANCELLED (never sent)
//! ```
//!
//! The pre-send risk check ([`risk`]) is optional and off by default; with it
//! off, the `HELD`/`REFUSED`/`CANCELLED` branch does not exist. How a hold is
//! released or cancelled, and why a release sends the parked message rather
//! than a rebuilt one, is in [`hold`].
//!
//! `TIMEOUT_UNRESOLVED` is a work item, not a terminal verdict: the
//! [`reconciler`] decides when to declare it and when to send a payment status
//! request (pacs.028) — never a blind resend (see the handbook's timeout
//! reconciliation chapter).
//!
//! Everything here is pure and deterministic: events carry caller-provided
//! unix timestamps, state is a fold over events, and the in-memory
//! [`store::PaymentStore`] enforces idempotency-keyed creation. Ports
//! (REST/gRPC northbound, MQ southbound), durable storage and the outbox
//! publisher arrive in later iterations on top of this core.

pub mod auth;
pub mod hold;
pub mod http;
pub mod payment;
pub mod reconciler;
pub mod risk;
pub mod service;
pub mod southbound;
pub mod sqlite;
pub mod store;

pub use auth::{Access, ApiKeys, AuthConfigError, Caller, Role, RouteSpec};
pub use hold::{HoldConfigError, HoldPolicy};
pub use payment::{
    advice_from_pacs002, message_sha256, AdviceStatus, HoldAction, HoldResolution, Payment,
    PaymentEvent, PaymentState,
};
pub use reconciler::{reconciliation_action, ReconciliationAction};
pub use risk::{
    OnUnavailable, RiskCheckInput, RiskDecision, RiskError, RiskGate, RiskOutcome, RiskPolicy,
    RiskProvider, RiskSource, RiskVerdict, SimRiskProvider,
};
pub use service::{OpsSummary, PaymentService, ServiceError, SubmitRequest};
pub use southbound::{AnyPort, FedNowPort, HttpSimPort, MqSimPort, PortError, SubmitOutcome};
pub use sqlite::SqliteStore;
pub use store::{CreateOutcome, InMemoryStore, OutboxEntry, PaymentStore};
