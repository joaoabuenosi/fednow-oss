//! End-to-end: the gateway drives real payments through a live fednow-sim —
//! the full loop this project exists for, entirely in-process.

use fednow_gateway::{
    HttpSimPort, InMemoryStore, PaymentService, PaymentState, ServiceError, SubmitRequest,
};

/// Spin the simulator on an ephemeral port; returns its base URL.
fn start_sim() -> String {
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
    format!("http://{}", rx.recv().unwrap())
}

fn service(base_url: &str) -> PaymentService<InMemoryStore, HttpSimPort> {
    PaymentService::new(
        InMemoryStore::new(),
        HttpSimPort::new(base_url),
        "021040078",
    )
}

fn request(key: &str, reference: &str, amount_cents: u64) -> SubmitRequest {
    SubmitRequest {
        idempotency_key: key.to_string(),
        date_yyyymmdd: "20260702".to_string(),
        sender_reference: reference.to_string(),
        creation_date_time: "2026-07-02T15:30:00Z".to_string(),
        end_to_end_identification: format!("E2E-{reference}"),
        uetr: Some("8a562c67-ca16-48ba-b074-65581be6f001".to_string()),
        amount_cents,
        debtor_name: "Jane Example Debtor".to_string(),
        debtor_account: "123456789012".to_string(),
        creditor_name: "John Example Creditor".to_string(),
        creditor_account: "987654321000".to_string(),
        creditor_agent_routing_number: "091000019".to_string(),
        category_purpose: "CONS".to_string(),
        settlement_date: "2026-07-02".to_string(),
    }
}

#[test]
fn happy_path_settles_through_the_simulator() {
    let sim = start_sim();
    let svc = service(&sim);

    let payment = svc
        .submit(&request("e2e-1", "E2E0001", 125_000), 1_000)
        .unwrap();
    assert_eq!(payment.state, PaymentState::Settled);

    // Resubmitting the same idempotency key touches nothing and returns the
    // settled payment.
    let again = svc
        .submit(&request("e2e-1", "E2E0001", 125_000), 2_000)
        .unwrap();
    assert_eq!(again.state, PaymentState::Settled);
    assert_eq!(again.events.len(), payment.events.len());
}

#[test]
fn participant_rejection_lands_in_rejected_with_reason() {
    let sim = start_sim();
    let svc = service(&sim);

    let payment = svc
        .submit(&request("e2e-2", "E2E0002", 125_011), 1_000)
        .unwrap();
    assert_eq!(payment.state, PaymentState::Rejected);
    assert_eq!(payment.rejection_reason.as_deref(), Some("AC04"));
}

#[test]
fn profile_invalid_request_never_reaches_the_wire() {
    let sim = start_sim();
    let svc = service(&sim);

    let mut bad = request("e2e-3", "E2E0003", 125_000);
    bad.category_purpose = "WRONG".to_string(); // not CONS/BIZZ

    let err = svc.submit(&bad, 1_000).unwrap_err();
    match err {
        ServiceError::Validation(codes) => {
            assert!(codes.contains(&"fednow.ctgypurp.known"), "{codes:?}")
        }
        other => panic!("expected Validation, got {other:?}"),
    }
    // The payment exists (idempotency key is burned) but never advanced.
    assert_eq!(svc.load("e2e-3").unwrap().state, PaymentState::Created);
}

#[test]
fn timeout_arc_resolves_to_settled_via_pacs028() {
    let sim = start_sim();
    let svc = service(&sim);

    // Amount ending .33: the simulator accepts and goes silent.
    let payment = svc
        .submit(&request("e2e-4", "E2E0004", 125_033), 1_000)
        .unwrap();
    assert_eq!(payment.state, PaymentState::AckPending);

    // Inside the presumed timeout: nothing happens.
    let p = svc.reconcile("e2e-4", "20260702", 1_010, 30, 60).unwrap();
    assert_eq!(p.state, PaymentState::AckPending);

    // Past it: the timeout is declared…
    let p = svc.reconcile("e2e-4", "20260702", 1_031, 30, 60).unwrap();
    assert_eq!(p.state, PaymentState::TimeoutUnresolved);

    // …and the next pass queries the service, which reveals the truth:
    // the payment settled all along. No resend ever happened.
    let p = svc.reconcile("e2e-4", "20260702", 1_032, 30, 60).unwrap();
    assert_eq!(p.state, PaymentState::Settled);
    assert_eq!(p.queries_sent, 1);
}
