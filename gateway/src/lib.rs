//! fednow-gateway — send middleware for the FedNow Service.
//!
//! v0 is the domain core: the per-payment state machine from the project
//! design, persisted as immutable events —
//!
//! ```text
//! CREATED → VALIDATED → SUBMITTED → ACK_PENDING → SETTLED
//!                                        │        → REJECTED
//!                                        └──────→ TIMEOUT_UNRESOLVED
//! ```
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

pub mod payment;
pub mod reconciler;
pub mod service;
pub mod southbound;
pub mod store;

pub use payment::{advice_from_pacs002, AdviceStatus, Payment, PaymentEvent, PaymentState};
pub use reconciler::{reconciliation_action, ReconciliationAction};
pub use service::{PaymentService, ServiceError, SubmitRequest};
pub use southbound::{FedNowPort, HttpSimPort, PortError, SubmitOutcome};
pub use store::{CreateOutcome, InMemoryStore, PaymentStore};
