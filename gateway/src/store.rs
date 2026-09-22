//! Event storage with idempotency-keyed creation.
//!
//! The northbound API requires an idempotency key on creation: submitting the
//! same key twice must return the existing payment, never create a second one.
//! v0 ships an in-memory implementation; a durable store implements the same
//! trait later (the outbox lives at that layer too).

use std::collections::HashMap;
use std::sync::Mutex;

use crate::payment::{message_sha256, Payment, PaymentEvent, PaymentState, TransitionError};

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
    /// How many outbox entries still await publication (ops visibility:
    /// a growing number means the transport is down or refusing).
    fn unpublished_count(&self) -> usize;

    /// Park a held payment's built message outside the outbox: append
    /// `events` (the `RiskChecked` hold, then `HoldParked`) AND store
    /// `message_xml` under the key, in one transaction.
    ///
    /// Nothing reads parked messages but [`Self::release_parked`], so a
    /// parked message cannot reach the wire any other way.
    fn park(
        &self,
        idempotency_key: &str,
        events: Vec<PaymentEvent>,
        message_xml: String,
    ) -> Result<Payment, TransitionError>;
    /// The parked message of a held payment, if one is on record.
    fn parked_message(&self, idempotency_key: &str) -> Option<String>;
    /// Release a held payment, in one transaction: append `event` (a
    /// `HoldReleased`), take the parked message out of parking and enqueue
    /// it in the outbox. Refused — and nothing changes — when there is no
    /// parked message, when its SHA-256 is not the one `event` records, when
    /// the transition is illegal (not `HELD`, already released), or when the
    /// payment already has an outbox entry.
    fn release_parked(
        &self,
        idempotency_key: &str,
        event: PaymentEvent,
    ) -> Result<Payment, TransitionError>;
    /// Cancel a held payment, in one transaction: append `event` (a
    /// `HoldCancelled`) and delete the parked message, if any. A message that
    /// can never be sent is not kept.
    fn discard_parked(
        &self,
        idempotency_key: &str,
        event: PaymentEvent,
    ) -> Result<Payment, TransitionError>;
}

/// The digest a `HoldReleased` event claims for the message it sends.
pub(crate) fn released_digest(event: &PaymentEvent) -> Result<&str, TransitionError> {
    match event {
        PaymentEvent::HoldReleased { message_sha256, .. } => Ok(message_sha256),
        other => Err(TransitionError {
            state: PaymentState::Held,
            event: format!("{other:?} passed to release_parked"),
        }),
    }
}

/// Refuse to move a parked message whose bytes are not the ones recorded.
pub(crate) fn check_parked(
    message_xml: &str,
    expected_sha256: &str,
) -> Result<(), TransitionError> {
    if message_sha256(message_xml) == expected_sha256 {
        Ok(())
    } else {
        Err(TransitionError {
            state: PaymentState::Held,
            event: "parked message does not match its recorded digest".to_string(),
        })
    }
}

pub(crate) fn no_parked_message(idempotency_key: &str) -> TransitionError {
    TransitionError {
        state: PaymentState::Held,
        event: format!("no parked message for '{idempotency_key}'"),
    }
}

pub(crate) fn already_in_outbox(idempotency_key: &str) -> TransitionError {
    TransitionError {
        state: PaymentState::Submitted,
        event: format!("'{idempotency_key}' already has an outbox entry"),
    }
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
    parked: HashMap<String, String>,
}

impl InMemoryInner {
    fn enqueue(&mut self, idempotency_key: &str, message_xml: String) {
        let id = self.next_outbox_id;
        self.next_outbox_id += 1;
        self.outbox
            .push((id, idempotency_key.to_string(), message_xml, false));
    }

    fn in_outbox(&self, idempotency_key: &str) -> bool {
        self.outbox
            .iter()
            .any(|(_, key, ..)| key == idempotency_key)
    }
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
        // One outbox entry per payment, as the SQLite store's unique index.
        if inner.in_outbox(idempotency_key) {
            return Err(already_in_outbox(idempotency_key));
        }
        // Same lock covers both structures: atomic by construction.
        let payment = append_to(&mut inner.streams, idempotency_key, event)?;
        inner.enqueue(idempotency_key, message_xml);
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

    fn unpublished_count(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner
            .outbox
            .iter()
            .filter(|(_, _, _, published)| !published)
            .count()
    }

    fn park(
        &self,
        idempotency_key: &str,
        events: Vec<PaymentEvent>,
        message_xml: String,
    ) -> Result<Payment, TransitionError> {
        let mut inner = self.inner.lock().unwrap();
        // Validate every transition on a copy before touching the stream, so
        // a refusal leaves nothing behind (as the SQLite transaction does).
        let stream =
            inner
                .streams
                .get(idempotency_key)
                .cloned()
                .ok_or_else(|| TransitionError {
                    state: PaymentState::Created,
                    event: format!("park for unknown key '{idempotency_key}'"),
                })?;
        let mut payment = Payment::replay(stream)?;
        for event in &events {
            payment.apply(event.clone())?;
        }
        if let Some(stream) = inner.streams.get_mut(idempotency_key) {
            stream.extend(events);
        }
        inner
            .parked
            .insert(idempotency_key.to_string(), message_xml);
        Ok(payment)
    }

    fn parked_message(&self, idempotency_key: &str) -> Option<String> {
        self.inner
            .lock()
            .unwrap()
            .parked
            .get(idempotency_key)
            .cloned()
    }

    fn release_parked(
        &self,
        idempotency_key: &str,
        event: PaymentEvent,
    ) -> Result<Payment, TransitionError> {
        let mut inner = self.inner.lock().unwrap();
        let expected = released_digest(&event)?;
        let message_xml = inner
            .parked
            .get(idempotency_key)
            .cloned()
            .ok_or_else(|| no_parked_message(idempotency_key))?;
        check_parked(&message_xml, expected)?;
        if inner.in_outbox(idempotency_key) {
            return Err(already_in_outbox(idempotency_key));
        }
        let payment = append_to(&mut inner.streams, idempotency_key, event)?;
        inner.parked.remove(idempotency_key);
        inner.enqueue(idempotency_key, message_xml);
        Ok(payment)
    }

    fn discard_parked(
        &self,
        idempotency_key: &str,
        event: PaymentEvent,
    ) -> Result<Payment, TransitionError> {
        let mut inner = self.inner.lock().unwrap();
        let payment = append_to(&mut inner.streams, idempotency_key, event)?;
        inner.parked.remove(idempotency_key);
        Ok(payment)
    }
}
