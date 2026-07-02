//! fednow-gateway — production send middleware (planned).
//!
//! Roadmap: hexagonal architecture with a REST/gRPC northbound port (mandatory
//! idempotency key on creation), a per-payment state machine
//! (CREATED → VALIDATED → SUBMITTED → ACK_PENDING → SETTLED | REJECTED |
//! TIMEOUT_UNRESOLVED) persisted with event sourcing, an outbox for effective
//! exactly-once publication, a reconciler that resolves ACK_PENDING via pacs.028
//! (never blind resends), pluggable pre-send risk hooks, a double-entry internal
//! ledger and OpenTelemetry. Southbound adapter: IBM MQ + mTLS for the real
//! FedNow Service; fednow-sim in development.
//!
//! Nothing is implemented yet; this crate exists so the workspace layout is final.
