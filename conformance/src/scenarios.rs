//! Drive a FedNow-Service-side endpoint through the official CTP
//! credit-transfer scenarios (Readiness Portal Guide, chapter 3).
//!
//! The endpoint under test plays the FedNow Service + receiving participant —
//! exactly what `fednow-sim` does, but any implementation of the same HTTP dev
//! contract can be pointed at. Each scenario submits a FedNow-conformant
//! pacs.008 and asserts the advice choreography.

use fednow_core::builder::{fednow_message_id, Pacs008Builder, Pacs028Builder};
use fednow_core::pacs002;
use fednow_core::validate::{validate_pacs002_direction, Pacs002Direction};

#[derive(Debug)]
pub struct ScenarioResult {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

struct Ctx {
    base_url: String,
    sender_rtn: String,
    creditor_rtn: String,
}

impl Ctx {
    fn post(&self, xml: &str) -> Result<(u16, String), String> {
        let url = format!("{}/fednow/messages", self.base_url);
        match ureq::post(&url)
            .set("content-type", "application/xml")
            .send_string(xml)
        {
            Ok(resp) => {
                let status = resp.status();
                let body = resp.into_string().map_err(|e| e.to_string())?;
                Ok((status, body))
            }
            Err(ureq::Error::Status(status, resp)) => {
                Ok((status, resp.into_string().unwrap_or_default()))
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn pacs008(&self, reference: &str, amount_cents: u64) -> String {
        Pacs008Builder::new(
            fednow_message_id("20260702", &self.sender_rtn, reference),
            "2026-07-02T15:30:00Z",
            format!("E2E-{reference}"),
            amount_cents,
            self.sender_rtn.clone(),
            self.creditor_rtn.clone(),
        )
        .interbank_settlement_date("2026-07-02")
        .category_purpose("CONS")
        .debtor_name("Conformance Debtor")
        .debtor_account("123456789012")
        .creditor_name("Conformance Creditor")
        .creditor_account("987654321000")
        .to_xml()
        .expect("builder output")
    }

    fn pacs028(&self, reference: &str, original_msg_id: &str) -> String {
        Pacs028Builder::new(
            fednow_message_id("20260702", &self.sender_rtn, reference),
            "2026-07-02T15:40:00Z",
            original_msg_id,
            "2026-07-02T15:30:00Z",
            self.sender_rtn.clone(),
            "021150706",
        )
        .to_xml()
        .expect("builder output")
    }
}

/// Expectation on a returned advice.
fn expect_advice(body: &str, status_code: &str, external_reason: bool) -> Result<(), String> {
    let doc = pacs002::parse(body).map_err(|e| format!("advice does not parse: {e}"))?;
    let issues = validate_pacs002_direction(&doc, Pacs002Direction::ServiceToParticipant);
    if !issues.is_empty() {
        return Err(format!(
            "advice is not direction-clean: {:?}",
            issues.iter().map(|i| i.code).collect::<Vec<_>>()
        ));
    }
    let tx = &doc
        .fi_to_fi_payment_status_report
        .transaction_information_and_status[0];
    if tx.transaction_status.as_deref() != Some(status_code) {
        return Err(format!(
            "expected TxSts {status_code}, got {:?}",
            tx.transaction_status
        ));
    }
    if status_code == "RJCT" {
        let has = tx
            .status_reason_information
            .first()
            .and_then(|s| s.reason.as_ref());
        match has {
            Some(r) if external_reason && r.code.is_some() => {}
            Some(r) if !external_reason && r.proprietary.is_some() => {}
            other => {
                return Err(format!(
                    "expected {} rejection reason, got {other:?}",
                    if external_reason {
                        "external (Cd)"
                    } else {
                        "proprietary (Prtry)"
                    }
                ))
            }
        }
    }
    Ok(())
}

/// Run all CTP credit-transfer scenarios against `base_url`.
pub fn run(base_url: &str) -> Vec<ScenarioResult> {
    let ctx = Ctx {
        base_url: base_url.trim_end_matches('/').to_string(),
        sender_rtn: "021040078".to_string(),
        creditor_rtn: "091000019".to_string(),
    };

    let mut results = Vec::new();
    let mut run_one = |name: &'static str, f: &dyn Fn(&Ctx) -> Result<(), String>| {
        let outcome = f(&ctx);
        results.push(match outcome {
            Ok(()) => ScenarioResult {
                name,
                passed: true,
                detail: "ok".to_string(),
            },
            Err(detail) => ScenarioResult {
                name,
                passed: false,
                detail,
            },
        });
    };

    run_one("cct-1-happy-path", &|ctx| {
        let (status, body) = ctx.post(&ctx.pacs008("CONF0001", 125_000))?;
        if status != 200 {
            return Err(format!("expected 200, got {status}"));
        }
        expect_advice(&body, "ACSC", true)
    });

    run_one("cct-2-service-reject", &|ctx| {
        let (status, body) = ctx.post(&ctx.pacs008("CONF0002", 125_055))?;
        if status != 200 {
            return Err(format!("expected 200, got {status}"));
        }
        expect_advice(&body, "RJCT", false)
    });

    run_one("cct-3-participant-reject", &|ctx| {
        let (status, body) = ctx.post(&ctx.pacs008("CONF0003", 125_011))?;
        if status != 200 {
            return Err(format!("expected 200, got {status}"));
        }
        expect_advice(&body, "RJCT", true)
    });

    run_one("cct-4-accept-without-posting", &|ctx| {
        let (status, body) = ctx.post(&ctx.pacs008("CONF0004", 125_066))?;
        if status != 200 {
            return Err(format!("expected 200, got {status}"));
        }
        expect_advice(&body, "ACWP", true)?;
        // The follow-up (funds credited) is retrieved with a status request.
        let query = ctx.pacs028(
            "CONFQ004",
            &fednow_message_id("20260702", &ctx.sender_rtn, "CONF0004"),
        );
        let (status, body) = ctx.post(&query)?;
        if status != 200 {
            return Err(format!("follow-up query: expected 200, got {status}"));
        }
        expect_advice(&body, "ACCC", true)
    });

    run_one("cct-5-timeout-then-query", &|ctx| {
        let (status, body) = ctx.post(&ctx.pacs008("CONF0005", 125_033))?;
        if status != 202 {
            return Err(format!(
                "timeout scenario must answer 202, got {status}: {body}"
            ));
        }
        let query = ctx.pacs028(
            "CONFQ005",
            &fednow_message_id("20260702", &ctx.sender_rtn, "CONF0005"),
        );
        let (status, body) = ctx.post(&query)?;
        if status != 200 {
            return Err(format!("query: expected 200, got {status}"));
        }
        expect_advice(&body, "ACSC", true)
    });

    run_one("cct-6-query-unknown-is-not-found", &|ctx| {
        let query = ctx.pacs028(
            "CONFQ006",
            &fednow_message_id("20260702", &ctx.sender_rtn, "NEVERSENT9"),
        );
        let (status, _) = ctx.post(&query)?;
        if status != 404 {
            return Err(format!("unknown original must answer 404, got {status}"));
        }
        Ok(())
    });

    results
}
