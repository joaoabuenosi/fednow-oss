//! End-to-end over the MQ-mode transport: fire-and-forget sends, advices
//! pumped off the receive queue — the asynchrony of the real connection.

use fednow_gateway::{InMemoryStore, MqSimPort, PaymentService, PaymentState, SubmitRequest};

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

fn service(base_url: &str) -> PaymentService<InMemoryStore, MqSimPort> {
    PaymentService::new(
        InMemoryStore::new(),
        MqSimPort::new(base_url, "021040078"),
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

/// Pump until at least one advice lands or the deadline passes.
fn pump_until_applied(
    svc: &PaymentService<InMemoryStore, MqSimPort>,
    now_unix: i64,
    max_millis: u64,
) -> usize {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(max_millis);
    loop {
        let applied = svc.pump_advices(now_unix);
        if applied > 0 || std::time::Instant::now() >= deadline {
            return applied;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn happy_path_is_asynchronous_then_settles_via_the_pump() {
    let sim = start_sim();
    let svc = service(&sim);

    // Over MQ, submit() cannot settle synchronously: the send is
    // fire-and-forget, so the payment parks in ACK_PENDING.
    let payment = svc
        .submit(&request("mq-1", "MQE2E0001", 125_000), 1_000)
        .unwrap();
    assert_eq!(payment.state, PaymentState::AckPending);

    // The advice is already waiting on the receive queue; one pump applies it.
    assert_eq!(pump_until_applied(&svc, 1_001, 2_000), 1);
    assert_eq!(svc.load("mq-1").unwrap().state, PaymentState::Settled);
}

#[test]
fn rejection_arrives_through_the_queue_with_its_reason() {
    let sim = start_sim();
    let svc = service(&sim);

    let payment = svc
        .submit(&request("mq-2", "MQE2E0002", 125_011), 1_000)
        .unwrap();
    assert_eq!(payment.state, PaymentState::AckPending);

    assert_eq!(pump_until_applied(&svc, 1_001, 2_000), 1);
    let payment = svc.load("mq-2").unwrap();
    assert_eq!(payment.state, PaymentState::Rejected);
    assert_eq!(payment.rejection_reason.as_deref(), Some("AC04"));
}

#[test]
fn timeout_arc_resolves_via_pacs028_and_the_queue() {
    let sim = start_sim();
    let svc = service(&sim);

    // .33: the simulator accepts and goes silent — nothing to pump.
    let payment = svc
        .submit(&request("mq-3", "MQE2E0003", 125_033), 1_000)
        .unwrap();
    assert_eq!(payment.state, PaymentState::AckPending);
    assert_eq!(svc.pump_advices(1_001), 0);

    // Declare the timeout, then query; the pacs.028 answer arrives on the
    // queue, not in the query response.
    let p = svc.reconcile("mq-3", "20260702", 1_031, 30, 60).unwrap();
    assert_eq!(p.state, PaymentState::TimeoutUnresolved);
    let p = svc.reconcile("mq-3", "20260702", 1_032, 30, 60).unwrap();
    assert_eq!(p.state, PaymentState::TimeoutUnresolved, "answer is async");
    assert_eq!(p.queries_sent, 1);

    assert_eq!(pump_until_applied(&svc, 1_033, 2_000), 1);
    assert_eq!(svc.load("mq-3").unwrap().state, PaymentState::Settled);
}

#[test]
fn acwp_follow_up_is_pushed_and_recorded_post_settlement() {
    let sim = start_sim();
    let svc = service(&sim);

    // .66: ACWP now, the receiving participant's ACCC pushed ~500ms later.
    svc.submit(&request("mq-4", "MQE2E0004", 125_066), 1_000)
        .unwrap();

    assert!(pump_until_applied(&svc, 1_001, 2_000) >= 1);
    let payment = svc.load("mq-4").unwrap();
    assert_eq!(payment.state, PaymentState::Settled);

    // The pushed follow-up needs no pacs.028 — it just arrives.
    pump_until_applied(&svc, 1_002, 3_000);
    let payment = svc.load("mq-4").unwrap();
    assert_eq!(payment.state, PaymentState::Settled);
    assert_eq!(
        payment.last_advice,
        Some(fednow_gateway::AdviceStatus::Accc),
        "the post-settlement confirmation is recorded"
    );
}
