//! The pre-send risk check: the gate's policy (allow / hold / refuse, timeout,
//! provider error, concurrency budget), its place on the send path, the audit
//! event it leaves, and — the property that matters most — that a gateway
//! without a risk provider behaves exactly as it did before the check existed.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use fednow_gateway::http::{router, AppState, ReconcileConfig};
use fednow_gateway::risk::{gate_from_lookup, sanitize_reason, RiskConfigError};
use fednow_gateway::{
    ApiKeys, FedNowPort, HttpSimPort, InMemoryStore, OnUnavailable, PaymentEvent, PaymentService,
    PaymentState, PortError, RiskCheckInput, RiskDecision, RiskError, RiskGate, RiskOutcome,
    RiskPolicy, RiskProvider, RiskSource, SimRiskProvider, SubmitOutcome, SubmitRequest,
};
use http_body_util::BodyExt;
use tower::ServiceExt;

// ---------------------------------------------------------------- fixtures --

/// A southbound port that only counts what reaches the wire.
#[derive(Clone, Default)]
struct CountingPort {
    submits: Arc<AtomicUsize>,
}

impl FedNowPort for CountingPort {
    fn submit(&self, _pacs008_xml: &str) -> Result<SubmitOutcome, PortError> {
        self.submits.fetch_add(1, Ordering::SeqCst);
        Ok(SubmitOutcome::Accepted)
    }

    fn query(&self, _pacs028_xml: &str) -> Result<SubmitOutcome, PortError> {
        Ok(SubmitOutcome::Accepted)
    }
}

/// A provider that answers whatever it was built with, and records what it
/// was asked.
struct FixedProvider {
    answer: Mutex<Option<Result<RiskDecision, RiskError>>>,
    seen: Mutex<Vec<RiskCheckInput>>,
}

impl FixedProvider {
    fn new(answer: Result<RiskDecision, RiskError>) -> Arc<Self> {
        Arc::new(Self {
            answer: Mutex::new(Some(answer)),
            seen: Mutex::new(Vec::new()),
        })
    }
}

impl RiskProvider for FixedProvider {
    fn name(&self) -> &'static str {
        "fixed"
    }

    fn check(&self, input: &RiskCheckInput, _timeout: Duration) -> Result<RiskDecision, RiskError> {
        self.seen.lock().unwrap().push(input.clone());
        self.answer
            .lock()
            .unwrap()
            .take()
            .expect("FixedProvider answers once")
    }
}

/// A provider that blocks until released (or for a fixed time).
struct SlowProvider {
    release: Mutex<Option<mpsc::Receiver<()>>>,
    sleep: Duration,
}

impl RiskProvider for SlowProvider {
    fn name(&self) -> &'static str {
        "slow"
    }

    fn check(
        &self,
        _input: &RiskCheckInput,
        _timeout: Duration,
    ) -> Result<RiskDecision, RiskError> {
        let rx = self.release.lock().unwrap().take();
        match rx {
            Some(rx) => {
                let _released = rx.recv();
            }
            None => std::thread::sleep(self.sleep),
        }
        Ok(RiskDecision::Allow)
    }
}

/// A provider that panics.
struct PanickingProvider;

impl RiskProvider for PanickingProvider {
    fn name(&self) -> &'static str {
        "panicking"
    }

    fn check(
        &self,
        _input: &RiskCheckInput,
        _timeout: Duration,
    ) -> Result<RiskDecision, RiskError> {
        panic!("provider bug");
    }
}

fn policy(timeout_ms: u64, on_unavailable: OnUnavailable) -> RiskPolicy {
    RiskPolicy {
        timeout: Duration::from_millis(timeout_ms),
        max_in_flight: 8,
        on_unavailable,
    }
}

fn input() -> RiskCheckInput {
    RiskCheckInput {
        idempotency_key: "risk-1".to_string(),
        message_identification: "20260702991000009RISK0001".to_string(),
        end_to_end_identification: "E2E-RISK-0001".to_string(),
        amount_cents: 125_000,
        creditor_agent_routing_number: "992000008".to_string(),
        creditor_account: "987654321000".to_string(),
        category_purpose: "CONS".to_string(),
    }
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

fn service(gate: Option<RiskGate>) -> (PaymentService<InMemoryStore, CountingPort>, CountingPort) {
    let port = CountingPort::default();
    let svc = PaymentService::new(InMemoryStore::new(), port.clone(), "991000009");
    let svc = match gate {
        Some(g) => svc.with_risk_gate(g),
        None => svc,
    };
    (svc, port)
}

fn risk_events(events: &[PaymentEvent]) -> Vec<&PaymentEvent> {
    events
        .iter()
        .filter(|e| matches!(e, PaymentEvent::RiskChecked { .. }))
        .collect()
}

// -------------------------------------------------------- gate: decisions --

#[test]
fn provider_allow_hold_refuse_pass_through() {
    for (answer, outcome, reason) in [
        (RiskDecision::Allow, RiskOutcome::Allow, None),
        (
            RiskDecision::Hold {
                reason: "velocity.receiver".to_string(),
            },
            RiskOutcome::Hold,
            Some("velocity.receiver"),
        ),
        (
            RiskDecision::Refuse {
                reason: "receiver.flagged".to_string(),
            },
            RiskOutcome::Refuse,
            Some("receiver.flagged"),
        ),
    ] {
        let gate = RiskGate::new(
            FixedProvider::new(Ok(answer)),
            policy(1_000, OnUnavailable::Hold),
        );
        let verdict = gate.check(&input()).unwrap();
        assert_eq!(verdict.outcome, outcome);
        assert_eq!(verdict.reason.as_deref(), reason);
        assert_eq!(verdict.source, RiskSource::Provider);
        assert_eq!(verdict.provider, "fixed");
    }
}

#[test]
fn disabled_gate_runs_nothing() {
    assert!(RiskGate::disabled().check(&input()).is_none());
    assert!(!RiskGate::default().is_enabled());
}

// ------------------------------------------------- gate: failure policies --

#[test]
fn provider_timeout_applies_each_policy() {
    for (policy_choice, outcome) in [
        (OnUnavailable::Hold, RiskOutcome::Hold),
        (OnUnavailable::Refuse, RiskOutcome::Refuse),
        (OnUnavailable::Allow, RiskOutcome::Allow),
    ] {
        let provider = Arc::new(SlowProvider {
            release: Mutex::new(None),
            sleep: Duration::from_millis(500),
        });
        let gate = RiskGate::new(provider, policy(50, policy_choice));
        let started = std::time::Instant::now();
        let verdict = gate.check(&input()).unwrap();
        // The gate stopped waiting at the timeout, not when the provider
        // finished.
        assert!(started.elapsed() < Duration::from_millis(400));
        assert_eq!(verdict.source, RiskSource::Timeout);
        assert_eq!(verdict.outcome, outcome);
        if outcome == RiskOutcome::Allow {
            assert_eq!(verdict.reason, None);
        } else {
            assert_eq!(verdict.reason.as_deref(), Some("risk_check_timeout"));
        }
    }
}

#[test]
fn provider_error_applies_each_policy_and_never_records_the_detail() {
    for (policy_choice, outcome) in [
        (OnUnavailable::Hold, RiskOutcome::Hold),
        (OnUnavailable::Refuse, RiskOutcome::Refuse),
        (OnUnavailable::Allow, RiskOutcome::Allow),
    ] {
        let gate = RiskGate::new(
            FixedProvider::new(Err(RiskError::Unavailable(
                "connect https://provider.invalid/ acct 987654321000".to_string(),
            ))),
            policy(1_000, policy_choice),
        );
        let verdict = gate.check(&input()).unwrap();
        assert_eq!(verdict.source, RiskSource::Error);
        assert_eq!(verdict.outcome, outcome);
        let recorded = format!("{verdict:?}");
        assert!(!recorded.contains("987654321000"), "{recorded}");
        assert!(!recorded.contains("provider.invalid"), "{recorded}");
    }
}

#[test]
fn provider_panic_is_an_error_not_a_crash() {
    let gate = RiskGate::new(
        Arc::new(PanickingProvider),
        policy(1_000, OnUnavailable::Hold),
    );
    let verdict = gate.check(&input()).unwrap();
    assert_eq!(verdict.source, RiskSource::Error);
    assert_eq!(verdict.outcome, RiskOutcome::Hold);
}

#[test]
fn default_policy_fails_closed_to_hold() {
    let default = RiskPolicy::default();
    assert_eq!(default.on_unavailable, OnUnavailable::Hold);
    assert_eq!(default.timeout, Duration::from_millis(1_000));
}

#[test]
fn concurrency_budget_bounds_calls_in_flight() {
    let (release, rx) = mpsc::channel();
    let provider = Arc::new(SlowProvider {
        release: Mutex::new(Some(rx)),
        sleep: Duration::ZERO,
    });
    let gate = RiskGate::new(
        provider,
        RiskPolicy {
            timeout: Duration::from_millis(50),
            max_in_flight: 1,
            on_unavailable: OnUnavailable::Refuse,
        },
    );

    // The first call hangs: the gate gives up at the timeout, but the call is
    // still in flight and still holds the only slot.
    assert_eq!(gate.check(&input()).unwrap().source, RiskSource::Timeout);
    // So the second is not attempted at all.
    let second = gate.check(&input()).unwrap();
    assert_eq!(second.source, RiskSource::BudgetExhausted);
    assert_eq!(second.outcome, RiskOutcome::Refuse);
    assert_eq!(
        second.reason.as_deref(),
        Some("risk_check_budget_exhausted")
    );

    // Release the hung call; its slot comes back.
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let verdict = gate.check(&input()).unwrap();
        if verdict.source == RiskSource::Provider {
            assert_eq!(verdict.outcome, RiskOutcome::Allow);
            break;
        }
        assert!(std::time::Instant::now() < deadline, "slot never released");
        std::thread::sleep(Duration::from_millis(10));
    }
}

// ------------------------------------------------------- reason hygiene --

#[test]
fn reasons_are_sanitised_before_they_can_be_recorded() {
    for ok in ["sim.hold", "velocity.receiver", "rule-17", "a", "mule_2024"] {
        assert_eq!(sanitize_reason(ok), ok);
    }
    for bad in [
        "",
        "Hold",              // uppercase
        "acct 987654321000", // space, and a long digit run
        "acct987654321000",  // long digit run alone
        "routing.99200",     // five digits
        "John Example",      // a name
        "{\"score\":0.93}",  // a payload fragment
        "9hold",             // must start with a letter
        &"x".repeat(65),     // too long
        "r\u{e9}sum\u{e9}",  // non-ASCII
    ] {
        assert_eq!(sanitize_reason(bad), "unspecified", "{bad:?}");
    }

    // End to end: a provider that stuffs an account number into its reason
    // gets `unspecified` in the verdict.
    let gate = RiskGate::new(
        FixedProvider::new(Ok(RiskDecision::Refuse {
            reason: "creditor 987654321000 flagged".to_string(),
        })),
        policy(1_000, OnUnavailable::Hold),
    );
    assert_eq!(
        gate.check(&input()).unwrap().reason.as_deref(),
        Some("unspecified")
    );
}

// -------------------------------------------------- the send path itself --

#[test]
fn allow_sends_and_records_the_check() {
    let provider = FixedProvider::new(Ok(RiskDecision::Allow));
    let (svc, port) = service(Some(RiskGate::new(
        provider.clone(),
        policy(1_000, OnUnavailable::Hold),
    )));
    let p = svc.submit(&request("allow-1", "RISK0001"), 1_000).unwrap();

    assert_eq!(p.state, PaymentState::AckPending);
    assert_eq!(port.submits.load(Ordering::SeqCst), 1);
    assert_eq!(p.risk_outcome, Some(RiskOutcome::Allow));
    // Order on the send path: validate → check → outbox → wire.
    let order: Vec<&str> = p
        .events
        .iter()
        .map(|e| match e {
            PaymentEvent::Created { .. } => "created",
            PaymentEvent::Validated { .. } => "validated",
            PaymentEvent::RiskChecked { .. } => "risk_checked",
            PaymentEvent::Submitted { .. } => "submitted",
            PaymentEvent::Published { .. } => "published",
            _ => "other",
        })
        .collect();
    assert_eq!(
        order,
        [
            "created",
            "validated",
            "risk_checked",
            "submitted",
            "published"
        ]
    );
    // The provider saw the payment it was asked about.
    let seen = provider.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].amount_cents, 125_000);
    assert_eq!(seen[0].creditor_agent_routing_number, "992000008");
}

#[test]
fn hold_and_refuse_never_reach_the_outbox_or_the_wire() {
    for (answer, state) in [
        (
            RiskDecision::Hold {
                reason: "review".to_string(),
            },
            PaymentState::Held,
        ),
        (
            RiskDecision::Refuse {
                reason: "blocked".to_string(),
            },
            PaymentState::Refused,
        ),
    ] {
        let provider = FixedProvider::new(Ok(answer));
        let (svc, port) = service(Some(RiskGate::new(
            provider.clone(),
            policy(1_000, OnUnavailable::Allow),
        )));
        let p = svc.submit(&request("stop-1", "RISK0002"), 1_000).unwrap();
        assert_eq!(p.state, state);
        assert_eq!(port.submits.load(Ordering::SeqCst), 0);
        assert_eq!(svc.summary(1_000).outbox_pending, 0);
        assert!(!p
            .events
            .iter()
            .any(|e| matches!(e, PaymentEvent::Submitted { .. })));

        // Idempotent: the same key returns the stopped payment, without
        // asking the provider again or sending.
        let again = svc.submit(&request("stop-1", "RISK0002"), 1_001).unwrap();
        assert_eq!(again.state, state);
        assert_eq!(provider.seen.lock().unwrap().len(), 1);
        assert_eq!(again.events.len(), p.events.len());
        assert_eq!(port.submits.load(Ordering::SeqCst), 0);

        // Visible to operators by state.
        assert_eq!(svc.summary(1_002).by_state.get(state.name()), Some(&1));
    }
}

#[test]
fn timeout_and_error_on_the_send_path_follow_the_default_policy() {
    // Timeout: the default policy holds.
    let (svc, port) = service(Some(RiskGate::new(
        Arc::new(SlowProvider {
            release: Mutex::new(None),
            sleep: Duration::from_millis(300),
        }),
        RiskPolicy {
            timeout: Duration::from_millis(30),
            ..RiskPolicy::default()
        },
    )));
    let p = svc.submit(&request("to-1", "RISK0003"), 1_000).unwrap();
    assert_eq!(p.state, PaymentState::Held);
    assert_eq!(p.risk_source, Some(RiskSource::Timeout));
    assert_eq!(p.risk_reason.as_deref(), Some("risk_check_timeout"));
    assert_eq!(port.submits.load(Ordering::SeqCst), 0);

    // Error: the default policy holds.
    let (svc, port) = service(Some(RiskGate::new(
        FixedProvider::new(Err(RiskError::Unavailable("down".to_string()))),
        RiskPolicy::default(),
    )));
    let p = svc.submit(&request("err-1", "RISK0004"), 1_000).unwrap();
    assert_eq!(p.state, PaymentState::Held);
    assert_eq!(p.risk_source, Some(RiskSource::Error));
    assert_eq!(port.submits.load(Ordering::SeqCst), 0);

    // Fail-open is an explicit choice, and it is still recorded.
    let (svc, port) = service(Some(RiskGate::new(
        FixedProvider::new(Err(RiskError::Unavailable("down".to_string()))),
        policy(1_000, OnUnavailable::Allow),
    )));
    let p = svc.submit(&request("open-1", "RISK0005"), 1_000).unwrap();
    assert_eq!(p.state, PaymentState::AckPending);
    assert_eq!(port.submits.load(Ordering::SeqCst), 1);
    assert_eq!(p.risk_outcome, Some(RiskOutcome::Allow));
    assert_eq!(p.risk_source, Some(RiskSource::Error));
}

#[test]
fn invalid_payments_cost_no_risk_check() {
    let provider = FixedProvider::new(Ok(RiskDecision::Allow));
    let (svc, _port) = service(Some(RiskGate::new(
        provider.clone(),
        policy(1_000, OnUnavailable::Hold),
    )));
    let mut req = request("bad-1", "RISK0006");
    req.category_purpose = "WRONG".to_string();
    assert!(svc.submit(&req, 1_000).is_err());
    assert!(provider.seen.lock().unwrap().is_empty());
}

#[test]
fn audit_event_carries_no_payment_data() {
    let (svc, _port) = service(Some(RiskGate::new(
        FixedProvider::new(Ok(RiskDecision::Hold {
            reason: "review".to_string(),
        })),
        policy(1_000, OnUnavailable::Hold),
    )));
    let p = svc.submit(&request("audit-1", "RISK0007"), 1_000).unwrap();
    let events = risk_events(&p.events);
    assert_eq!(events.len(), 1);
    let json = serde_json::to_string(events[0]).unwrap();
    for sensitive in [
        "987654321000",
        "123456789012",
        "John Example",
        "Jane Example",
        "992000008",
        "125000",
    ] {
        assert!(!json.contains(sensitive), "{sensitive} leaked into {json}");
    }
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let fields = value["RiskChecked"].as_object().unwrap();
    let mut keys: Vec<&str> = fields.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "at_unix",
            "elapsed_ms",
            "outcome",
            "provider",
            "reason",
            "source"
        ]
    );
}

// ------------------------------------- unconfigured == exactly as before --

#[test]
fn unconfigured_gateway_behaves_exactly_as_before() {
    // No gate at all (the pre-existing constructor), the explicit disabled
    // gate, and the gate the binary builds from an empty environment.
    let from_env = gate_from_lookup(|_| None, "http://localhost:8080").unwrap();
    assert!(!from_env.is_enabled());
    let from_none = gate_from_lookup(
        |name| (name == "FEDNOW_GW_RISK_PROVIDER").then(|| "none".to_string()),
        "http://localhost:8080",
    )
    .unwrap();
    assert!(!from_none.is_enabled());

    let mut histories = Vec::new();
    for gate in [None, Some(RiskGate::disabled()), Some(from_env)] {
        let (svc, port) = service(gate);
        let p = svc.submit(&request("same-1", "RISK0008"), 1_000).unwrap();
        assert_eq!(p.state, PaymentState::AckPending);
        assert_eq!(port.submits.load(Ordering::SeqCst), 1);
        assert!(risk_events(&p.events).is_empty());
        assert_eq!(p.risk_outcome, None);
        histories.push(p.events);
    }
    assert_eq!(histories[0], histories[1]);
    assert_eq!(histories[0], histories[2]);
    assert_eq!(
        histories[0].len(),
        4,
        "Created, Validated, Submitted, Published"
    );

    // The storage format of the pre-existing events is untouched.
    assert_eq!(
        serde_json::to_string(&PaymentEvent::Validated { at_unix: 7 }).unwrap(),
        r#"{"Validated":{"at_unix":7}}"#
    );
}

// ------------------------------------------------------------ config --

fn lookup(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |name| {
        pairs
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.to_string())
    }
}

#[test]
fn config_parses_and_refuses_what_it_does_not_understand() {
    let gate = gate_from_lookup(
        lookup(&[
            ("FEDNOW_GW_RISK_PROVIDER", "sim"),
            ("FEDNOW_GW_RISK_TIMEOUT_MS", "250"),
            ("FEDNOW_GW_RISK_MAX_IN_FLIGHT", "4"),
            ("FEDNOW_GW_RISK_ON_UNAVAILABLE", "refuse"),
        ]),
        "http://localhost:8080",
    )
    .unwrap();
    assert!(gate.is_enabled());
    assert_eq!(
        gate.policy(),
        RiskPolicy {
            timeout: Duration::from_millis(250),
            max_in_flight: 4,
            on_unavailable: OnUnavailable::Refuse,
        }
    );

    // Defaults once a provider is named.
    let gate = gate_from_lookup(lookup(&[("FEDNOW_GW_RISK_PROVIDER", "sim")]), "x").unwrap();
    assert_eq!(gate.policy(), RiskPolicy::default());

    assert_eq!(
        gate_from_lookup(lookup(&[("FEDNOW_GW_RISK_PROVIDER", "acme")]), "x").unwrap_err(),
        RiskConfigError::UnknownProvider("acme".to_string())
    );
    assert_eq!(
        gate_from_lookup(
            lookup(&[
                ("FEDNOW_GW_RISK_PROVIDER", "sim"),
                ("FEDNOW_GW_RISK_ON_UNAVAILABLE", "open")
            ]),
            "x"
        )
        .unwrap_err(),
        RiskConfigError::UnknownPolicy("open".to_string())
    );
    for (var, value) in [
        ("FEDNOW_GW_RISK_TIMEOUT_MS", "0"),
        ("FEDNOW_GW_RISK_TIMEOUT_MS", "soon"),
        ("FEDNOW_GW_RISK_MAX_IN_FLIGHT", "0"),
        ("FEDNOW_GW_RISK_MAX_IN_FLIGHT", "-1"),
    ] {
        let err = gate_from_lookup(
            |name| match name {
                "FEDNOW_GW_RISK_PROVIDER" => Some("sim".to_string()),
                n if n == var => Some(value.to_string()),
                _ => None,
            },
            "x",
        )
        .unwrap_err();
        assert_eq!(err, RiskConfigError::NotPositive(var, value.to_string()));
    }
}

// ------------------------------------ simulator-backed, through REST --

fn start_sim() -> String {
    let (tx, rx) = mpsc::channel();
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

/// Test-only key, generated for this file; it authorizes nothing anywhere.
const FULL_KEY: &str = "test-only-risk-tests-key-0123456789abcdef";

fn app(sim_url: &str, gate: Option<RiskGate>) -> axum::Router {
    let svc = PaymentService::new(InMemoryStore::new(), HttpSimPort::new(sim_url), "991000009");
    let svc = match gate {
        Some(g) => svc.with_risk_gate(g),
        None => svc,
    };
    router(Arc::new(AppState {
        service: svc,
        reconcile: ReconcileConfig {
            timeout_secs: 20,
            backoff_secs: 0,
        },
        api_keys: ApiKeys::from_lists(Some(FULL_KEY), None).unwrap(),
    }))
}

async fn post(app: &axum::Router, key: &str, amount_cents: u64) -> serde_json::Value {
    let body = format!(
        r#"{{"reference": "RISKSIM1", "amount_cents": {amount_cents},
            "debtor_name": "Jane Example Debtor", "debtor_account": "123456789012",
            "creditor_name": "John Example Creditor", "creditor_account": "987654321000",
            "creditor_agent_routing_number": "992000008", "category_purpose": "CONS"}}"#
    );
    let response = app
        .clone()
        .oneshot(
            Request::post("/payments")
                .header("content-type", "application/json")
                .header("Idempotency-Key", key)
                .header(header::AUTHORIZATION, format!("Bearer {FULL_KEY}"))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn sim_provider_drives_every_outcome_through_rest() {
    let sim = start_sim();
    let gate = RiskGate::new(
        Arc::new(SimRiskProvider::new(sim.clone())),
        RiskPolicy {
            timeout: Duration::from_millis(300),
            ..RiskPolicy::default()
        },
    );
    let app = app(&sim, Some(gate));

    let settled = post(&app, "sim-allow", 125_000).await;
    assert_eq!(settled["state"], "SETTLED");
    assert_eq!(
        settled["risk"],
        serde_json::json!({"outcome": "allow", "reason": null, "source": "provider"})
    );

    let held = post(&app, "sim-hold", 125_077).await;
    assert_eq!(held["state"], "HELD");
    assert_eq!(
        held["risk"],
        serde_json::json!({"outcome": "hold", "reason": "sim.hold", "source": "provider"})
    );

    let refused = post(&app, "sim-refuse", 125_088).await;
    assert_eq!(refused["state"], "REFUSED");
    assert_eq!(refused["risk"]["reason"], "sim.refuse");

    let down = post(&app, "sim-down", 125_098).await;
    assert_eq!(down["state"], "HELD");
    assert_eq!(
        down["risk"],
        serde_json::json!({"outcome": "hold", "reason": "risk_check_error", "source": "error"})
    );

    let slow = post(&app, "sim-slow", 125_099).await;
    assert_eq!(slow["state"], "HELD");
    assert_eq!(slow["risk"]["source"], "timeout");
}

#[tokio::test]
async fn unconfigured_rest_response_has_no_risk_field() {
    let sim = start_sim();
    let app = app(&sim, None);
    // Even on an amount the demo endpoint would hold: nothing asks it.
    let view = post(&app, "plain-1", 125_077).await;
    assert_eq!(view["state"], "SETTLED");
    assert!(view.get("risk").is_none(), "{view}");
}
