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
}

/// In-memory store: a mutexed map of event streams.
#[derive(Default)]
pub struct InMemoryStore {
    streams: Mutex<HashMap<String, Vec<PaymentEvent>>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
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
        let mut streams = self.streams.lock().unwrap();
        if let Some(existing) = streams.get(&key) {
            return Ok(CreateOutcome::Existing(Payment::replay(existing.clone())?));
        }
        let payment = Payment::new(event.clone())?;
        streams.insert(key, vec![event]);
        Ok(CreateOutcome::Created(payment))
    }

    fn append(
        &self,
        idempotency_key: &str,
        event: PaymentEvent,
    ) -> Result<Payment, TransitionError> {
        let mut streams = self.streams.lock().unwrap();
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

    fn load(&self, idempotency_key: &str) -> Option<Payment> {
        let streams = self.streams.lock().unwrap();
        let stream = streams.get(idempotency_key)?;
        Payment::replay(stream.clone()).ok()
    }
}
