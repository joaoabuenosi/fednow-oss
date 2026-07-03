//! The SQLite store: same contract as in-memory, plus the two things only a
//! durable store can prove — survival across reopen, and the atomic outbox.

use fednow_gateway::{
    CreateOutcome, HttpSimPort, InMemoryStore, PaymentEvent, PaymentService, PaymentState,
    PaymentStore, SqliteStore, SubmitRequest,
};

fn created(key: &str) -> PaymentEvent {
    PaymentEvent::Created {
        idempotency_key: key.to_string(),
        message_identification: "20260702021040078SQL0000001".to_string(),
        creation_date_time: "2026-07-02T15:30:00Z".to_string(),
        end_to_end_identification: "E2E-SQL-0001".to_string(),
        uetr: None,
        amount_cents: 125_000,
        at_unix: 1_000,
    }
}

#[test]
fn create_is_idempotent_and_appends_are_transition_checked() {
    let store = SqliteStore::in_memory().unwrap();

    assert!(matches!(
        store.create(created("k1")).unwrap(),
        CreateOutcome::Created(_)
    ));
    assert!(matches!(
        store.create(created("k1")).unwrap(),
        CreateOutcome::Existing(_)
    ));

    store
        .append("k1", PaymentEvent::Validated { at_unix: 1_001 })
        .unwrap();
    // Illegal transition is refused and the stream stays intact.
    assert!(store
        .append("k1", PaymentEvent::Published { at_unix: 1_002 })
        .is_err());
    assert_eq!(store.load("k1").unwrap().state, PaymentState::Validated);
    assert_eq!(store.keys(), vec!["k1".to_string()]);
}

#[test]
fn state_survives_closing_and_reopening_the_database() {
    let dir = std::env::temp_dir().join("fednow-gw-sqlite-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("reopen.db");
    let path_str = path.to_str().unwrap();
    let _ = std::fs::remove_file(&path);

    {
        let store = SqliteStore::open(path_str).unwrap();
        store.create(created("durable")).unwrap();
        store
            .append("durable", PaymentEvent::Validated { at_unix: 1_001 })
            .unwrap();
        store
            .submit_to_outbox(
                "durable",
                PaymentEvent::Submitted { at_unix: 1_002 },
                "<xml/>".to_string(),
            )
            .unwrap();
    } // dropped: connection closed

    let store = SqliteStore::open(path_str).unwrap();
    let payment = store.load("durable").unwrap();
    assert_eq!(payment.state, PaymentState::Submitted);
    // The outbox entry survived too: the send intent is durable.
    let entry = store.next_unpublished().unwrap();
    assert_eq!(entry.idempotency_key, "durable");
    store.mark_published(entry.id);
    assert!(store.next_unpublished().is_none());

    let _ = std::fs::remove_file(&path);
}

/// A port that always fails at the transport level.
struct DeadPort;
impl fednow_gateway::FedNowPort for DeadPort {
    fn submit(&self, _: &str) -> Result<fednow_gateway::SubmitOutcome, fednow_gateway::PortError> {
        Err(fednow_gateway::PortError::Transport("wire down".into()))
    }
    fn query(&self, _: &str) -> Result<fednow_gateway::SubmitOutcome, fednow_gateway::PortError> {
        Err(fednow_gateway::PortError::Transport("wire down".into()))
    }
}

fn request(key: &str) -> SubmitRequest {
    SubmitRequest {
        idempotency_key: key.to_string(),
        date_yyyymmdd: "20260702".to_string(),
        sender_reference: "OUTBOX01".to_string(),
        creation_date_time: "2026-07-02T15:30:00Z".to_string(),
        end_to_end_identification: "E2E-OUTBOX01".to_string(),
        uetr: None,
        amount_cents: 125_000,
        debtor_name: "Jane".to_string(),
        debtor_account: "123456789012".to_string(),
        creditor_name: "John".to_string(),
        creditor_account: "987654321000".to_string(),
        creditor_agent_routing_number: "091000019".to_string(),
        category_purpose: "CONS".to_string(),
        settlement_date: "2026-07-02".to_string(),
    }
}

#[test]
fn transport_failure_leaves_payment_submitted_with_outbox_intact() {
    // The whole point of the outbox: the wire is down, but the send intent is
    // durable — the payment sits in SUBMITTED (timeout clock NOT running) and
    // the sweeper retries later. Nothing was half-sent, nothing was lost.
    let svc = PaymentService::new(SqliteStore::in_memory().unwrap(), DeadPort, "021040078");
    let payment = svc.submit(&request("wire-down"), 1_000).unwrap();
    assert_eq!(payment.state, PaymentState::Submitted);

    // Retrying while the wire is still down changes nothing.
    assert_eq!(svc.publish_pending(1_010), 0);
    assert_eq!(
        svc.load("wire-down").unwrap().state,
        PaymentState::Submitted
    );
}

#[test]
fn in_memory_store_honors_the_same_outbox_contract() {
    let svc = PaymentService::new(InMemoryStore::new(), DeadPort, "021040078");
    let payment = svc.submit(&request("mem-down"), 1_000).unwrap();
    assert_eq!(payment.state, PaymentState::Submitted);
}

#[test]
fn full_loop_runs_on_sqlite_against_the_simulator() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap()).unwrap();
            axum::serve(listener, fednow_sim::router(Default::default()))
                .await
                .unwrap();
        });
    });
    let base_url = format!("http://{}", rx.recv().unwrap());

    let svc = PaymentService::new(
        SqliteStore::in_memory().unwrap(),
        HttpSimPort::new(base_url),
        "021040078",
    );
    let payment = svc.submit(&request("sqlite-e2e"), 1_000).unwrap();
    assert_eq!(payment.state, PaymentState::Settled);
}
