//! The northbound REST port.
//!
//! | Method | Path | Purpose |
//! |---|---|---|
//! | POST | `/payments` | submit a payment — **`Idempotency-Key` header mandatory** |
//! | GET | `/payments/{key}` | current state of a payment |
//! | POST | `/payments/{key}/reconcile` | drive one reconciliation pass (also runs on the background sweeper) |
//! | GET | `/healthz` | liveness |
//!
//! The service layer is blocking (domain + `ureq`); handlers hop through
//! `spawn_blocking`. Clocks live here, not in the domain: `now` and calendar
//! dates are computed per request.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::payment::{Payment, PaymentState};
use crate::service::{PaymentService, ServiceError, SubmitRequest};
use crate::southbound::FedNowPort;
use crate::store::PaymentStore;

/// Reconciliation timing, in seconds.
#[derive(Debug, Clone, Copy)]
pub struct ReconcileConfig {
    pub timeout_secs: i64,
    pub backoff_secs: i64,
}

pub struct AppState<S, P> {
    pub service: PaymentService<S, P>,
    pub reconcile: ReconcileConfig,
}

/// Build the HTTP router over any store/port combination.
pub fn router<S, P>(state: Arc<AppState<S, P>>) -> Router
where
    S: PaymentStore + Send + Sync + 'static,
    P: FedNowPort + Send + Sync + 'static,
{
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/payments", post(submit_payment::<S, P>))
        .route("/payments/{key}", get(get_payment::<S, P>))
        .route("/payments/{key}/reconcile", post(reconcile_payment::<S, P>))
        .with_state(state)
}

/// Submission body. The idempotency key travels in the `Idempotency-Key`
/// header; money is integer cents, always.
#[derive(Debug, Clone, Deserialize)]
pub struct SubmitBody {
    /// Sender reference, 1..18 alphanumerics (part of the FedNow message id).
    pub reference: String,
    pub amount_cents: u64,
    pub debtor_name: String,
    pub debtor_account: String,
    pub creditor_name: String,
    pub creditor_account: String,
    pub creditor_agent_routing_number: String,
    /// `CONS` or `BIZZ`.
    pub category_purpose: String,
    /// Defaults to the reference.
    pub end_to_end_identification: Option<String>,
    pub uetr: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PaymentView {
    pub idempotency_key: String,
    pub state: String,
    pub message_identification: String,
    pub end_to_end_identification: String,
    pub uetr: Option<String>,
    pub queries_sent: u32,
    pub rejection_reason: Option<String>,
    pub events: usize,
}

impl PaymentView {
    fn from(p: &Payment) -> Self {
        Self {
            idempotency_key: p.idempotency_key.clone(),
            state: state_name(p.state).to_string(),
            message_identification: p.message_identification.clone(),
            end_to_end_identification: p.end_to_end_identification.clone(),
            uetr: p.uetr.clone(),
            queries_sent: p.queries_sent,
            rejection_reason: p.rejection_reason.clone(),
            events: p.events.len(),
        }
    }
}

fn state_name(state: PaymentState) -> &'static str {
    match state {
        PaymentState::Created => "CREATED",
        PaymentState::Validated => "VALIDATED",
        PaymentState::Submitted => "SUBMITTED",
        PaymentState::AckPending => "ACK_PENDING",
        PaymentState::Settled => "SETTLED",
        PaymentState::Rejected => "REJECTED",
        PaymentState::TimeoutUnresolved => "TIMEOUT_UNRESOLVED",
    }
}

async fn submit_payment<S, P>(
    State(state): State<Arc<AppState<S, P>>>,
    headers: HeaderMap,
    Json(body): Json<SubmitBody>,
) -> Response
where
    S: PaymentStore + Send + Sync + 'static,
    P: FedNowPort + Send + Sync + 'static,
{
    let Some(key) = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
    else {
        return (
            StatusCode::BAD_REQUEST,
            "the Idempotency-Key header is mandatory",
        )
            .into_response();
    };

    let now = Utc::now();
    let req = SubmitRequest {
        idempotency_key: key,
        date_yyyymmdd: now.format("%Y%m%d").to_string(),
        sender_reference: body.reference.clone(),
        creation_date_time: now.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        end_to_end_identification: body
            .end_to_end_identification
            .clone()
            .unwrap_or_else(|| body.reference.clone()),
        uetr: body.uetr.clone(),
        amount_cents: body.amount_cents,
        debtor_name: body.debtor_name.clone(),
        debtor_account: body.debtor_account.clone(),
        creditor_name: body.creditor_name.clone(),
        creditor_account: body.creditor_account.clone(),
        creditor_agent_routing_number: body.creditor_agent_routing_number.clone(),
        category_purpose: body.category_purpose.clone(),
        settlement_date: now.format("%Y-%m-%d").to_string(),
    };
    let now_unix = now.timestamp();

    let result = tokio::task::spawn_blocking(move || state.service.submit(&req, now_unix)).await;
    match result {
        Ok(Ok(payment)) => (StatusCode::OK, Json(PaymentView::from(&payment))).into_response(),
        Ok(Err(e)) => service_error(e),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_payment<S, P>(
    State(state): State<Arc<AppState<S, P>>>,
    Path(key): Path<String>,
) -> Response
where
    S: PaymentStore + Send + Sync + 'static,
    P: FedNowPort + Send + Sync + 'static,
{
    match state.service.load(&key) {
        Some(p) => (StatusCode::OK, Json(PaymentView::from(&p))).into_response(),
        None => (StatusCode::NOT_FOUND, "unknown payment").into_response(),
    }
}

async fn reconcile_payment<S, P>(
    State(state): State<Arc<AppState<S, P>>>,
    Path(key): Path<String>,
) -> Response
where
    S: PaymentStore + Send + Sync + 'static,
    P: FedNowPort + Send + Sync + 'static,
{
    let now = Utc::now();
    let date = now.format("%Y%m%d").to_string();
    let now_unix = now.timestamp();
    let cfg = state.reconcile;

    let result = tokio::task::spawn_blocking(move || {
        state
            .service
            .reconcile(&key, &date, now_unix, cfg.timeout_secs, cfg.backoff_secs)
    })
    .await;
    match result {
        Ok(Ok(payment)) => (StatusCode::OK, Json(PaymentView::from(&payment))).into_response(),
        Ok(Err(ServiceError::UnknownPayment(_))) => {
            (StatusCode::NOT_FOUND, "unknown payment").into_response()
        }
        Ok(Err(e)) => service_error(e),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn service_error(e: ServiceError) -> Response {
    match e {
        ServiceError::Validation(codes) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "error": "fednow_profile_violation", "codes": codes })),
        )
            .into_response(),
        other => (StatusCode::INTERNAL_SERVER_ERROR, other.to_string()).into_response(),
    }
}
