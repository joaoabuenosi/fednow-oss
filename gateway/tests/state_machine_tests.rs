//! The payment state machine, exercised end to end: happy path, rejection,
//! the timeout/reconciliation arc, idempotent creation and illegal moves.

use fednow_core::builder::Pacs002Builder;
use fednow_core::pacs002;
use fednow_gateway::{
    advice_from_pacs002, reconciliation_action, AdviceStatus, CreateOutcome, InMemoryStore,
    Payment, PaymentEvent, PaymentState, PaymentStore, ReconciliationAction,
};

fn created(key: &str) -> PaymentEvent {
    PaymentEvent::Created {
        idempotency_key: key.to_string(),
        message_identification: "20260702021040078GW00000001".to_string(),
        end_to_end_identification: "E2E-GW-0001".to_string(),
        uetr: Some("8a562c67-ca16-48ba-b074-65581be6f001".to_string()),
        amount_cents: 125_000,
        at_unix: 1_000,
    }
}

fn to_ack_pending(payment: &mut Payment) {
    payment
        .apply(PaymentEvent::Validated { at_unix: 1_001 })
        .unwrap();
    payment
        .apply(PaymentEvent::Submitted { at_unix: 1_002 })
        .unwrap();
    payment
        .apply(PaymentEvent::Published { at_unix: 1_003 })
        .unwrap();
}

#[test]
fn happy_path_reaches_settled() {
    let mut p = Payment::new(created("k1")).unwrap();
    to_ack_pending(&mut p);
    assert_eq!(p.state, PaymentState::AckPending);
    assert_eq!(p.published_at_unix, Some(1_003));

    // Interim acceptance keeps us waiting…
    p.apply(PaymentEvent::AdviceReceived {
        status: AdviceStatus::Actc,
        reason: None,
        at_unix: 1_004,
    })
    .unwrap();
    assert_eq!(p.state, PaymentState::AckPending);

    // …the settlement advice resolves.
    p.apply(PaymentEvent::AdviceReceived {
        status: AdviceStatus::Acsc,
        reason: None,
        at_unix: 1_005,
    })
    .unwrap();
    assert_eq!(p.state, PaymentState::Settled);

    // A later ACCC confirmation is recorded, state holds.
    p.apply(PaymentEvent::AdviceReceived {
        status: AdviceStatus::Accc,
        reason: None,
        at_unix: 1_010,
    })
    .unwrap();
    assert_eq!(p.state, PaymentState::Settled);
    assert_eq!(p.last_advice, Some(AdviceStatus::Accc));
}

#[test]
fn rejection_carries_the_reason() {
    let mut p = Payment::new(created("k2")).unwrap();
    to_ack_pending(&mut p);
    p.apply(PaymentEvent::AdviceReceived {
        status: AdviceStatus::Rjct,
        reason: Some("AC04".to_string()),
        at_unix: 1_004,
    })
    .unwrap();
    assert_eq!(p.state, PaymentState::Rejected);
    assert_eq!(p.rejection_reason.as_deref(), Some("AC04"));
}

#[test]
fn timeout_arc_resolves_via_query_advice() {
    let mut p = Payment::new(created("k3")).unwrap();
    to_ack_pending(&mut p);

    // Inside the window: nothing to do.
    assert_eq!(
        reconciliation_action(&p, 1_010, 30, 60),
        ReconciliationAction::None
    );
    // Past the presumed timeout: declare.
    assert_eq!(
        reconciliation_action(&p, 1_033, 30, 60),
        ReconciliationAction::DeclareTimeout
    );
    p.apply(PaymentEvent::TimeoutDeclared { at_unix: 1_033 })
        .unwrap();
    assert_eq!(p.state, PaymentState::TimeoutUnresolved);

    // First query is due immediately; then backoff applies.
    assert_eq!(
        reconciliation_action(&p, 1_034, 30, 60),
        ReconciliationAction::SendQuery
    );
    p.apply(PaymentEvent::QuerySent { at_unix: 1_034 }).unwrap();
    assert_eq!(
        reconciliation_action(&p, 1_050, 30, 60),
        ReconciliationAction::None
    );
    assert_eq!(
        reconciliation_action(&p, 1_100, 30, 60),
        ReconciliationAction::SendQuery
    );

    // The query's answer — it settled all along — resolves the alarm state.
    p.apply(PaymentEvent::AdviceReceived {
        status: AdviceStatus::Acsc,
        reason: None,
        at_unix: 1_101,
    })
    .unwrap();
    assert_eq!(p.state, PaymentState::Settled);
    assert_eq!(p.queries_sent, 1);
}

#[test]
fn advice_before_publication_is_illegal() {
    let mut p = Payment::new(created("k4")).unwrap();
    let err = p
        .apply(PaymentEvent::AdviceReceived {
            status: AdviceStatus::Acsc,
            reason: None,
            at_unix: 1_001,
        })
        .unwrap_err();
    assert_eq!(err.state, PaymentState::Created);
}

#[test]
fn replay_is_deterministic() {
    let mut p = Payment::new(created("k5")).unwrap();
    to_ack_pending(&mut p);
    p.apply(PaymentEvent::AdviceReceived {
        status: AdviceStatus::Acwp,
        reason: None,
        at_unix: 1_004,
    })
    .unwrap();

    let replayed = Payment::replay(p.events.clone()).unwrap();
    assert_eq!(replayed.state, p.state);
    assert_eq!(replayed.state, PaymentState::Settled); // ACWP = funds settled
    assert_eq!(replayed.last_advice, Some(AdviceStatus::Acwp));
}

#[test]
fn creation_is_idempotent_by_key() {
    let store = InMemoryStore::new();
    let first = store.create(created("same-key")).unwrap();
    assert!(matches!(first, CreateOutcome::Created(_)));

    let second = store.create(created("same-key")).unwrap();
    match second {
        CreateOutcome::Existing(p) => assert_eq!(p.state, PaymentState::Created),
        other => panic!("expected Existing, got {other:?}"),
    }

    // Progress persists through the store, and replays on load.
    store
        .append("same-key", PaymentEvent::Validated { at_unix: 1_001 })
        .unwrap();
    let loaded = store.load("same-key").unwrap();
    assert_eq!(loaded.state, PaymentState::Validated);
}

#[test]
fn illegal_append_does_not_corrupt_the_stream() {
    let store = InMemoryStore::new();
    store.create(created("k6")).unwrap();
    let err = store.append(
        "k6",
        PaymentEvent::Published { at_unix: 1_002 }, // must validate+submit first
    );
    assert!(err.is_err());
    assert_eq!(store.load("k6").unwrap().state, PaymentState::Created);
}

#[test]
fn advices_map_from_real_pacs002_documents() {
    // A service advice built with fednow-core feeds the state machine directly.
    let xml = Pacs002Builder::new(
        "FEDNOWSVCADVICE000000000000009",
        "2026-07-02T15:30:05Z",
        "20260702021040078GW00000001",
        "2026-07-02T15:30:00Z",
        "RJCT",
        "021040078",
        "091000019",
    )
    .reason_code("AC04")
    .to_xml()
    .unwrap();
    let doc = pacs002::parse(&xml).unwrap();
    let (status, reason) = advice_from_pacs002(&doc).unwrap();
    assert_eq!(status, AdviceStatus::Rjct);
    assert_eq!(reason.as_deref(), Some("AC04"));

    let mut p = Payment::new(created("k7")).unwrap();
    to_ack_pending(&mut p);
    p.apply(PaymentEvent::AdviceReceived {
        status,
        reason,
        at_unix: 1_004,
    })
    .unwrap();
    assert_eq!(p.state, PaymentState::Rejected);
}
