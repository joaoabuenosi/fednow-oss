//! Event storage with idempotency-keyed creation.
//!
//! The northbound API requires an idempotency key on creation: submitting the
//! same key twice must return the existing payment, never create a second one.
//! v0 ships an in-memory implementation; a durable store implements the same
//! trait later (the outbox lives at that layer too).

use std::collections::HashMap;
use std::sync::Mutex;

use crate::payment::{Payment, PaymentEvent, TransitionError};

/// Result of an idempotency-keyed create.
#[derive(Debug)]
pub enum CreateOutcome {
    /// First time this key was seen; the payment was created.
    Created(Payment),
    /// The key already exists; here is the payment as it stands. No new
    /// payment was created.
    Existing(Payment),
}

/// An outbox entry: a message waiting for confirmed handoff to the transport.
#[derive(Debug, Clone)]
pub struct OutboxEntry {
    pub id: i64,
    pub idempotency_key: String,
    pub message_xml: String,
}

/// Storage for payment event streams, keyed by idempotency key.
pub trait PaymentStore {
    /// Create a payment from its `Created` event, idempotently.
    fn create(&self, event: PaymentEvent) -> Result<CreateOutcome, TransitionError>;
    /// Append an event to an existing payment, enforcing transitions.
    fn append(
        &self,
        idempotency_key: &str,
        event: PaymentEvent,
    ) -> Result<Payment, TransitionError>;
    /// Load a payment by idempotency key.
    fn load(&self, idempotency_key: &str) -> Option<Payment>;
    /// All known idempotency keys (the reconciler sweeps them).
    fn keys(&self) -> Vec<String>;

    /// The outbox pattern's atomic step: append `event` (normally `Submitted`)
    /// AND enqueue `message_xml` for publication, in one transaction. Either
    /// both are durable or neither is.
    fn submit_to_outbox(
        &self,
        idempotency_key: &str,
        event: PaymentEvent,
        message_xml: String,
    ) -> Result<Payment, TransitionError>;
    /// The oldest unpublished outbox entry, if any.
    fn next_unpublished(&self) -> Option<OutboxEntry>;
    /// Mark an outbox entry as published (confirmed handoff).
    fn mark_published(&self, outbox_id: i64);
}

/// In-memory store: a mutexed map of event streams plus an outbox queue.
/// One mutex guards both, so `submit_to_outbox` is atomic by construction.
#[derive(Default)]
pub struct InMemoryStore {
    inner: Mutex<InMemoryInner>,
}

#[derive(Default)]
struct InMemoryInner {
    streams: HashMap<String, Vec<PaymentEvent>>,
    outbox: Vec<(i64, String, String, bool)>, // (id, key, xml, published)
    next_outbox_id: i64,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

fn append_to(
    streams: &mut HashMap<String, Vec<PaymentEvent>>,
    idempotency_key: &str,
    event: PaymentEvent,
) -> Result<Payment, TransitionError> {
    let stream = streams
        .get_mut(idempotency_key)
        .ok_or_else(|| TransitionError {
            state: crate::payment::PaymentState::Created,
            event: format!("append to unknown key '{idempotency_key}'"),
        })?;
    // Validate the transition on a replayed aggregate before persisting.
    let mut payment = Payment::replay(stream.clone())?;
    payment.apply(event.clone())?;
    stream.push(event);
    Ok(payment)
}

impl PaymentStore for InMemoryStore {
    fn create(&self, event: PaymentEvent) -> Result<CreateOutcome, TransitionError> {
        let key = match &event {
            PaymentEvent::Created {
                idempotency_key, ..
            } => idempotency_key.clone(),
            other => {
                return Err(TransitionError {
                    state: crate::payment::PaymentState::Created,
                    event: format!("{other:?} passed to create"),
                })
            }
        };
        let mut inner = self.inner.lock().unwrap();
        if let Some(existing) = inner.streams.get(&key) {
            return Ok(CreateOutcome::Existing(Payment::replay(existing.clone())?));
        }
        let payment = Payment::new(event.clone())?;
        inner.streams.insert(key, vec![event]);
        Ok(CreateOutcome::Created(payment))
    }

    fn append(
        &self,
        idempotency_key: &str,
        event: PaymentEvent,
    ) -> Result<Payment, TransitionError> {
        let mut inner = self.inner.lock().unwrap();
        append_to(&mut inner.streams, idempotency_key, event)
    }

    fn load(&self, idempotency_key: &str) -> Option<Payment> {
        let inner = self.inner.lock().unwrap();
        let stream = inner.streams.get(idempotency_key)?;
        Payment::replay(stream.clone()).ok()
    }

    fn keys(&self) -> Vec<String> {
        self.inner.lock().unwrap().streams.keys().cloned().collect()
    }

    fn submit_to_outbox(
        &self,
        idempotency_key: &str,
        event: PaymentEvent,
        message_xml: String,
    ) -> Result<Payment, TransitionError> {
        let mut inner = self.inner.lock().unwrap();
        // Same lock covers both structures: atomic by construction.
        let payment = append_to(&mut inner.streams, idempotency_key, event)?;
        let id = inner.next_outbox_id;
        inner.next_outbox_id += 1;
        inner
            .outbox
            .push((id, idempotency_key.to_string(), message_xml, false));
        Ok(payment)
    }

    fn next_unpublished(&self) -> Option<OutboxEntry> {
        let inner = self.inner.lock().unwrap();
        inner
            .outbox
            .iter()
            .find(|(_, _, _, published)| !published)
            .map(|(id, key, xml, _)| OutboxEntry {
                id: *id,
                idempotency_key: key.clone(),
                message_xml: xml.clone(),
            })
    }

    fn mark_published(&self, outbox_id: i64) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(entry) = inner.outbox.iter_mut().find(|(id, ..)| *id == outbox_id) {
            entry.3 = true;
        }
    }
}
