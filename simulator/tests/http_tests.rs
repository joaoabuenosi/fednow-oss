//! End-to-end tests for the fednow-sim HTTP dev mode: a FedNow-conformant
//! pacs.008 goes in, the correct pacs.002 service advice comes out.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use fednow_core::builder::{fednow_message_id, Pacs008Builder};
use fednow_core::pacs002;
use fednow_core::validate::{validate_pacs002_direction, Pacs002Direction};
use fednow_sim::{router, Scenario, SimConfig};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn valid_pacs008(amount_cents: u64) -> String {
    Pacs008Builder::new(
        fednow_message_id("20260702", "021040078", "SIMTEST001"),
        "2026-07-02T15:30:00Z",
        "E2E-SIMTEST-0001",
        amount_cents,
        "021040078",
        "091000019",
    )
    .uetr("8a562c67-ca16-48ba-b074-65581be6f001")
    .interbank_settlement_date("2026-07-02")
    .category_purpose("CONS")
    .debtor_name("Jane Example Debtor")
    .debtor_account("123456789012")
    .creditor_name("John Example Creditor")
    .creditor_account("987654321000")
    .to_xml()
    .unwrap()
}

async fn post(config: SimConfig, body: String) -> (StatusCode, String) {
    let app = router(config);
    let response = app
        .oneshot(
            Request::post("/fednow/messages")
                .header("content-type", "application/xml")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

fn parse_advice(xml: &str) -> pacs002::Document {
    let doc = pacs002::parse(xml).expect("advice must parse as pacs.002");
    let issues = validate_pacs002_direction(&doc, Pacs002Direction::ServiceToParticipant);
    assert!(
        issues.is_empty(),
        "advice must be direction-clean: {issues:#?}"
    );
    doc
}

#[tokio::test]
async fn default_scenario_settles_the_payment() {
    let (status, body) = post(SimConfig::default(), valid_pacs008(125_000)).await;
    assert_eq!(status, StatusCode::OK);

    let advice = parse_advice(&body);
    let tx = &advice
        .fi_to_fi_payment_status_report
        .transaction_information_and_status[0];
    assert_eq!(tx.transaction_status.as_deref(), Some("ACSC"));
    assert!(tx.acceptance_date_time.is_some());
    assert!(tx.effective_interbank_settlement_date.is_some());

    let orig = tx.original_group_information.as_ref().unwrap();
    assert_eq!(
        orig.original_message_identification,
        "20260702021040078SIMTEST001"
    );
    assert_eq!(
        tx.original_end_to_end_identification.as_deref(),
        Some("E2E-SIMTEST-0001")
    );
}

#[tokio::test]
async fn amount_ending_11_rejects_with_ac04() {
    let (status, body) = post(SimConfig::default(), valid_pacs008(125_011)).await;
    assert_eq!(status, StatusCode::OK);

    let advice = parse_advice(&body);
    let tx = &advice
        .fi_to_fi_payment_status_report
        .transaction_information_and_status[0];
    assert_eq!(tx.transaction_status.as_deref(), Some("RJCT"));
    assert_eq!(
        tx.status_reason_information[0]
            .reason
            .as_ref()
            .unwrap()
            .code
            .as_deref(),
        Some("AC04")
    );
}

#[tokio::test]
async fn amount_ending_22_accepts_without_posting() {
    let (status, body) = post(SimConfig::default(), valid_pacs008(125_022)).await;
    assert_eq!(status, StatusCode::OK);
    let advice = parse_advice(&body);
    let tx = &advice
        .fi_to_fi_payment_status_report
        .transaction_information_and_status[0];
    assert_eq!(tx.transaction_status.as_deref(), Some("ACWP"));
}

#[tokio::test]
async fn amount_ending_33_times_out_with_no_advice() {
    let (status, body) = post(SimConfig::default(), valid_pacs008(125_033)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(body.is_empty());
}

#[tokio::test]
async fn config_scenario_overrides_amount_trigger() {
    let config = SimConfig::from_toml(
        r#"
[scenarios]
"091000019" = { action = "reject", reason = "RR04" }
"#,
    )
    .unwrap();
    assert_eq!(
        config.scenarios["091000019"],
        Scenario::Reject("RR04".to_string())
    );

    // Amount says "settle", config says "reject" — config wins.
    let (status, body) = post(config, valid_pacs008(125_000)).await;
    assert_eq!(status, StatusCode::OK);
    let advice = parse_advice(&body);
    let tx = &advice
        .fi_to_fi_payment_status_report
        .transaction_information_and_status[0];
    assert_eq!(tx.transaction_status.as_deref(), Some("RJCT"));
    assert_eq!(
        tx.status_reason_information[0]
            .reason
            .as_ref()
            .unwrap()
            .code
            .as_deref(),
        Some("RR04")
    );
}

#[tokio::test]
async fn profile_invalid_message_is_rejected_with_simv() {
    // Missing CtgyPurp/accounts/settlement date -> FedNow-profile violations.
    let invalid = Pacs008Builder::new(
        fednow_message_id("20260702", "021040078", "SIMTEST002"),
        "2026-07-02T15:30:00Z",
        "E2E-SIMTEST-0002",
        5_000,
        "021040078",
        "091000019",
    )
    .to_xml()
    .unwrap();

    let (status, body) = post(SimConfig::default(), invalid).await;
    assert_eq!(status, StatusCode::OK);
    let advice = parse_advice(&body);
    let tx = &advice
        .fi_to_fi_payment_status_report
        .transaction_information_and_status[0];
    assert_eq!(tx.transaction_status.as_deref(), Some("RJCT"));
    let rsn = &tx.status_reason_information[0];
    assert_eq!(
        rsn.reason.as_ref().unwrap().proprietary.as_deref(),
        Some("SIMV")
    );
    assert!(
        rsn.additional_information[0].contains("fednow."),
        "violated rule codes travel in AddtlInf: {:?}",
        rsn.additional_information
    );
}

#[tokio::test]
async fn malformed_xml_is_a_400() {
    let (status, _) = post(SimConfig::default(), "<not-xml".to_string()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn healthz_answers() {
    let app = router(SimConfig::default());
    let response = app
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
