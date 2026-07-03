//! fednow-sim — a local FedNow Service simulator.
//!
//! v0 speaks the HTTP dev mode described in the project requirements: POST a
//! pacs.008 customer credit transfer and receive the pacs.002 advice the FedNow
//! Service would send, under configurable scenarios. An MQ-compatible interface
//! comes later; the scenario engine is shared between both.
//!
//! ## Scenario selection
//!
//! Priority order:
//! 1. **Config file** (`fednow-sim.toml`, or the path in `FEDNOW_SIM_CONFIG`):
//!    maps creditor-agent routing numbers to scenarios.
//! 2. **Amount triggers** (zero-config, Stripe-sandbox style): a settlement
//!    amount ending in `.11` is rejected (`RJCT`/`AC04`), `.22` is accepted
//!    without posting (`ACWP`), `.33` times out (no advice — HTTP 202).
//! 3. **Default**: accepted and settled (`ACSC`).
//!
//! Messages that fail FedNow-profile validation are rejected with the
//! simulator-specific proprietary reason `SIMV` and the violated rule codes in
//! `AddtlInf` — the real service's technical error codes live in the
//! access-controlled Technical Specifications (issue #14) and will replace
//! `SIMV` once known.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use chrono::{SecondsFormat, Utc};
use fednow_core::builder::Pacs002Builder;
use fednow_core::validate::validate_pacs008;
use fednow_core::{pacs008, ValidationIssue};
use serde::Deserialize;

/// What the simulator should do with an accepted message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scenario {
    /// Advice `ACSC`: accepted, settlement completed.
    Settle,
    /// Advice `ACWP`: accepted without posting.
    AcceptWithoutPosting,
    /// Advice `RJCT` with the given external reason code.
    Reject(String),
    /// No advice at all — the hard production case the reconciler exists for.
    Timeout,
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
                "accept-without-posting" => Scenario::AcceptWithoutPosting,
                "reject" => Scenario::Reject(s.reason.unwrap_or_else(|| "AC04".to_string())),
                "timeout" => Scenario::Timeout,
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
        _ => Scenario::Settle,
    }
}

/// Build the HTTP router (exposed separately so tests drive it without a socket).
pub fn router(config: SimConfig) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/fednow/messages", post(handle_message))
        .with_state(Arc::new(config))
}

async fn handle_message(State(config): State<Arc<SimConfig>>, body: Bytes) -> Response {
    let Ok(xml) = std::str::from_utf8(&body) else {
        return (StatusCode::BAD_REQUEST, "body is not valid UTF-8").into_response();
    };

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
        decide(&config, &doc)
    } else {
        // Profile-invalid messages are always rejected, whatever the scenario.
        Scenario::Reject("SIMV".to_string())
    };

    match scenario {
        Scenario::Timeout => StatusCode::ACCEPTED.into_response(),
        s => {
            let advice = advice_xml(&doc, &s, &issues);
            match advice {
                Ok(xml) => (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/xml")],
                    xml,
                )
                    .into_response(),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
            }
        }
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
    let orig_id = &hdr.message_identification;
    let advice_id: String = format!("SIM{}", &orig_id[..orig_id.len().min(32)]);

    let status = match scenario {
        Scenario::Settle => "ACSC",
        Scenario::AcceptWithoutPosting => "ACWP",
        Scenario::Reject(_) => "RJCT",
        Scenario::Timeout => unreachable!("timeout produces no advice"),
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
        Scenario::Reject(code) if code == "SIMV" => {
            // Simulator-specific: validation failure, rule codes in AddtlInf.
            let detail: Vec<&str> = issues.iter().map(|i| i.code).take(5).collect();
            builder
                .reason_proprietary("SIMV")
                .additional_information(truncate(&detail.join(" "), 105))
        }
        Scenario::Reject(code) => builder.reason_code(code.clone()),
        Scenario::Timeout => unreachable!(),
    };

    builder.to_xml().map_err(|e| e.to_string())
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
