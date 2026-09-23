//! Durable storage: SQLite (bundled — zero system dependencies).
//!
//! Events are JSON rows in an append-only table; the outbox is a second table
//! written **in the same transaction** as the `Submitted` event — the outbox
//! pattern's whole point. Reopening the database replays every payment
//! exactly as it was: the "persist before you send" promise of the handbook.
//!
//! A held payment's built message waits in a third table, `held_messages`,
//! written in the same transaction as the hold and moved to the outbox in the
//! same transaction as the release. The outbox carries a unique index on the
//! idempotency key, so no path — a double release, a race, a bug — can give
//! one payment two outbox entries: the database refuses the second insert.

use std::sync::Mutex;

use rusqlite::{params, Connection};

use crate::payment::{Payment, PaymentEvent, PaymentState, TransitionError};
use crate::store::{
    already_in_outbox, check_parked, no_parked_message, released_digest, CreateOutcome,
    OutboxEntry, PaymentStore,
};

pub struct SqliteStore {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS payment_events (
    idempotency_key TEXT NOT NULL,
    seq             INTEGER NOT NULL,
    event           TEXT NOT NULL,
    PRIMARY KEY (idempotency_key, seq)
);
CREATE TABLE IF NOT EXISTS outbox (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    idempotency_key TEXT NOT NULL,
    message_xml     TEXT NOT NULL,
    published       INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX IF NOT EXISTS outbox_one_entry_per_payment
    ON outbox (idempotency_key);
CREATE TABLE IF NOT EXISTS held_messages (
    idempotency_key TEXT PRIMARY KEY,
    message_xml     TEXT NOT NULL
);
";

impl SqliteStore {
    /// Open (or create) a database file.
    pub fn open(path: &str) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// An in-memory database (tests, throwaway runs).
    pub fn in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn storage_error(detail: String) -> TransitionError {
        TransitionError {
            state: PaymentState::Created,
            event: format!("storage: {detail}"),
        }
    }
}

fn load_stream(conn: &Connection, key: &str) -> Result<Vec<PaymentEvent>, String> {
    let mut stmt = conn
        .prepare("SELECT event FROM payment_events WHERE idempotency_key = ?1 ORDER BY seq")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![key], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut events = Vec::new();
    for row in rows {
        let json = row.map_err(|e| e.to_string())?;
        events.push(serde_json::from_str(&json).map_err(|e| e.to_string())?);
    }
    Ok(events)
}

/// Append `event` to the stream of `key` inside `tx`, enforcing the
/// transition on the replayed aggregate.
fn append_in(
    conn: &Connection,
    key: &str,
    event: &PaymentEvent,
) -> Result<Payment, TransitionError> {
    let stream = load_stream(conn, key).map_err(SqliteStore::storage_error)?;
    if stream.is_empty() {
        return Err(SqliteStore::storage_error(format!(
            "append to unknown key '{key}'"
        )));
    }
    let seq = stream.len();
    let mut payment = Payment::replay(stream)?;
    payment.apply(event.clone())?;
    insert_event(conn, key, seq, event).map_err(SqliteStore::storage_error)?;
    Ok(payment)
}

fn outbox_has(conn: &Connection, key: &str) -> Result<bool, TransitionError> {
    conn.query_row(
        "SELECT COUNT(*) FROM outbox WHERE idempotency_key = ?1",
        params![key],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .map_err(|e| SqliteStore::storage_error(e.to_string()))
}

fn insert_event(
    conn: &Connection,
    key: &str,
    seq: usize,
    event: &PaymentEvent,
) -> Result<(), String> {
    let json = serde_json::to_string(event).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO payment_events (idempotency_key, seq, event) VALUES (?1, ?2, ?3)",
        params![key, seq as i64, json],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

impl PaymentStore for SqliteStore {
    fn create(&self, event: PaymentEvent) -> Result<CreateOutcome, TransitionError> {
        let key = match &event {
            PaymentEvent::Created {
                idempotency_key, ..
            } => idempotency_key.clone(),
            other => {
                return Err(Self::storage_error(format!("{other:?} passed to create")));
            }
        };
        let mut conn = self.conn.lock().unwrap();
        let tx = conn
            .transaction()
            .map_err(|e| Self::storage_error(e.to_string()))?;
        let existing = load_stream(&tx, &key).map_err(Self::storage_error)?;
        if !existing.is_empty() {
            let payment = Payment::replay(existing)?;
            drop(tx);
            return Ok(CreateOutcome::Existing(payment));
        }
        let payment = Payment::new(event.clone())?;
        insert_event(&tx, &key, 0, &event).map_err(Self::storage_error)?;
        tx.commit()
            .map_err(|e| Self::storage_error(e.to_string()))?;
        Ok(CreateOutcome::Created(payment))
    }

    fn append(
        &self,
        idempotency_key: &str,
        event: PaymentEvent,
    ) -> Result<Payment, TransitionError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn
            .transaction()
            .map_err(|e| Self::storage_error(e.to_string()))?;
        let stream = load_stream(&tx, idempotency_key).map_err(Self::storage_error)?;
        if stream.is_empty() {
            return Err(Self::storage_error(format!(
                "append to unknown key '{idempotency_key}'"
            )));
        }
        let seq = stream.len();
        let mut payment = Payment::replay(stream)?;
        payment.apply(event.clone())?;
        insert_event(&tx, idempotency_key, seq, &event).map_err(Self::storage_error)?;
        tx.commit()
            .map_err(|e| Self::storage_error(e.to_string()))?;
        Ok(payment)
    }

    fn load(&self, idempotency_key: &str) -> Option<Payment> {
        let conn = self.conn.lock().unwrap();
        let stream = load_stream(&conn, idempotency_key).ok()?;
        if stream.is_empty() {
            return None;
        }
        Payment::replay(stream).ok()
    }

    fn keys(&self) -> Vec<String> {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) = conn.prepare("SELECT DISTINCT idempotency_key FROM payment_events")
        else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
            return Vec::new();
        };
        rows.flatten().collect()
    }

    fn submit_to_outbox(
        &self,
        idempotency_key: &str,
        event: PaymentEvent,
        message_xml: String,
    ) -> Result<Payment, TransitionError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn
            .transaction()
            .map_err(|e| Self::storage_error(e.to_string()))?;
        let stream = load_stream(&tx, idempotency_key).map_err(Self::storage_error)?;
        if stream.is_empty() {
            return Err(Self::storage_error(format!(
                "append to unknown key '{idempotency_key}'"
            )));
        }
        let seq = stream.len();
        let mut payment = Payment::replay(stream)?;
        payment.apply(event.clone())?;
        insert_event(&tx, idempotency_key, seq, &event).map_err(Self::storage_error)?;
        tx.execute(
            "INSERT INTO outbox (idempotency_key, message_xml) VALUES (?1, ?2)",
            params![idempotency_key, message_xml],
        )
        .map_err(|e| Self::storage_error(e.to_string()))?;
        tx.commit()
            .map_err(|e| Self::storage_error(e.to_string()))?;
        Ok(payment)
    }

    fn next_unpublished(&self) -> Option<OutboxEntry> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, idempotency_key, message_xml FROM outbox \
             WHERE published = 0 ORDER BY id LIMIT 1",
            [],
            |row| {
                Ok(OutboxEntry {
                    id: row.get(0)?,
                    idempotency_key: row.get(1)?,
                    message_xml: row.get(2)?,
                })
            },
        )
        .ok()
    }

    fn mark_published(&self, outbox_id: i64) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE outbox SET published = 1 WHERE id = ?1",
            params![outbox_id],
        );
    }

    fn unpublished_count(&self) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM outbox WHERE published = 0",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|n| n.max(0) as usize)
        .unwrap_or(0)
    }

    fn park(
        &self,
        idempotency_key: &str,
        events: Vec<PaymentEvent>,
        message_xml: String,
    ) -> Result<Payment, TransitionError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn
            .transaction()
            .map_err(|e| Self::storage_error(e.to_string()))?;
        let mut payment = None;
        for event in &events {
            payment = Some(append_in(&tx, idempotency_key, event)?);
        }
        let payment =
            payment.ok_or_else(|| Self::storage_error("park without events".to_string()))?;
        tx.execute(
            "INSERT INTO held_messages (idempotency_key, message_xml) VALUES (?1, ?2)",
            params![idempotency_key, message_xml],
        )
        .map_err(|e| Self::storage_error(e.to_string()))?;
        tx.commit()
            .map_err(|e| Self::storage_error(e.to_string()))?;
        Ok(payment)
    }

    fn parked_message(&self, idempotency_key: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT message_xml FROM held_messages WHERE idempotency_key = ?1",
            params![idempotency_key],
            |row| row.get::<_, String>(0),
        )
        .ok()
    }

    fn release_parked(
        &self,
        idempotency_key: &str,
        event: PaymentEvent,
    ) -> Result<Payment, TransitionError> {
        let expected = released_digest(&event)?;
        let mut conn = self.conn.lock().unwrap();
        let tx = conn
            .transaction()
            .map_err(|e| Self::storage_error(e.to_string()))?;
        let message_xml: String = match tx.query_row(
            "SELECT message_xml FROM held_messages WHERE idempotency_key = ?1",
            params![idempotency_key],
            |row| row.get(0),
        ) {
            Ok(xml) => xml,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                return Err(no_parked_message(idempotency_key))
            }
            Err(e) => return Err(Self::storage_error(e.to_string())),
        };
        check_parked(&message_xml, expected)?;
        if outbox_has(&tx, idempotency_key)? {
            return Err(already_in_outbox(idempotency_key));
        }
        let payment = append_in(&tx, idempotency_key, &event)?;
        tx.execute(
            "DELETE FROM held_messages WHERE idempotency_key = ?1",
            params![idempotency_key],
        )
        .map_err(|e| Self::storage_error(e.to_string()))?;
        // The unique index makes a second entry impossible even if the checks
        // above were ever bypassed.
        tx.execute(
            "INSERT INTO outbox (idempotency_key, message_xml) VALUES (?1, ?2)",
            params![idempotency_key, message_xml],
        )
        .map_err(|e| Self::storage_error(e.to_string()))?;
        tx.commit()
            .map_err(|e| Self::storage_error(e.to_string()))?;
        Ok(payment)
    }

    fn discard_parked(
        &self,
        idempotency_key: &str,
        event: PaymentEvent,
    ) -> Result<Payment, TransitionError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn
            .transaction()
            .map_err(|e| Self::storage_error(e.to_string()))?;
        let payment = append_in(&tx, idempotency_key, &event)?;
        tx.execute(
            "DELETE FROM held_messages WHERE idempotency_key = ?1",
            params![idempotency_key],
        )
        .map_err(|e| Self::storage_error(e.to_string()))?;
        tx.commit()
            .map_err(|e| Self::storage_error(e.to_string()))?;
        Ok(payment)
    }
}
