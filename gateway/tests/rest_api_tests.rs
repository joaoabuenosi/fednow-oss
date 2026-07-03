//! REST API tests: the northbound contract against a live in-process sim.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use fednow_gateway::http::{router, AppState, ReconcileConfig};
use fednow_gateway::{HttpSimPort, InMemoryStore, PaymentService};
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

fn app(sim_url: &str, timeout_secs: i64) -> axum::Router {
    router(Arc::new(AppState {
        service: PaymentService::new(InMemoryStore::new(), HttpSimPort::new(sim_url), "021040078"),
        reconcile: ReconcileConfig {
            timeout_secs,
            backoff_secs: 0,
        },
    }))
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
            "creditor_agent_routing_number": "091000019",
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
        Request::get("/payments/r1").body(Body::empty()).unwrap(),
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
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["state"], "TIMEOUT_UNRESOLVED");

    let (status, view) = call(
        &app,
        Request::get("/ops/summary").body(Body::empty()).unwrap(),
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
