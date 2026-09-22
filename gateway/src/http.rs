//! The northbound REST port.
//!
//! | Method | Path | Access | Purpose |
//! |---|---|---|---|
//! | POST | `/payments` | full | submit a payment — **`Idempotency-Key` header mandatory** |
//! | GET | `/payments/{key}` | read | current state of a payment |
//! | POST | `/payments/{key}/reconcile` | full | drive one reconciliation pass (also runs on the background sweeper) |
//! | GET | `/ops/summary` | read | operational snapshot |
//! | GET | `/healthz` | public | liveness |
//!
//! Every route but `/healthz` needs `Authorization: Bearer <key>` — see
//! [`crate::auth`]. Routes are registered through [`Routes`], which records
//! each one's access level; [`route_table`] returns that record, and one
//! middleware enforces it. A route the table does not know requires a
//! full-access key.
//!
//! The service layer is blocking (domain + `ureq`); handlers hop through
//! `spawn_blocking`. Clocks live here, not in the domain: `now` and calendar
//! dates are computed per request.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::handler::Handler;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{on, MethodFilter};
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::auth::{authorize, Access, ApiKeys, Guard, RouteSpec};
use crate::payment::Payment;
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
    /// Northbound credentials. Required: an [`ApiKeys`] cannot be empty, so
    /// there is no router without authentication.
    pub api_keys: ApiKeys,
}

/// Build the HTTP router over any store/port combination.
pub fn router<S, P>(state: Arc<AppState<S, P>>) -> Router
where
    S: PaymentStore + Send + Sync + 'static,
    P: FedNowPort + Send + Sync + 'static,
{
    router_and_table(state).0
}

/// Every registered route with its access level — the same record the auth
/// middleware enforces. Tests walk it so a new route cannot ship unchecked.
pub fn route_table<S, P>(state: Arc<AppState<S, P>>) -> Vec<RouteSpec>
where
    S: PaymentStore + Send + Sync + 'static,
    P: FedNowPort + Send + Sync + 'static,
{
    router_and_table(state).1
}

fn router_and_table<S, P>(state: Arc<AppState<S, P>>) -> (Router, Vec<RouteSpec>)
where
    S: PaymentStore + Send + Sync + 'static,
    P: FedNowPort + Send + Sync + 'static,
{
    let keys = state.api_keys.clone();
    let Routes { router, table } = Routes::new()
        .add(Verb::Get, "/healthz", Access::Public, healthz)
        .add(
            Verb::Post,
            "/payments",
            Access::Write,
            submit_payment::<S, P>,
        )
        .add(
            Verb::Get,
            "/payments/{key}",
            Access::Read,
            get_payment::<S, P>,
        )
        .add(
            Verb::Post,
            "/payments/{key}/reconcile",
            Access::Write,
            reconcile_payment::<S, P>,
        )
        .add(Verb::Get, "/ops/summary", Access::Read, ops_summary::<S, P>);
    let guard = Guard {
        keys,
        routes: Arc::new(table.clone()),
    };
    // `route_layer`: runs after routing, so the middleware sees the matched
    // route template. It covers every route registered above.
    let router = router
        .route_layer(axum::middleware::from_fn_with_state(guard, authorize))
        .with_state(state);
    (router, table)
}

#[derive(Debug, Clone, Copy)]
enum Verb {
    Get,
    Post,
}

impl Verb {
    fn method(self) -> Method {
        match self {
            Verb::Get => Method::GET,
            Verb::Post => Method::POST,
        }
    }

    fn filter(self) -> MethodFilter {
        match self {
            Verb::Get => MethodFilter::GET,
            Verb::Post => MethodFilter::POST,
        }
    }
}

/// Router builder that records each route's access level as it is added,
/// so the access table and the router cannot disagree.
struct Routes<St> {
    router: Router<St>,
    table: Vec<RouteSpec>,
}

impl<St: Clone + Send + Sync + 'static> Routes<St> {
    fn new() -> Self {
        Self {
            router: Router::new(),
            table: Vec::new(),
        }
    }

    fn add<H, T>(mut self, verb: Verb, path: &'static str, access: Access, handler: H) -> Self
    where
        H: Handler<T, St>,
        T: 'static,
    {
        self.router = self.router.route(path, on(verb.filter(), handler));
        self.table.push(RouteSpec {
            method: verb.method(),
            path,
            access,
        });
        self
    }
}

async fn healthz() -> &'static str {
    "ok"
}

/// Operational snapshot for 24x7 operators and monitoring (read-only keys
/// suffice). Not a probe: it needs a credential; `/healthz` does not.
#[derive(Debug, Serialize)]
struct OpsSummaryView {
    payments_total: usize,
    by_state: std::collections::BTreeMap<&'static str, usize>,
    outbox_pending: usize,
    oldest_unresolved_age_secs: Option<i64>,
}

async fn ops_summary<S, P>(State(state): State<Arc<AppState<S, P>>>) -> Response
where
    S: PaymentStore + Send + Sync + 'static,
    P: FedNowPort + Send + Sync + 'static,
{
    let now_unix = Utc::now().timestamp();
    let result = tokio::task::spawn_blocking(move || state.service.summary(now_unix)).await;
    match result {
        Ok(s) => (
            StatusCode::OK,
            Json(OpsSummaryView {
                payments_total: s.payments_total,
                by_state: s.by_state,
                outbox_pending: s.outbox_pending,
                oldest_unresolved_age_secs: s.oldest_unresolved_age_secs,
            }),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
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
    /// The pre-send risk check, when one ran. Absent (not `null`) otherwise,
    /// so a gateway without a risk provider answers exactly as before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<RiskView>,
}

/// The risk check as the API reports it: the same fields the audit event
/// records, minus timings.
#[derive(Debug, Serialize)]
pub struct RiskView {
    /// `allow`, `hold` or `refuse`.
    pub outcome: &'static str,
    pub reason: Option<String>,
    /// `provider`, `timeout`, `error` or `budget_exhausted`.
    pub source: &'static str,
}

impl PaymentView {
    fn from(p: &Payment) -> Self {
        Self {
            idempotency_key: p.idempotency_key.clone(),
            state: p.state.name().to_string(),
            message_identification: p.message_identification.clone(),
            end_to_end_identification: p.end_to_end_identification.clone(),
            uetr: p.uetr.clone(),
            queries_sent: p.queries_sent,
            rejection_reason: p.rejection_reason.clone(),
            events: p.events.len(),
            risk: p.risk_outcome.map(|outcome| RiskView {
                outcome: outcome.name(),
                reason: p.risk_reason.clone(),
                source: p.risk_source.map_or("provider", |s| s.name()),
            }),
        }
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
