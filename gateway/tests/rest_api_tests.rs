//! REST API tests: the northbound contract against a live in-process sim.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Method};
use axum::http::{Request, StatusCode};
use fednow_gateway::http::{route_table, router, AppState, ReconcileConfig};
use fednow_gateway::{Access, ApiKeys, HttpSimPort, InMemoryStore, PaymentService};
use http_body_util::BodyExt;
use tower::ServiceExt;

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
const FULL_KEY: &str = "test-only-full-access-key-0123456789abcdef";
const READ_KEY: &str = "test-only-read-only-key-0123456789abcdef";
const OPERATOR_KEY: &str = "test-only-operator-key-0123456789abcdef";

fn state(sim_url: &str, timeout_secs: i64) -> Arc<AppState<InMemoryStore, HttpSimPort>> {
    Arc::new(AppState {
        service: PaymentService::new(InMemoryStore::new(), HttpSimPort::new(sim_url), "991000009"),
        reconcile: ReconcileConfig {
            timeout_secs,
            backoff_secs: 0,
        },
        api_keys: ApiKeys::from_lists(Some(FULL_KEY), Some(READ_KEY))
            .unwrap()
            .with_operators(Some(&format!("ops-test:{OPERATOR_KEY}")))
            .unwrap(),
    })
}

fn app(sim_url: &str, timeout_secs: i64) -> axum::Router {
    router(state(sim_url, timeout_secs))
}

fn bearer(key: &str) -> String {
    format!("Bearer {key}")
}

fn body_json(reference: &str, amount_cents: u64) -> String {
    format!(
        r#"{{
            "reference": "{reference}",
            "amount_cents": {amount_cents},
            "debtor_name": "Jane Example Debtor",
            "debtor_account": "123456789012",
            "creditor_name": "John Example Creditor",
            "creditor_account": "987654321000",
            "creditor_agent_routing_number": "992000008",
            "category_purpose": "CONS",
            "uetr": "8a562c67-ca16-48ba-b074-65581be6f001"
        }}"#
    )
}

async fn call(app: &axum::Router, req: Request<Body>) -> (StatusCode, serde_json::Value) {
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| serde_json::json!(String::from_utf8_lossy(&bytes)));
    (status, value)
}

fn post_payment(key: &str, body: String) -> Request<Body> {
    Request::post("/payments")
        .header("content-type", "application/json")
        .header("Idempotency-Key", key)
        .header(header::AUTHORIZATION, bearer(FULL_KEY))
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn submit_settles_and_replays_idempotently() {
    let sim = start_sim();
    let app = app(&sim, 20);

    let (status, view) = call(&app, post_payment("r1", body_json("REST0001", 125_000))).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["state"], "SETTLED");
    let events = view["events"].as_u64().unwrap();

    // Same key again: same payment, no new events.
    let (status, again) = call(&app, post_payment("r1", body_json("REST0001", 125_000))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["events"].as_u64().unwrap(), events);

    // And it is queryable.
    let (status, got) = call(
        &app,
        Request::get("/payments/r1")
            .header(header::AUTHORIZATION, bearer(FULL_KEY))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(got["state"], "SETTLED");
}

#[tokio::test]
async fn missing_idempotency_key_is_a_400() {
    let sim = start_sim();
    let app = app(&sim, 20);
    let req = Request::post("/payments")
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, bearer(FULL_KEY))
        .body(Body::from(body_json("REST0002", 125_000)))
        .unwrap();
    let (status, _) = call(&app, req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn profile_violation_is_a_422_with_rule_codes() {
    let sim = start_sim();
    let app = app(&sim, 20);
    let body = body_json("REST0003", 125_000).replace("CONS", "NOPE");
    let (status, view) = call(&app, post_payment("r3", body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(view["codes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c == "fednow.ctgypurp.known"));
}

#[tokio::test]
async fn timeout_then_reconcile_endpoint_resolves_to_settled() {
    let sim = start_sim();
    // timeout_secs = 0 so the first reconcile pass declares immediately.
    let app = app(&sim, 0);

    let (status, view) = call(&app, post_payment("r4", body_json("REST0004", 125_033))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["state"], "ACK_PENDING");

    let reconcile = || {
        Request::post("/payments/r4/reconcile")
            .header(header::AUTHORIZATION, bearer(FULL_KEY))
            .body(Body::empty())
            .unwrap()
    };

    let (status, view) = call(&app, reconcile()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["state"], "TIMEOUT_UNRESOLVED");

    let (status, view) = call(&app, reconcile()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["state"], "SETTLED", "the pacs.028 revealed settlement");
    assert_eq!(view["queries_sent"], 1);
}

#[tokio::test]
async fn ops_summary_reports_states_and_outbox() {
    let sim = start_sim();
    let app = app(&sim, 0); // timeout 0: reconcile declares immediately

    // One settled, one parked in the timeout arc.
    let (status, _) = call(&app, post_payment("o1", body_json("REST0005", 125_000))).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(&app, post_payment("o2", body_json("REST0006", 125_033))).await;
    assert_eq!(status, StatusCode::OK);
    let (status, view) = call(
        &app,
        Request::post("/payments/o2/reconcile")
            .header(header::AUTHORIZATION, bearer(FULL_KEY))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["state"], "TIMEOUT_UNRESOLVED");

    let (status, view) = call(
        &app,
        Request::get("/ops/summary")
            .header(header::AUTHORIZATION, bearer(READ_KEY))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["payments_total"], 2);
    assert_eq!(view["by_state"]["SETTLED"], 1);
    assert_eq!(view["by_state"]["TIMEOUT_UNRESOLVED"], 1);
    assert_eq!(view["outbox_pending"], 0);
    assert!(
        view["oldest_unresolved_age_secs"].is_i64(),
        "unresolved age must be reported: {view}"
    );
}

// ---------------------------------------------------------------- auth --

/// The access level every route is expected to have. Adding, removing or
/// re-levelling a route fails `route_table_is_the_reviewed_one` until this
/// table is updated too — which puts the access decision in front of a
/// reviewer instead of letting a route inherit whatever it got.
const EXPECTED_ROUTES: &[(Method, &str, Access)] = &[
    (Method::GET, "/healthz", Access::Public),
    (Method::POST, "/payments", Access::Write),
    (Method::GET, "/payments/{key}", Access::Read),
    (Method::POST, "/payments/{key}/reconcile", Access::Write),
    (Method::POST, "/payments/{key}/release", Access::Operate),
    (Method::POST, "/payments/{key}/cancel", Access::Operate),
    (Method::GET, "/ops/summary", Access::Read),
];

/// A concrete request for a route template: `{key}` becomes a sample key.
fn request_for(method: &Method, template: &str, authorization: Option<&str>) -> Request<Body> {
    let mut req = Request::builder()
        .method(method.clone())
        .uri(template.replace("{key}", "some-key"));
    if let Some(value) = authorization {
        req = req.header(header::AUTHORIZATION, value);
    }
    req.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn route_table_is_the_reviewed_one() {
    // No request is sent; the sim URL is never dialled.
    let table = route_table(state("http://127.0.0.1:9", 20));
    let actual: Vec<_> = table
        .iter()
        .map(|r| (r.method.clone(), r.path, r.access))
        .collect();
    assert_eq!(
        actual,
        EXPECTED_ROUTES.to_vec(),
        "the router's routes changed: state each new route's access level in EXPECTED_ROUTES"
    );
    let public: Vec<_> = table
        .iter()
        .filter(|r| r.access == Access::Public)
        .map(|r| r.path)
        .collect();
    assert_eq!(public, ["/healthz"], "only the liveness probe is public");
}

#[tokio::test]
async fn every_protected_route_rejects_missing_and_wrong_credentials() {
    let app = app("http://127.0.0.1:9", 20);
    let table = route_table(state("http://127.0.0.1:9", 20));
    let protected: Vec<_> = table
        .iter()
        .filter(|r| r.access != Access::Public)
        .collect();
    assert!(!protected.is_empty());

    let wrong = bearer("test-only-wrong-key-00000000000000000000000000");
    let cases: [(Option<&str>, &str); 5] = [
        (None, "no Authorization header"),
        (Some(wrong.as_str()), "unknown key"),
        (Some("Basic dGVzdDp0ZXN0"), "wrong scheme"),
        (Some("Bearer"), "empty bearer"),
        (Some(FULL_KEY), "key without the Bearer scheme"),
    ];
    for route in &protected {
        for (authorization, why) in cases {
            let response = app
                .clone()
                .oneshot(request_for(&route.method, route.path, authorization))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{} {} with {why}",
                route.method,
                route.path
            );
            let challenge = response
                .headers()
                .get(header::WWW_AUTHENTICATE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string();
            assert!(challenge.starts_with("Bearer realm="), "{challenge}");
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let body = String::from_utf8_lossy(&bytes);
            assert!(
                !body.contains("test-only"),
                "a credential leaked into the body: {body}"
            );
        }
    }
}

#[tokio::test]
async fn read_only_key_reads_but_gets_403_on_writes() {
    let sim = start_sim();
    let app = app(&sim, 20);
    let table = route_table(state(&sim, 20));
    let read_only = bearer(READ_KEY);

    for route in table
        .iter()
        .filter(|r| matches!(r.access, Access::Write | Access::Operate))
    {
        let response = app
            .clone()
            .oneshot(request_for(&route.method, route.path, Some(&read_only)))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{} {} with a read-only key",
            route.method,
            route.path
        );
    }
    for route in table.iter().filter(|r| r.access == Access::Read) {
        let response = app
            .clone()
            .oneshot(request_for(&route.method, route.path, Some(&read_only)))
            .await
            .unwrap();
        let status = response.status();
        assert!(
            status != StatusCode::UNAUTHORIZED && status != StatusCode::FORBIDDEN,
            "{} {} refused a read-only key: {status}",
            route.method,
            route.path
        );
    }
}

/// Status of `route` called with `key`, past or at the auth gate.
async fn status_with(app: &axum::Router, method: &Method, path: &str, key: &str) -> StatusCode {
    app.clone()
        .oneshot(request_for(method, path, Some(&bearer(key))))
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn full_key_is_accepted_on_every_read_and_write_route() {
    let sim = start_sim();
    let app = app(&sim, 20);
    let table = route_table(state(&sim, 20));
    let full = bearer(FULL_KEY);
    for route in table
        .iter()
        .filter(|r| matches!(r.access, Access::Read | Access::Write))
    {
        let response = app
            .clone()
            .oneshot(request_for(&route.method, route.path, Some(&full)))
            .await
            .unwrap();
        let status = response.status();
        // Past the gate: whatever the handler says (400 for a POST without
        // Idempotency-Key, 404 for an unknown payment), it is not auth.
        assert!(
            status != StatusCode::UNAUTHORIZED && status != StatusCode::FORBIDDEN,
            "{} {} refused a full-access key: {status}",
            route.method,
            route.path
        );
    }
}

#[tokio::test]
async fn healthz_needs_no_credential() {
    let app = app("http://127.0.0.1:9", 20);
    let (status, body) = call(&app, Request::get("/healthz").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "ok");
}

#[tokio::test]
async fn wrong_method_on_a_protected_path_is_still_gated() {
    // Not in the table (only POST /payments is): protected by default.
    let app = app("http://127.0.0.1:9", 20);
    let (status, _) = call(&app, request_for(&Method::GET, "/payments", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(
        &app,
        request_for(&Method::GET, "/payments", Some(&bearer(READ_KEY))),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "unlisted routes need full access"
    );
}

#[tokio::test]
async fn only_an_operator_key_passes_the_operate_routes() {
    // Separation of duties: the key that submits cannot release its own
    // holds, and the key that releases cannot submit.
    let sim = start_sim();
    let app = app(&sim, 20);
    let table = route_table(state(&sim, 20));
    let operate: Vec<_> = table
        .iter()
        .filter(|r| r.access == Access::Operate)
        .collect();
    assert_eq!(operate.len(), 2, "release and cancel");
    for route in &operate {
        for key in [FULL_KEY, READ_KEY] {
            assert_eq!(
                status_with(&app, &route.method, route.path, key).await,
                StatusCode::FORBIDDEN,
                "{} {} with a non-operator key",
                route.method,
                route.path
            );
        }
        let status = status_with(&app, &route.method, route.path, OPERATOR_KEY).await;
        assert!(
            status != StatusCode::UNAUTHORIZED && status != StatusCode::FORBIDDEN,
            "{} {} refused an operator key: {status}",
            route.method,
            route.path
        );
    }
    for route in &table {
        let status = status_with(&app, &route.method, route.path, OPERATOR_KEY).await;
        match route.access {
            Access::Write => assert_eq!(status, StatusCode::FORBIDDEN, "{}", route.path),
            _ => assert!(
                status != StatusCode::UNAUTHORIZED && status != StatusCode::FORBIDDEN,
                "{} {} refused an operator key: {status}",
                route.method,
                route.path
            ),
        }
    }
}
