//! Resolving a held payment: release sends exactly the parked message, once;
//! cancel and expiry end in `CANCELLED`; only an operator key may do either;
//! every outcome is in the event history and survives a restart; and a
//! gateway without a risk provider behaves exactly as before.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use fednow_gateway::hold::{hold_policy_from_lookup, HoldConfigError, DEFAULT_HOLD_MAX_AGE_SECS};
use fednow_gateway::http::{router, AppState, ReconcileConfig};
use fednow_gateway::risk::gate_from_lookup;
use fednow_gateway::{
    message_sha256, ApiKeys, FedNowPort, HoldAction, HoldPolicy, HttpSimPort, InMemoryStore,
    OnUnavailable, Payment, PaymentEvent, PaymentService, PaymentState, PaymentStore, PortError,
    RiskCheckInput, RiskDecision, RiskError, RiskGate, RiskOutcome, RiskPolicy, RiskProvider,
    RiskSource, ServiceError, SimRiskProvider, SqliteStore, SubmitOutcome, SubmitRequest,
};
use http_body_util::BodyExt;
use tower::ServiceExt;

// ---------------------------------------------------------------- fixtures --

/// A southbound port that records every message that reaches the wire.
#[derive(Clone, Default)]
struct RecordingPort {
    sent: Arc<Mutex<Vec<String>>>,
}

impl RecordingPort {
    fn sent(&self) -> Vec<String> {
        self.sent.lock().unwrap().clone()
    }
}

impl FedNowPort for RecordingPort {
    fn submit(&self, pacs008_xml: &str) -> Result<SubmitOutcome, PortError> {
        self.sent.lock().unwrap().push(pacs008_xml.to_string());
        Ok(SubmitOutcome::Accepted)
    }

    fn query(&self, _pacs028_xml: &str) -> Result<SubmitOutcome, PortError> {
        Ok(SubmitOutcome::Accepted)
    }
}

/// A provider that holds every payment, and counts how often it was asked.
#[derive(Default)]
struct HoldAll {
    asked: AtomicUsize,
}

impl RiskProvider for HoldAll {
    fn name(&self) -> &'static str {
        "hold_all"
    }

    fn check(
        &self,
        _input: &RiskCheckInput,
        _timeout: Duration,
    ) -> Result<RiskDecision, RiskError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        Ok(RiskDecision::Hold {
            reason: "velocity".to_string(),
        })
    }
}

/// Allows everything except amounts ending in 88, which it refuses.
struct AllowOrRefuse;

impl RiskProvider for AllowOrRefuse {
    fn name(&self) -> &'static str {
        "allow_or_refuse"
    }

    fn check(&self, input: &RiskCheckInput, _timeout: Duration) -> Result<RiskDecision, RiskError> {
        Ok(if input.amount_cents % 100 == 88 {
            RiskDecision::Refuse {
                reason: "blocked".to_string(),
            }
        } else {
            RiskDecision::Allow
        })
    }
}

fn gate(provider: Arc<dyn RiskProvider>) -> RiskGate {
    RiskGate::new(
        provider,
        RiskPolicy {
            timeout: Duration::from_millis(500),
            max_in_flight: 8,
            on_unavailable: OnUnavailable::Hold,
        },
    )
}

fn request(key: &str, reference: &str) -> SubmitRequest {
    SubmitRequest {
        idempotency_key: key.to_string(),
        date_yyyymmdd: "20260702".to_string(),
        sender_reference: reference.to_string(),
        creation_date_time: "2026-07-02T15:30:00Z".to_string(),
        end_to_end_identification: format!("E2E-{reference}"),
        uetr: Some("8a562c67-ca16-48ba-b074-65581be6f001".to_string()),
        amount_cents: 125_000,
        debtor_name: "Jane Example Debtor".to_string(),
        debtor_account: "123456789012".to_string(),
        creditor_name: "John Example Creditor".to_string(),
        creditor_account: "987654321000".to_string(),
        creditor_agent_routing_number: "992000008".to_string(),
        category_purpose: "CONS".to_string(),
        settlement_date: "2026-07-02".to_string(),
    }
}

const MAX_AGE: i64 = 600;
const HELD_AT: i64 = 1_000;
const OPERATOR: &str = "operator:ops-a";

type Svc<S> = PaymentService<S, RecordingPort>;

fn service_on<S: PaymentStore>(
    store: S,
    provider: Option<Arc<dyn RiskProvider>>,
) -> (Svc<S>, RecordingPort) {
    let port = RecordingPort::default();
    let svc = PaymentService::new(store, port.clone(), "991000009").with_hold_policy(HoldPolicy {
        max_age_secs: MAX_AGE,
    });
    let svc = match provider {
        Some(p) => svc.with_risk_gate(gate(p)),
        None => svc,
    };
    (svc, port)
}

/// A service whose provider holds everything, with one payment held.
fn held(key: &str) -> (Svc<InMemoryStore>, RecordingPort, Payment) {
    let (svc, port) = service_on(InMemoryStore::new(), Some(Arc::new(HoldAll::default())));
    let p = svc.submit(&request(key, "HOLD0001"), HELD_AT).unwrap();
    assert_eq!(p.state, PaymentState::Held);
    (svc, port, p)
}

fn names(events: &[PaymentEvent]) -> Vec<&'static str> {
    events
        .iter()
        .map(|e| match e {
            PaymentEvent::Created { .. } => "Created",
            PaymentEvent::Validated { .. } => "Validated",
            PaymentEvent::RiskChecked { .. } => "RiskChecked",
            PaymentEvent::HoldParked { .. } => "HoldParked",
            PaymentEvent::HoldReleased { .. } => "HoldReleased",
            PaymentEvent::HoldCancelled { .. } => "HoldCancelled",
            PaymentEvent::Submitted { .. } => "Submitted",
            PaymentEvent::Published { .. } => "Published",
            PaymentEvent::AdviceReceived { .. } => "AdviceReceived",
            PaymentEvent::TimeoutDeclared { .. } => "TimeoutDeclared",
            PaymentEvent::QuerySent { .. } => "QuerySent",
        })
        .collect()
}

// ----------------------------------------------------------------- hold --

#[test]
fn a_hold_parks_the_checked_message_and_records_only_its_digest() {
    let (svc, port, p) = held("park-1");
    assert!(port.sent().is_empty(), "a hold sends nothing");
    assert_eq!(
        names(&p.events),
        ["Created", "Validated", "RiskChecked", "HoldParked"]
    );
    assert_eq!(p.held_at_unix, Some(HELD_AT));
    let parked = svc_parked(&svc, "park-1");
    assert_eq!(p.held_message_sha256, Some(message_sha256(&parked)));
    assert!(parked.contains("2026-07-02T15:30:00Z"));

    // The event history carries the digest, never the message.
    let json = serde_json::to_string(&p.events[3]).unwrap();
    assert!(
        json.starts_with(r#"{"HoldParked":{"message_sha256":""#),
        "{json}"
    );
    assert_no_payment_data(&json);
}

/// Fixture values that must never reach the audit trail, each with a label.
/// A failure names the label only: neither the value nor the audited JSON
/// goes to the test output, which is public in CI.
const PAYMENT_DATA: [(&str, &str); 7] = [
    ("debtor account", "123456789012"),
    ("creditor account", "987654321000"),
    ("debtor name", "Jane"),
    ("creditor name", "John"),
    ("creditor agent routing number", "992000008"),
    ("amount in cents", "125000"),
    ("amount", "1250.00"),
];

fn assert_no_payment_data(audited: &str) {
    let found: Vec<&str> = PAYMENT_DATA
        .iter()
        .filter(|(_, value)| audited.contains(value))
        .map(|(label, _)| *label)
        .collect();
    assert!(found.is_empty(), "the audit trail carries: {found:?}");
}

fn svc_parked<S: PaymentStore>(svc: &Svc<S>, key: &str) -> String {
    svc.store().parked_message(key).expect("a parked message")
}

// -------------------------------------------------------------- release --

#[test]
fn release_sends_exactly_the_parked_message_once() {
    let (svc, port, before) = held("rel-1");
    let parked = svc_parked(&svc, "rel-1");

    // At the last releasable second: the message sent is the one that was
    // checked, byte for byte, not one rebuilt with the release's clock.
    let p = svc
        .release("rel-1", OPERATOR, "reviewed_ok", HELD_AT + MAX_AGE)
        .unwrap();
    assert_eq!(p.state, PaymentState::AckPending);
    assert_eq!(port.sent(), vec![parked.clone()]);
    assert_eq!(
        svc.store().parked_message("rel-1"),
        None,
        "moved, not copied"
    );
    assert_eq!(svc.store().unpublished_count(), 0);

    assert_eq!(
        names(&p.events),
        [
            "Created",
            "Validated",
            "RiskChecked",
            "HoldParked",
            "HoldReleased",
            "Published"
        ]
    );
    match &p.events[4] {
        PaymentEvent::HoldReleased {
            actor,
            reason,
            message_sha256: digest,
            at_unix,
        } => {
            assert_eq!(actor, OPERATOR);
            assert_eq!(reason, "reviewed_ok");
            assert_eq!(Some(digest), before.held_message_sha256.as_ref());
            assert_eq!(digest, &message_sha256(&parked));
            assert_eq!(*at_unix, HELD_AT + MAX_AGE);
        }
        other => panic!("expected HoldReleased, got {other:?}"),
    }
    let r = p.hold_resolution.clone().unwrap();
    assert_eq!(
        (r.action, r.actor.as_str(), r.reason.as_str()),
        (HoldAction::Released, OPERATOR, "reviewed_ok")
    );
    // The risk check's verdict is history, not rewritten.
    assert_eq!(p.risk_outcome, Some(RiskOutcome::Hold));

    // The whole audit trail of a held-then-released payment carries no
    // account, name, amount or counterparty routing number.
    assert_no_payment_data(&serde_json::to_string(&p.events[2..]).unwrap());
}

#[test]
fn a_release_never_asks_the_provider_again() {
    let provider = Arc::new(HoldAll::default());
    let (svc, _port) = service_on(InMemoryStore::new(), Some(provider.clone()));
    svc.submit(&request("rel-ask", "HOLD0002"), HELD_AT)
        .unwrap();
    svc.release("rel-ask", OPERATOR, "reviewed_ok", HELD_AT + 1)
        .unwrap();
    assert_eq!(provider.asked.load(Ordering::SeqCst), 1);

    // Resubmitting the same key still returns the payment as it stands.
    let again = svc
        .submit(&request("rel-ask", "HOLD0002"), HELD_AT + 2)
        .unwrap();
    assert_eq!(again.state, PaymentState::AckPending);
    assert_eq!(provider.asked.load(Ordering::SeqCst), 1);
}

#[test]
fn release_of_a_payment_that_is_not_held_changes_nothing() {
    let (svc, port) = service_on(InMemoryStore::new(), Some(Arc::new(AllowOrRefuse)));
    let sent = svc
        .submit(&request("not-held-allow", "ALLOW001"), HELD_AT)
        .unwrap();
    assert_eq!(sent.state, PaymentState::AckPending);
    let mut refused_req = request("not-held-refuse", "REFUSE01");
    refused_req.amount_cents = 125_088;
    let refused = svc.submit(&refused_req, HELD_AT).unwrap();
    assert_eq!(refused.state, PaymentState::Refused);

    for (key, state) in [
        ("not-held-allow", PaymentState::AckPending),
        ("not-held-refuse", PaymentState::Refused),
    ] {
        let before = svc.load(key).unwrap().events;
        for result in [
            svc.release(key, OPERATOR, "reviewed_ok", HELD_AT + 1),
            svc.cancel(key, OPERATOR, "reviewed_ko", HELD_AT + 1),
        ] {
            match result {
                Err(ServiceError::NotHeld(s)) => assert_eq!(s, state, "{key}"),
                other => panic!("{key}: expected NotHeld, got {other:?}"),
            }
        }
        assert_eq!(svc.load(key).unwrap().events, before, "{key}");
    }
    assert!(matches!(
        svc.release("never-submitted", OPERATOR, "reviewed_ok", HELD_AT),
        Err(ServiceError::UnknownPayment(_))
    ));
    assert_eq!(port.sent().len(), 1, "only the allowed payment was sent");
}

#[test]
fn a_double_release_sends_once() {
    let (svc, port, _) = held("dbl-1");
    svc.release("dbl-1", OPERATOR, "reviewed_ok", HELD_AT + 1)
        .unwrap();
    let after_first = svc.load("dbl-1").unwrap().events;
    match svc.release("dbl-1", "operator:ops-b", "reviewed_ok", HELD_AT + 2) {
        Err(ServiceError::NotHeld(PaymentState::AckPending)) => {}
        other => panic!("expected NotHeld(AckPending), got {other:?}"),
    }
    assert_eq!(svc.load("dbl-1").unwrap().events, after_first);
    assert_eq!(port.sent().len(), 1);
}

#[test]
fn racing_releases_send_once() {
    for round in 0..20 {
        let key = format!("race-{round}");
        let (svc, port, _) = held(&key);
        let svc = Arc::new(svc);
        let barrier = Arc::new(std::sync::Barrier::new(4));
        let handles: Vec<_> = (0..4)
            .map(|i| {
                let (svc, barrier, key) = (Arc::clone(&svc), Arc::clone(&barrier), key.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    svc.release(
                        &key,
                        &format!("operator:ops-{i}"),
                        "reviewed_ok",
                        HELD_AT + 1,
                    )
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let ok = results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(ok, 1, "round {round}: {results:?}");
        for r in results.iter().filter(|r| r.is_err()) {
            assert!(matches!(r, Err(ServiceError::NotHeld(_))), "{r:?}");
        }
        assert_eq!(port.sent().len(), 1, "round {round}");
        let released = svc
            .load(&key)
            .unwrap()
            .events
            .iter()
            .filter(|e| matches!(e, PaymentEvent::HoldReleased { .. }))
            .count();
        assert_eq!(released, 1, "round {round}");
    }
}

#[test]
fn a_reason_must_be_a_code() {
    let (svc, port, before) = held("why-1");
    for bad in [
        "",
        "Looks fine to me",
        "acct_123456789012",
        "ok\n",
        &"x".repeat(65),
    ] {
        assert!(
            matches!(
                svc.release("why-1", OPERATOR, bad, HELD_AT + 1),
                Err(ServiceError::InvalidReason)
            ),
            "{bad:?}"
        );
        assert!(matches!(
            svc.cancel("why-1", OPERATOR, bad, HELD_AT + 1),
            Err(ServiceError::InvalidReason)
        ));
    }
    assert_eq!(svc.load("why-1").unwrap().events, before.events);
    assert!(port.sent().is_empty());

    // The target is judged before the request: an unknown payment is 404 and
    // one that is not held is 409, whatever the reason says.
    assert!(matches!(
        svc.release("no-such-key", OPERATOR, "Not A Code", HELD_AT + 1),
        Err(ServiceError::UnknownPayment(_))
    ));
    svc.cancel("why-1", OPERATOR, "reviewed_ko", HELD_AT + 2)
        .unwrap();
    assert!(matches!(
        svc.release("why-1", OPERATOR, "Not A Code", HELD_AT + 3),
        Err(ServiceError::NotHeld(PaymentState::Cancelled))
    ));
}

#[test]
fn the_state_machine_refuses_a_release_of_other_bytes() {
    let (_, _, p) = held("sm-1");
    let mut replay = Payment::replay(p.events.clone()).unwrap();
    let err = replay
        .apply(PaymentEvent::HoldReleased {
            actor: OPERATOR.to_string(),
            reason: "reviewed_ok".to_string(),
            message_sha256: message_sha256("<Document>something else</Document>"),
            at_unix: HELD_AT + 1,
        })
        .unwrap_err();
    assert_eq!(err.state, PaymentState::Held);
    // And a release is not a way into the outbox for a payment never held.
    let mut plain = Payment::replay(p.events[..2].to_vec()).unwrap();
    assert!(plain
        .apply(PaymentEvent::HoldReleased {
            actor: OPERATOR.to_string(),
            reason: "reviewed_ok".to_string(),
            message_sha256: p.held_message_sha256.clone().unwrap(),
            at_unix: HELD_AT + 1,
        })
        .is_err());
}

// --------------------------------------------------------------- cancel --

#[test]
fn cancel_ends_in_cancelled_and_discards_the_message() {
    let (svc, port, _) = held("can-1");
    let p = svc
        .cancel("can-1", OPERATOR, "confirmed_fraud", HELD_AT + 5)
        .unwrap();
    assert_eq!(p.state, PaymentState::Cancelled);
    assert_eq!(p.state.name(), "CANCELLED");
    assert_ne!(p.state, PaymentState::Refused);
    assert_ne!(p.state, PaymentState::Rejected);
    assert_eq!(svc.store().parked_message("can-1"), None);
    let r = p.hold_resolution.clone().unwrap();
    assert_eq!(
        (r.action, r.actor.as_str(), r.reason.as_str(), r.at_unix),
        (
            HoldAction::Cancelled,
            OPERATOR,
            "confirmed_fraud",
            HELD_AT + 5
        )
    );
    assert_eq!(names(&p.events).last(), Some(&"HoldCancelled"));

    // Terminal: neither a release nor a second cancel moves it.
    assert!(matches!(
        svc.release("can-1", OPERATOR, "reviewed_ok", HELD_AT + 6),
        Err(ServiceError::NotHeld(PaymentState::Cancelled))
    ));
    assert!(matches!(
        svc.cancel("can-1", OPERATOR, "confirmed_fraud", HELD_AT + 6),
        Err(ServiceError::NotHeld(PaymentState::Cancelled))
    ));
    assert!(port.sent().is_empty());
}

// ------------------------------------------------------------ staleness --

#[test]
fn a_hold_is_releasable_up_to_its_maximum_age_and_not_after() {
    // At exactly the maximum age: still releasable.
    let (svc, port, _) = held("age-edge");
    assert!(svc
        .release("age-edge", OPERATOR, "reviewed_ok", HELD_AT + MAX_AGE)
        .is_ok());
    assert_eq!(port.sent().len(), 1);

    // One second later: cancelled by the gateway instead of sent.
    let (svc, port, _) = held("age-late");
    assert!(matches!(
        svc.release("age-late", OPERATOR, "reviewed_ok", HELD_AT + MAX_AGE + 1),
        Err(ServiceError::HoldExpired)
    ));
    let p = svc.load("age-late").unwrap();
    assert_eq!(p.state, PaymentState::Cancelled);
    let r = p.hold_resolution.unwrap();
    assert_eq!(
        (r.action, r.actor.as_str(), r.reason.as_str()),
        (HoldAction::Cancelled, "gateway", "hold_expired")
    );
    assert!(port.sent().is_empty());
    assert_eq!(svc.store().parked_message("age-late"), None);

    // An operator may still cancel a stale hold explicitly.
    let (svc, _, _) = held("age-cancel");
    let p = svc
        .cancel(
            "age-cancel",
            OPERATOR,
            "customer_withdrew",
            HELD_AT + 10 * MAX_AGE,
        )
        .unwrap();
    assert_eq!(p.hold_resolution.unwrap().actor, OPERATOR);
}

#[test]
fn the_sweeper_expires_only_stale_holds() {
    let (svc, port) = service_on(InMemoryStore::new(), Some(Arc::new(HoldAll::default())));
    svc.submit(&request("sweep-old", "SWEEP001"), HELD_AT)
        .unwrap();
    svc.submit(&request("sweep-new", "SWEEP002"), HELD_AT + 300)
        .unwrap();

    assert_eq!(svc.expire_holds(HELD_AT + MAX_AGE), 0);
    assert_eq!(svc.expire_holds(HELD_AT + MAX_AGE + 1), 1);
    assert_eq!(
        svc.load("sweep-old").unwrap().state,
        PaymentState::Cancelled
    );
    assert_eq!(svc.load("sweep-new").unwrap().state, PaymentState::Held);
    assert_eq!(svc.expire_holds(HELD_AT + MAX_AGE + 1), 0, "idempotent");
    assert!(port.sent().is_empty());
}

#[test]
fn the_maximum_age_is_configurable_and_validated() {
    assert_eq!(
        hold_policy_from_lookup(|_| None).unwrap().max_age_secs,
        DEFAULT_HOLD_MAX_AGE_SECS
    );
    let set = |v: &'static str| {
        hold_policy_from_lookup(move |name| {
            (name == "FEDNOW_GW_HOLD_MAX_AGE_SECS").then(|| v.to_string())
        })
    };
    assert_eq!(set(" 90 ").unwrap().max_age_secs, 90);
    for bad in ["0", "-5", "soon", "", "1.5"] {
        assert_eq!(
            set(bad).unwrap_err(),
            HoldConfigError::NotPositive(bad.to_string())
        );
    }
}

// ---------------------------------------------- legacy holds (from #96) --

#[test]
fn a_hold_recorded_before_parking_existed_can_be_cancelled_not_released() {
    // What a #96 gateway recorded: a hold, no parked message.
    let key = "legacy-1";
    let (svc, port) = {
        let store = InMemoryStore::new();
        store
            .create(PaymentEvent::Created {
                idempotency_key: key.to_string(),
                message_identification: "20260702991000009LEGACY01".to_string(),
                creation_date_time: "2026-07-02T15:30:00Z".to_string(),
                end_to_end_identification: "E2E-LEGACY01".to_string(),
                uetr: None,
                amount_cents: 125_000,
                at_unix: HELD_AT,
            })
            .unwrap();
        store
            .append(key, PaymentEvent::Validated { at_unix: HELD_AT })
            .unwrap();
        store
            .append(
                key,
                PaymentEvent::RiskChecked {
                    outcome: RiskOutcome::Hold,
                    reason: Some("sim.hold".to_string()),
                    source: RiskSource::Provider,
                    provider: "sim".to_string(),
                    elapsed_ms: 3,
                    at_unix: HELD_AT,
                },
            )
            .unwrap();
        service_on(store, None)
    };
    assert!(matches!(
        svc.release(key, OPERATOR, "reviewed_ok", HELD_AT + 1),
        Err(ServiceError::NoParkedMessage)
    ));
    assert_eq!(svc.load(key).unwrap().state, PaymentState::Held);
    let p = svc
        .cancel(key, OPERATOR, "resubmitted", HELD_AT + 2)
        .unwrap();
    assert_eq!(p.state, PaymentState::Cancelled);
    assert!(port.sent().is_empty());
}

// ------------------------------------------------ durability (SQLite) --

fn db_path(name: &str) -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("fednow-gw-hold-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hold.db");
    let path_str = path.to_str().unwrap().to_string();
    std::fs::remove_file(&path).ok();
    (dir, path_str)
}

#[test]
fn holds_releases_and_cancels_survive_restarts() {
    let (dir, path) = db_path("restart");

    // 1. Hold two payments, then "crash".
    let parked = {
        let (svc, port) = service_on(
            SqliteStore::open(&path).unwrap(),
            Some(Arc::new(HoldAll::default())),
        );
        svc.submit(&request("dur-rel", "DURABLE1"), HELD_AT)
            .unwrap();
        svc.submit(&request("dur-can", "DURABLE2"), HELD_AT)
            .unwrap();
        assert!(port.sent().is_empty());
        svc_parked(&svc, "dur-rel")
    };

    // 2. After a restart, still HELD with the same parked bytes; resolve them
    //    from a gateway that has no provider at all (release needs none).
    {
        let (svc, port) = service_on(SqliteStore::open(&path).unwrap(), None);
        let p = svc.load("dur-rel").unwrap();
        assert_eq!(p.state, PaymentState::Held);
        assert_eq!(svc_parked(&svc, "dur-rel"), parked);
        svc.release("dur-rel", OPERATOR, "reviewed_ok", HELD_AT + 30)
            .unwrap();
        svc.cancel("dur-can", "operator:ops-b", "confirmed_fraud", HELD_AT + 40)
            .unwrap();
        assert_eq!(port.sent(), vec![parked.clone()]);
    }

    // 3. After another restart: the history is intact, nothing is parked,
    //    and nothing is left to send.
    {
        let (svc, port) = service_on(SqliteStore::open(&path).unwrap(), None);
        let rel = svc.load("dur-rel").unwrap();
        assert_eq!(rel.state, PaymentState::AckPending);
        let r = rel.hold_resolution.unwrap();
        assert_eq!(
            (r.action, r.actor.as_str(), r.reason.as_str(), r.at_unix),
            (HoldAction::Released, OPERATOR, "reviewed_ok", HELD_AT + 30)
        );
        assert_eq!(rel.held_message_sha256, Some(message_sha256(&parked)));

        let can = svc.load("dur-can").unwrap();
        assert_eq!(can.state, PaymentState::Cancelled);
        let c = can.hold_resolution.unwrap();
        assert_eq!(
            (c.action, c.actor.as_str(), c.reason.as_str(), c.at_unix),
            (
                HoldAction::Cancelled,
                "operator:ops-b",
                "confirmed_fraud",
                HELD_AT + 40
            )
        );

        assert_eq!(svc.store().parked_message("dur-rel"), None);
        assert_eq!(svc.store().parked_message("dur-can"), None);
        assert_eq!(svc.publish_pending(HELD_AT + 50), 0);
        assert!(matches!(
            svc.release("dur-rel", OPERATOR, "reviewed_ok", HELD_AT + 60),
            Err(ServiceError::NotHeld(PaymentState::AckPending))
        ));
        assert!(port.sent().is_empty(), "nothing re-sent after restart");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_tampered_parked_message_is_refused_not_sent() {
    let (dir, path) = db_path("tamper");
    {
        let (svc, _) = service_on(
            SqliteStore::open(&path).unwrap(),
            Some(Arc::new(HoldAll::default())),
        );
        svc.submit(&request("tamper-1", "TAMPER01"), HELD_AT)
            .unwrap();
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        let n = conn
            .execute(
                "UPDATE held_messages SET message_xml = message_xml || '<!-- edited -->'",
                [],
            )
            .unwrap();
        assert_eq!(n, 1);
    }
    let (svc, port) = service_on(SqliteStore::open(&path).unwrap(), None);
    assert!(matches!(
        svc.release("tamper-1", OPERATOR, "reviewed_ok", HELD_AT + 1),
        Err(ServiceError::Transition(_))
    ));
    assert_eq!(svc.load("tamper-1").unwrap().state, PaymentState::Held);
    assert_eq!(svc.store().unpublished_count(), 0);
    assert!(port.sent().is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_outbox_holds_one_entry_per_payment() {
    // Defence in depth under the state machine: even a direct second
    // submit_to_outbox for the same payment is refused by the store.
    fn check<S: PaymentStore>(store: S) {
        let (svc, _) = service_on(store, None);
        svc.submit(&request("one-entry", "ONEENTRY"), HELD_AT)
            .unwrap();
        assert!(svc
            .store()
            .submit_to_outbox(
                "one-entry",
                PaymentEvent::Submitted { at_unix: HELD_AT },
                "<again/>".to_string(),
            )
            .is_err());
        assert_eq!(svc.store().unpublished_count(), 0);
    }
    check(SqliteStore::in_memory().unwrap());
    check(InMemoryStore::new());
}

// ------------------------------------- unconfigured == exactly as before --

#[test]
fn without_a_provider_nothing_about_holds_exists() {
    let from_env = gate_from_lookup(|_| None, "http://localhost:8080").unwrap();
    assert!(!from_env.is_enabled());

    let mut histories = Vec::new();
    for gate in [None, Some(from_env)] {
        let port = RecordingPort::default();
        let svc = PaymentService::new(InMemoryStore::new(), port.clone(), "991000009");
        let svc = match gate {
            Some(g) => svc.with_risk_gate(g),
            None => svc,
        };
        let p = svc
            .submit(&request("plain-1", "PLAIN001"), HELD_AT)
            .unwrap();
        assert_eq!(p.state, PaymentState::AckPending);
        assert_eq!(
            names(&p.events),
            ["Created", "Validated", "Submitted", "Published"]
        );
        assert_eq!((p.held_at_unix, p.hold_resolution.clone()), (None, None));

        // The new operations find nothing to act on and record nothing.
        assert!(matches!(
            svc.release("plain-1", OPERATOR, "reviewed_ok", HELD_AT + 1),
            Err(ServiceError::NotHeld(PaymentState::AckPending))
        ));
        assert!(matches!(
            svc.cancel("plain-1", OPERATOR, "reviewed_ok", HELD_AT + 1),
            Err(ServiceError::NotHeld(PaymentState::AckPending))
        ));
        assert_eq!(
            svc.expire_holds(HELD_AT + 10 * DEFAULT_HOLD_MAX_AGE_SECS),
            0
        );
        assert_eq!(svc.load("plain-1").unwrap().events, p.events);
        assert_eq!(svc.store().parked_message("plain-1"), None);
        assert_eq!(port.sent().len(), 1);
        histories.push(p.events);
    }
    assert_eq!(histories[0], histories[1]);
}

// ---------------------------------------------------------------- REST --

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

/// Test-only keys, generated for this file; they authorize nothing anywhere.
const FULL_KEY: &str = "test-only-hold-tests-full-0123456789abcdef";
const READ_KEY: &str = "test-only-hold-tests-read-0123456789abcdef";
const OPERATOR_KEY: &str = "test-only-hold-tests-ops-0123456789abcdef";

fn app(sim: &str, with_provider: bool) -> axum::Router {
    let svc = PaymentService::new(InMemoryStore::new(), HttpSimPort::new(sim), "991000009");
    let svc = if with_provider {
        svc.with_risk_gate(RiskGate::new(
            Arc::new(SimRiskProvider::new(sim)),
            RiskPolicy::default(),
        ))
    } else {
        svc
    };
    router(Arc::new(AppState {
        service: svc,
        reconcile: ReconcileConfig {
            timeout_secs: 20,
            backoff_secs: 0,
        },
        api_keys: ApiKeys::from_lists(Some(FULL_KEY), Some(READ_KEY))
            .unwrap()
            .with_operators(Some(&format!("ops-alice:{OPERATOR_KEY}")))
            .unwrap(),
    }))
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    key: Option<&str>,
    idempotency_key: Option<&str>,
    body: &str,
) -> (StatusCode, serde_json::Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(k) = key {
        req = req.header(header::AUTHORIZATION, format!("Bearer {k}"));
    }
    if let Some(k) = idempotency_key {
        req = req.header("Idempotency-Key", k);
    }
    let response = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| serde_json::json!(String::from_utf8_lossy(&bytes)));
    (status, value)
}

async fn submit(app: &axum::Router, key: &str, amount_cents: u64) -> serde_json::Value {
    let body = format!(
        r#"{{"reference": "HOLDREST", "amount_cents": {amount_cents},
            "debtor_name": "Jane Example Debtor", "debtor_account": "123456789012",
            "creditor_name": "John Example Creditor", "creditor_account": "987654321000",
            "creditor_agent_routing_number": "992000008", "category_purpose": "CONS"}}"#
    );
    let (status, view) = call(app, "POST", "/payments", Some(FULL_KEY), Some(key), &body).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    view
}

const REVIEWED: &str = r#"{"reason": "reviewed_ok"}"#;

#[tokio::test]
async fn rest_release_needs_an_operator_key_and_then_settles() {
    let sim = start_sim();
    let app = app(&sim, true);
    let held = submit(&app, "rest-hold", 125_077).await;
    assert_eq!(held["state"], "HELD");
    assert_eq!(held["events"], 4);
    assert!(held["hold"]["resolution"].is_null(), "{held}");
    let held_at = held["hold"]["held_at_unix"].as_i64().unwrap();
    assert_eq!(
        held["hold"]["releasable_until_unix"].as_i64().unwrap(),
        held_at + DEFAULT_HOLD_MAX_AGE_SECS
    );

    // Without the privilege: nothing happens.
    for (key, want) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(READ_KEY), StatusCode::FORBIDDEN),
        (Some(FULL_KEY), StatusCode::FORBIDDEN),
    ] {
        for route in ["release", "cancel"] {
            let (status, body) = call(
                &app,
                "POST",
                &format!("/payments/rest-hold/{route}"),
                key,
                None,
                REVIEWED,
            )
            .await;
            assert_eq!(status, want, "{route} with {key:?}: {body}");
            assert!(!body.to_string().contains("test-only"), "{body}");
        }
    }
    let (_, still) = call(&app, "GET", "/payments/rest-hold", Some(READ_KEY), None, "").await;
    assert_eq!(still["state"], "HELD");
    assert_eq!(still["events"], 4);

    // An operator key cannot submit either.
    let (status, _) = call(
        &app,
        "POST",
        "/payments",
        Some(OPERATOR_KEY),
        Some("rest-ops-submit"),
        "{}",
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Bad reason: 400, nothing changes.
    let (status, body) = call(
        &app,
        "POST",
        "/payments/rest-hold/release",
        Some(OPERATOR_KEY),
        None,
        r#"{"reason": "Jane said it's fine"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_reason");

    // The operator releases it: the parked message goes out and settles.
    let (status, released) = call(
        &app,
        "POST",
        "/payments/rest-hold/release",
        Some(OPERATOR_KEY),
        None,
        REVIEWED,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{released}");
    assert_eq!(released["state"], "SETTLED");
    assert_eq!(
        released["hold"]["resolution"]["actor"],
        "operator:ops-alice"
    );
    assert_eq!(released["hold"]["resolution"]["action"], "released");
    assert_eq!(released["hold"]["resolution"]["reason"], "reviewed_ok");
    assert!(released["hold"].get("releasable_until_unix").is_none());
    assert_eq!(released["risk"]["outcome"], "hold", "the verdict stays");

    // Again: 409, with the state it is in.
    let (status, body) = call(
        &app,
        "POST",
        "/payments/rest-hold/release",
        Some(OPERATOR_KEY),
        None,
        REVIEWED,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        body,
        serde_json::json!({"error": "not_held", "state": "SETTLED"})
    );

    // Unknown payment: 404.
    let (status, _) = call(
        &app,
        "POST",
        "/payments/nope/release",
        Some(OPERATOR_KEY),
        None,
        REVIEWED,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rest_cancel_ends_cancelled_and_shows_in_the_summary() {
    let sim = start_sim();
    let app = app(&sim, true);
    submit(&app, "rest-cancel", 125_077).await;
    let (status, view) = call(
        &app,
        "POST",
        "/payments/rest-cancel/cancel",
        Some(OPERATOR_KEY),
        None,
        r#"{"reason": "confirmed_fraud"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["state"], "CANCELLED");
    assert_eq!(view["hold"]["resolution"]["action"], "cancelled");
    assert_eq!(view["events"], 5);

    let (_, summary) = call(&app, "GET", "/ops/summary", Some(READ_KEY), None, "").await;
    assert_eq!(summary["by_state"]["CANCELLED"], 1, "{summary}");
    assert_eq!(summary["outbox_pending"], 0);
}

#[tokio::test]
async fn rest_without_a_provider_is_unchanged() {
    let sim = start_sim();
    let app = app(&sim, false);
    // An amount the demo provider would hold: nothing asks it.
    let view = submit(&app, "rest-plain", 125_077).await;
    assert_eq!(view["state"], "SETTLED");
    assert!(view.get("risk").is_none(), "{view}");
    assert!(view.get("hold").is_none(), "{view}");
    let (status, body) = call(
        &app,
        "POST",
        "/payments/rest-plain/release",
        Some(OPERATOR_KEY),
        None,
        REVIEWED,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "not_held");
    let (_, after) = call(
        &app,
        "GET",
        "/payments/rest-plain",
        Some(READ_KEY),
        None,
        "",
    )
    .await;
    assert_eq!(after, view);
}
