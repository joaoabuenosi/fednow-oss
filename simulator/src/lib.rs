//! fednow-sim — a local FedNow Service simulator.
//!
//! Two modes share one scenario engine:
//!
//! - **HTTP dev mode** (`POST /fednow/messages`): synchronous request/response
//!   — a pacs.008 in, the pacs.002 advice out. Zero-setup, good for first
//!   contact and quick tests.
//! - **MQ mode** (`/mq/participants/{rtn}/send` + `/receive`): the semantics of
//!   the real FedNow connection (IBM MQ queue pair). A send is fire-and-forget
//!   (202, no advice in the response); advices arrive later as `FedNowOutgoing`
//!   envelopes on the participant's receive queue. Messages travel wrapped in
//!   the FedNow technical envelope (`FedNowIncoming`/`FedNowOutgoing`, see
//!   `fednow_core::envelope`), with a Business Application Header — exactly the
//!   asynchrony the gateway must survive in production.
//!
//! ## Scenario selection
//!
//! Priority order:
//! 1. **Config file** (`fednow-sim.toml`, or the path in `FEDNOW_SIM_CONFIG`):
//!    maps creditor-agent routing numbers to scenarios.
//! 2. **Amount triggers** (zero-config, Stripe-sandbox style): a settlement
//!    amount ending in `.11` is rejected (`RJCT`/`AC04`), `.22` is accepted
//!    without posting (`ACWP`), `.33` times out (no advice — HTTP 202),
//!    `.44` is settled after a 2-second delay.
//! 3. **Default**: accepted and settled (`ACSC`).
//!
//! Messages that fail FedNow-profile validation are rejected with the
//! simulator-specific proprietary reason `SIMV` and the violated rule codes in
//! `AddtlInf` — the real service's technical error codes live in the
//! access-controlled Technical Specifications (issue #14) and will replace
//! `SIMV` once known.
//!
//! ## The timeout lesson
//!
//! A timed-out payment is *unresolved*, not failed: the simulator still decides
//! a final outcome internally (settled) — the sender just never hears it. The
//! only correct move is a payment status request (pacs.028), posted to the same
//! endpoint, which returns the withheld advice and reveals the truth. Blind
//! resends would double-pay; the simulator exists to make that lesson cheap.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use chrono::{SecondsFormat, Utc};
use fednow_core::builder::{Head001Builder, Pacs002Builder};
use fednow_core::envelope::{self, Direction, EnvelopedDocument};
use fednow_core::validate::{validate_envelope, validate_pacs008, validate_pacs028};
use fednow_core::{pacs002, pacs008, pacs028, ValidationIssue};
use serde::Deserialize;

/// What the simulator should do with an accepted message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scenario {
    /// Advice `ACSC`: accepted, settlement completed.
    Settle,
    /// Advice `ACWP`: accepted without posting.
    AcceptWithoutPosting,
    /// Advice `ACWP` now, with a follow-up status (`ACCC`, `BLCK` or `RJCT`)
    /// queued in the ledger — CTP scenario 4's full arc. The follow-up is
    /// retrieved with a pacs.028 (the HTTP dev mode cannot push).
    AcwpThen(String),
    /// Advice `RJCT` with the given external reason code — a rejection by the
    /// receiving participant (CTP scenario 3), e.g. `AC04`.
    Reject(String),
    /// Advice `RJCT` with the given proprietary reason — a rejection by the
    /// FedNow Service itself (CTP scenario 2), e.g. `E990`.
    RejectService(String),
    /// No advice at all — the hard production case the reconciler exists for.
    /// Internally the payment still settles; pacs.028 reveals it.
    Timeout,
    /// Advice `ACSC`, delivered after the given delay in milliseconds.
    Delay(u64),
}

/// Runtime configuration: creditor-agent routing number → scenario.
#[derive(Debug, Clone, Default)]
pub struct SimConfig {
    pub scenarios: HashMap<String, Scenario>,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(default)]
    scenarios: HashMap<String, RawScenario>,
}

#[derive(Debug, Deserialize)]
struct RawScenario {
    action: String,
    reason: Option<String>,
    delay_ms: Option<u64>,
    follow_up: Option<String>,
}

impl SimConfig {
    /// Parse the TOML configuration format.
    ///
    /// ```toml
    /// [scenarios]
    /// "091000019" = { action = "reject", reason = "AC04" }
    /// "999999992" = { action = "timeout" }
    /// ```
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let raw: RawConfig = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut scenarios = HashMap::new();
        for (rtn, s) in raw.scenarios {
            let scenario = match s.action.as_str() {
                "settle" => Scenario::Settle,
                "accept-without-posting" => match s.follow_up {
                    Some(f) => {
                        let f = f.to_uppercase();
                        if !["ACCC", "BLCK", "RJCT", "PDNG"].contains(&f.as_str()) {
                            return Err(format!("unknown follow_up '{f}' for RTN {rtn}"));
                        }
                        Scenario::AcwpThen(f)
                    }
                    None => Scenario::AcceptWithoutPosting,
                },
                "reject" => Scenario::Reject(s.reason.unwrap_or_else(|| "AC04".to_string())),
                "reject-service" => {
                    Scenario::RejectService(s.reason.unwrap_or_else(|| "E990".to_string()))
                }
                "timeout" => Scenario::Timeout,
                "delay" => Scenario::Delay(s.delay_ms.unwrap_or(2_000)),
                other => return Err(format!("unknown action '{other}' for RTN {rtn}")),
            };
            scenarios.insert(rtn, scenario);
        }
        Ok(Self { scenarios })
    }
}

/// Decide the scenario for a parsed pacs.008.
pub fn decide(config: &SimConfig, doc: &pacs008::Document) -> Scenario {
    let tx = &doc
        .fi_to_fi_customer_credit_transfer
        .credit_transfer_transaction_information[0];

    if let Some(member) = &tx
        .creditor_agent
        .financial_institution_identification
        .clearing_system_member_identification
    {
        if let Some(s) = config.scenarios.get(&member.member_identification) {
            return s.clone();
        }
    }

    match tx.interbank_settlement_amount.value.as_str() {
        v if v.ends_with(".11") => Scenario::Reject("AC04".to_string()),
        v if v.ends_with(".22") => Scenario::AcceptWithoutPosting,
        v if v.ends_with(".33") => Scenario::Timeout,
        v if v.ends_with(".44") => Scenario::Delay(2_000),
        v if v.ends_with(".55") => Scenario::RejectService("E990".to_string()),
        v if v.ends_with(".66") => Scenario::AcwpThen("ACCC".to_string()),
        _ => Scenario::Settle,
    }
}

/// Shared simulator state: configuration plus the advice ledger — every
/// advice each payment ever produced, in order, keyed by the original message
/// id, so a pacs.028 can always answer "what happened to X?" with the latest.
pub struct SimState {
    pub config: SimConfig,
    advices: Mutex<HashMap<String, Vec<String>>>,
    /// MQ mode: per-participant receive queues of `FedNowOutgoing` envelopes.
    queues: Mutex<HashMap<String, VecDeque<String>>>,
}

impl SimState {
    fn enqueue(&self, participant: &str, envelope_xml: String) {
        self.queues
            .lock()
            .unwrap()
            .entry(participant.to_string())
            .or_default()
            .push_back(envelope_xml);
    }
}

/// Build the HTTP router (exposed separately so tests drive it without a socket).
pub fn router(config: SimConfig) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/fednow/messages", post(handle_message))
        .route("/mq/participants/{rtn}/send", post(handle_mq_send))
        .route("/mq/participants/{rtn}/receive", get(handle_mq_receive))
        .with_state(Arc::new(SimState {
            config,
            advices: Mutex::new(HashMap::new()),
            queues: Mutex::new(HashMap::new()),
        }))
}

/// One endpoint, like one MQ channel: the message type is sniffed from the
/// namespace.
async fn handle_message(State(state): State<Arc<SimState>>, body: Bytes) -> Response {
    let Ok(xml) = std::str::from_utf8(&body) else {
        return (StatusCode::BAD_REQUEST, "body is not valid UTF-8").into_response();
    };

    if xml.contains(pacs028::NAMESPACE) {
        return handle_pacs028(&state, xml);
    }
    if xml.contains(pacs008::NAMESPACE) {
        return handle_pacs008(&state, xml).await;
    }
    (
        StatusCode::BAD_REQUEST,
        "unrecognized message namespace (expected pacs.008 or pacs.028)",
    )
        .into_response()
}

async fn handle_pacs008(state: &SimState, xml: &str) -> Response {
    let doc = match pacs008::parse(xml) {
        Ok(d) => d,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("not a pacs.008: {e}")).into_response();
        }
    };
    if doc
        .fi_to_fi_customer_credit_transfer
        .credit_transfer_transaction_information
        .is_empty()
    {
        return (StatusCode::BAD_REQUEST, "no CdtTrfTxInf present").into_response();
    }

    let issues = validate_pacs008(&doc);
    let scenario = if issues.is_empty() {
        decide(&state.config, &doc)
    } else {
        // Profile-invalid messages are always rejected by the service itself,
        // whatever the scenario.
        Scenario::RejectService("SIMV".to_string())
    };

    // The payment reaches a final state no matter what the sender sees: for
    // the timeout scenario the final outcome is "settled", it just goes
    // unadvised until a pacs.028 asks.
    let final_scenario = match &scenario {
        Scenario::Timeout | Scenario::Delay(_) => &Scenario::Settle,
        Scenario::AcwpThen(_) => &Scenario::AcceptWithoutPosting,
        s => s,
    };
    let advice = match advice_xml(&doc, final_scenario, &issues) {
        Ok(xml) => xml,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    let orig_msg_id = doc
        .fi_to_fi_customer_credit_transfer
        .group_header
        .message_identification
        .clone();
    let mut ledger_entry = vec![advice.clone()];
    if let Scenario::AcwpThen(follow_up) = &scenario {
        // The receiving participant's later status, relayed by the service —
        // queued in the ledger, retrieved with a pacs.028.
        match follow_up_advice_xml(&doc, follow_up) {
            Ok(xml) => ledger_entry.push(xml),
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
        }
    }
    state
        .advices
        .lock()
        .unwrap()
        .insert(orig_msg_id, ledger_entry);

    match scenario {
        Scenario::Timeout => StatusCode::ACCEPTED.into_response(),
        Scenario::Delay(ms) => {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            xml_ok(advice)
        }
        _ => xml_ok(advice),
    }
}

/// Answer a payment status request with the stored advice for the original
/// message — the reconciliation flow.
fn handle_pacs028(state: &SimState, xml: &str) -> Response {
    let doc = match pacs028::parse(xml) {
        Ok(d) => d,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("not a pacs.028: {e}")).into_response();
        }
    };
    let issues = validate_pacs028(&doc);
    if !issues.is_empty() {
        let codes: Vec<&str> = issues.iter().map(|i| i.code).collect();
        return (
            StatusCode::BAD_REQUEST,
            format!("invalid pacs.028: {}", codes.join(" ")),
        )
            .into_response();
    }

    let Some(orig_msg_id) = doc.fi_to_fi_payment_status_request.transaction_information[0]
        .original_group_information
        .as_ref()
        .map(|o| o.original_message_identification.as_str())
    else {
        return (StatusCode::BAD_REQUEST, "missing OrgnlGrpInf").into_response();
    };

    match state
        .advices
        .lock()
        .unwrap()
        .get(orig_msg_id)
        .and_then(|v| v.last())
    {
        Some(advice) => xml_ok(advice.clone()),
        None => (
            StatusCode::NOT_FOUND,
            format!("no payment known for original message id '{orig_msg_id}'"),
        )
            .into_response(),
    }
}

fn xml_ok(xml: String) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/xml")],
        xml,
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// MQ mode: fire-and-forget send + per-participant receive queue.
// ---------------------------------------------------------------------------

/// `POST /mq/participants/{rtn}/send` — accept a `FedNowIncoming` envelope.
///
/// Fire-and-forget like an MQ PUT: on success the response is `202 Accepted`
/// with no body, and any advice arrives later as a `FedNowOutgoing` envelope
/// on the participant's receive queue — including rejections for
/// profile-invalid messages. Only structurally broken input (not an envelope,
/// unparseable Document) gets a synchronous 400; the real service would
/// deliver an admi.002 message reject there, which the sim does not model yet.
async fn handle_mq_send(
    State(state): State<Arc<SimState>>,
    Path(rtn): Path<String>,
    body: Bytes,
) -> Response {
    let Ok(xml) = std::str::from_utf8(&body) else {
        return (StatusCode::BAD_REQUEST, "body is not valid UTF-8").into_response();
    };

    let env = match envelope::parse(xml) {
        Ok(e) => e,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("{e}")).into_response(),
    };
    if env.direction != Direction::Incoming {
        return (
            StatusCode::BAD_REQUEST,
            "participants send FedNowIncoming envelopes",
        )
            .into_response();
    }

    match &env.document {
        EnvelopedDocument::CustomerCreditTransfer(doc) => {
            if doc
                .fi_to_fi_customer_credit_transfer
                .credit_transfer_transaction_information
                .is_empty()
            {
                return (StatusCode::BAD_REQUEST, "no CdtTrfTxInf present").into_response();
            }
            let issues = validate_envelope(&env);
            mq_process_credit_transfer(&state, &rtn, doc, &issues)
        }
        EnvelopedDocument::PaymentStatusRequest(doc) => {
            mq_process_status_request(&state, &rtn, doc)
        }
        other => (
            StatusCode::BAD_REQUEST,
            format!(
                "message type {} is not supported by the simulator's MQ mode yet",
                other.message_name()
            ),
        )
            .into_response(),
    }
}

/// `GET /mq/participants/{rtn}/receive` — destructive get of the next queued
/// `FedNowOutgoing` envelope (like an MQ GET); `204 No Content` when empty.
async fn handle_mq_receive(
    State(state): State<Arc<SimState>>,
    Path(rtn): Path<String>,
) -> Response {
    let next = state
        .queues
        .lock()
        .unwrap()
        .get_mut(&rtn)
        .and_then(|q| q.pop_front());
    match next {
        Some(envelope_xml) => xml_ok(envelope_xml),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

/// Scenario engine for a credit transfer arriving over MQ: same decisions as
/// the HTTP mode, but every advice is enqueued instead of returned — and the
/// ACWP follow-up can actually be *pushed*, no pacs.028 needed.
fn mq_process_credit_transfer(
    state: &Arc<SimState>,
    rtn: &str,
    doc: &pacs008::Document,
    issues: &[ValidationIssue],
) -> Response {
    let scenario = if issues.is_empty() {
        decide(&state.config, doc)
    } else {
        Scenario::RejectService("SIMV".to_string())
    };

    let final_scenario = match &scenario {
        Scenario::Timeout | Scenario::Delay(_) => &Scenario::Settle,
        Scenario::AcwpThen(_) => &Scenario::AcceptWithoutPosting,
        s => s,
    };
    let advice = match advice_xml(doc, final_scenario, issues) {
        Ok(xml) => xml,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    let hdr = &doc.fi_to_fi_customer_credit_transfer.group_header;
    let orig_msg_id = hdr.message_identification.clone();

    // Ledger entry mirrors the HTTP mode so pacs.028 works identically.
    let mut ledger_entry = vec![advice.clone()];
    let follow_up = if let Scenario::AcwpThen(status) = &scenario {
        match follow_up_advice_xml(doc, status) {
            Ok(xml) => {
                ledger_entry.push(xml.clone());
                Some(xml)
            }
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
        }
    } else {
        None
    };
    state
        .advices
        .lock()
        .unwrap()
        .insert(orig_msg_id.clone(), ledger_entry);

    let wrapped = wrap_advice(&advice, rtn, &advice_id(&orig_msg_id));
    match scenario {
        Scenario::Timeout => {} // no advice: the reconciler's case
        Scenario::Delay(ms) => {
            let state = Arc::clone(state);
            let rtn = rtn.to_string();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                state.enqueue(&rtn, wrapped);
            });
        }
        Scenario::AcwpThen(_) => {
            state.enqueue(rtn, wrapped);
            // The receiving participant's status arrives shortly after — MQ
            // mode can push it, unlike the HTTP dev mode.
            let wrapped_follow_up = wrap_advice(
                follow_up.as_deref().unwrap_or_default(),
                rtn,
                &follow_up_advice_id(&orig_msg_id),
            );
            let state = Arc::clone(state);
            let rtn = rtn.to_string();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                state.enqueue(&rtn, wrapped_follow_up);
            });
        }
        _ => state.enqueue(rtn, wrapped),
    }

    StatusCode::ACCEPTED.into_response()
}

/// pacs.028 over MQ: enqueue the latest stored advice for the original
/// message. Unknown payments are a synchronous 404 as a dev convenience (the
/// real service answers asynchronously).
fn mq_process_status_request(
    state: &Arc<SimState>,
    rtn: &str,
    doc: &pacs028::Document,
) -> Response {
    let issues = validate_pacs028(doc);
    if !issues.is_empty() {
        let codes: Vec<&str> = issues.iter().map(|i| i.code).collect();
        return (
            StatusCode::BAD_REQUEST,
            format!("invalid pacs.028: {}", codes.join(" ")),
        )
            .into_response();
    }
    let Some(orig_msg_id) = doc.fi_to_fi_payment_status_request.transaction_information[0]
        .original_group_information
        .as_ref()
        .map(|o| o.original_message_identification.as_str())
    else {
        return (StatusCode::BAD_REQUEST, "missing OrgnlGrpInf").into_response();
    };

    let advice = state
        .advices
        .lock()
        .unwrap()
        .get(orig_msg_id)
        .and_then(|v| v.last())
        .cloned();
    match advice {
        Some(advice) => {
            // Recover the advice's own MsgId for the BAH.
            let biz_msg_idr = pacs002::parse(&advice)
                .map(|d| {
                    d.fi_to_fi_payment_status_report
                        .group_header
                        .message_identification
                })
                .unwrap_or_else(|_| advice_id(orig_msg_id));
            state.enqueue(rtn, wrap_advice(&advice, rtn, &biz_msg_idr));
            StatusCode::ACCEPTED.into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            format!("no payment known for original message id '{orig_msg_id}'"),
        )
            .into_response(),
    }
}

/// Wrap a pacs.002 advice in a `FedNowOutgoing` envelope with a service BAH.
fn wrap_advice(advice_xml: &str, to_rtn: &str, biz_msg_idr: &str) -> String {
    let now_ts = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let bah = Head001Builder::new(
        "021150706",
        to_rtn,
        truncate(biz_msg_idr, 35),
        "pacs.002.001.10",
        now_ts,
    )
    .to_xml()
    .expect("BAH serialization is infallible for valid inputs");
    envelope::build(
        Direction::Outgoing,
        "FedNowPaymentStatus",
        &bah,
        strip_xml_declaration(advice_xml),
        None,
    )
}

/// Drop a leading `<?xml …?>` declaration: envelope children are embedded in
/// a larger document and must not carry one.
fn strip_xml_declaration(xml: &str) -> &str {
    let trimmed = xml.trim_start();
    match trimmed.strip_prefix("<?xml") {
        Some(rest) => rest
            .split_once("?>")
            .map(|(_, tail)| tail.trim_start())
            .unwrap_or(trimmed),
        None => trimmed,
    }
}

/// Build the pacs.002 service advice for an accepted-for-processing pacs.008.
fn advice_xml(
    doc: &pacs008::Document,
    scenario: &Scenario,
    issues: &[ValidationIssue],
) -> Result<String, String> {
    let msg = &doc.fi_to_fi_customer_credit_transfer;
    let hdr = &msg.group_header;
    let tx = &msg.credit_transfer_transaction_information[0];

    let now = Utc::now();
    let now_ts = now.to_rfc3339_opts(SecondsFormat::Secs, true);
    let today = now.date_naive().to_string();

    // Service advice MsgId is free-form Max35Text; derive it from the original
    // so runs are traceable.
    let advice_id = advice_id(&hdr.message_identification);

    let status = match scenario {
        Scenario::Settle => "ACSC",
        Scenario::AcceptWithoutPosting => "ACWP",
        Scenario::Reject(_) | Scenario::RejectService(_) => "RJCT",
        Scenario::Timeout | Scenario::Delay(_) | Scenario::AcwpThen(_) => {
            unreachable!("mapped to a final scenario before advice building")
        }
    };

    let instg = agent_rtn(&tx.instructing_agent).unwrap_or("021150706");
    let instd = agent_rtn(&tx.instructed_agent).unwrap_or("021150706");

    let mut builder = Pacs002Builder::new(
        advice_id,
        now_ts.clone(),
        hdr.message_identification.clone(),
        hdr.creation_date_time.clone(),
        status,
        instg,
        instd,
    )
    .original_end_to_end_identification(
        tx.payment_identification.end_to_end_identification.clone(),
    );

    if let Some(uetr) = &tx.payment_identification.uetr {
        builder = builder.original_uetr(uetr.clone());
    }
    if let Some(instr_id) = &tx.payment_identification.instruction_identification {
        builder = builder.original_instruction_identification(instr_id.clone());
    }

    builder = match scenario {
        Scenario::Settle => builder
            .acceptance_date_time(now_ts)
            .effective_interbank_settlement_date(today),
        Scenario::AcceptWithoutPosting => builder.acceptance_date_time(now_ts),
        Scenario::RejectService(code) if code == "SIMV" => {
            // Simulator-specific: validation failure, rule codes in AddtlInf.
            let detail: Vec<&str> = issues.iter().map(|i| i.code).take(5).collect();
            builder
                .reason_proprietary("SIMV")
                .additional_information(truncate(&detail.join(" "), 105))
        }
        Scenario::RejectService(code) => builder.reason_proprietary(code.clone()),
        Scenario::Reject(code) => builder.reason_code(code.clone()),
        Scenario::Timeout | Scenario::Delay(_) | Scenario::AcwpThen(_) => unreachable!(),
    };

    builder.to_xml().map_err(|e| e.to_string())
}

/// The receiving participant's follow-up status after an ACWP, relayed by the
/// service (CTP scenario 4, steps 5–6).
fn follow_up_advice_xml(doc: &pacs008::Document, status: &str) -> Result<String, String> {
    let msg = &doc.fi_to_fi_customer_credit_transfer;
    let hdr = &msg.group_header;
    let tx = &msg.credit_transfer_transaction_information[0];

    let now_ts = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let advice_id = follow_up_advice_id(&hdr.message_identification);

    let mut builder = Pacs002Builder::new(
        advice_id,
        now_ts,
        hdr.message_identification.clone(),
        hdr.creation_date_time.clone(),
        status,
        agent_rtn(&tx.instructing_agent).unwrap_or("021150706"),
        agent_rtn(&tx.instructed_agent).unwrap_or("021150706"),
    )
    .original_end_to_end_identification(
        tx.payment_identification.end_to_end_identification.clone(),
    );
    if let Some(uetr) = &tx.payment_identification.uetr {
        builder = builder.original_uetr(uetr.clone());
    }
    if status == "RJCT" {
        // A post-ACWP rejection: funds already settled, a payment return
        // (pacs.004) follows in the real flow.
        builder = builder.reason_code("AC04");
    }
    builder.to_xml().map_err(|e| e.to_string())
}

/// Advice MsgId derived from the original, traceable across a run.
fn advice_id(orig_id: &str) -> String {
    format!("SIM{}", &orig_id[..orig_id.len().min(32)])
}

/// Follow-up advice MsgId (post-ACWP status relay).
fn follow_up_advice_id(orig_id: &str) -> String {
    format!("SIMF{}", &orig_id[..orig_id.len().min(31)])
}

fn agent_rtn(
    agent: &Option<fednow_core::pacs008::BranchAndFinancialInstitutionIdentification>,
) -> Option<&str> {
    agent
        .as_ref()?
        .financial_institution_identification
        .clearing_system_member_identification
        .as_ref()
        .map(|m| m.member_identification.as_str())
}

fn truncate(s: &str, max: usize) -> &str {
    &s[..s.len().min(max)]
}
