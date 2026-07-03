//! End-to-end tests for the fednow-sim MQ mode: fire-and-forget sends of
//! `FedNowIncoming` envelopes, advices delivered asynchronously as
//! `FedNowOutgoing` envelopes on the participant's receive queue.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use fednow_core::builder::{fednow_message_id, Head001Builder, Pacs008Builder};
use fednow_core::envelope::{self, Direction, EnvelopedDocument};
use fednow_core::validate_envelope;
use fednow_sim::{router, SimConfig};
use http_body_util::BodyExt;
use tower::ServiceExt;

const PARTICIPANT: &str = "021040078";

fn valid_pacs008(amount_cents: u64, reference: &str) -> String {
    Pacs008Builder::new(
        fednow_message_id("20260702", PARTICIPANT, reference),
        "2026-07-02T15:30:00Z",
        "E2E-MQTEST-0001",
        amount_cents,
        PARTICIPANT,
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

fn strip_decl(xml: &str) -> &str {
    xml.trim_start()
        .split_once("?>")
        .map(|(_, tail)| tail.trim_start())
        .unwrap_or(xml)
}

/// Wrap a participant-sent Document in a FedNowIncoming envelope.
fn incoming(wrapper: &str, msg_def_idr: &str, biz_msg_idr: &str, document_xml: &str) -> String {
    let bah = Head001Builder::new(
        PARTICIPANT,
        "021150706",
        biz_msg_idr,
        msg_def_idr,
        "2026-07-02T15:30:00Z",
    )
    .to_xml()
    .unwrap();
    envelope::build(
        Direction::Incoming,
        wrapper,
        &bah,
        strip_decl(document_xml),
        None,
    )
}

fn incoming_pacs008(amount_cents: u64, reference: &str) -> String {
    let doc = valid_pacs008(amount_cents, reference);
    incoming(
        "FedNowCustomerCreditTransfer",
        "pacs.008.001.08",
        &fednow_message_id("20260702", PARTICIPANT, reference),
        &doc,
    )
}

fn incoming_pacs028(orig_msg_id: &str) -> String {
    let doc = format!(
        r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pacs.028.001.03">
  <FIToFIPmtStsReq>
    <GrpHdr>
      <MsgId>20260702021040078MQQUERY01</MsgId>
      <CreDtTm>2026-07-02T15:35:00Z</CreDtTm>
    </GrpHdr>
    <TxInf>
      <OrgnlGrpInf>
        <OrgnlMsgId>{orig_msg_id}</OrgnlMsgId>
        <OrgnlMsgNmId>pacs.008.001.08</OrgnlMsgNmId>
        <OrgnlCreDtTm>2026-07-02T15:30:00Z</OrgnlCreDtTm>
      </OrgnlGrpInf>
      <InstgAgt>
        <FinInstnId>
          <ClrSysMmbId>
            <ClrSysId><Cd>USABA</Cd></ClrSysId>
            <MmbId>021040078</MmbId>
          </ClrSysMmbId>
        </FinInstnId>
      </InstgAgt>
      <InstdAgt>
        <FinInstnId>
          <ClrSysMmbId>
            <ClrSysId><Cd>USABA</Cd></ClrSysId>
            <MmbId>021150706</MmbId>
          </ClrSysMmbId>
        </FinInstnId>
      </InstdAgt>
    </TxInf>
  </FIToFIPmtStsReq>
</Document>"#
    );
    incoming(
        "FedNowPaymentStatusRequest",
        "pacs.028.001.03",
        "20260702021040078MQQUERY01",
        &doc,
    )
}

async fn send(app: &axum::Router, body: String) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/mq/participants/{PARTICIPANT}/send"))
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

async fn receive(app: &axum::Router) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/mq/participants/{PARTICIPANT}/receive"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

/// Parse a received FedNowOutgoing envelope and return the advice's TxSts,
/// asserting the envelope is profile-clean.
fn advice_status(envelope_xml: &str) -> String {
    let env = envelope::parse(envelope_xml).expect("received message must be an envelope");
    assert_eq!(env.direction, Direction::Outgoing);
    assert_eq!(env.wrapper, "FedNowPaymentStatus");
    let issues = validate_envelope(&env);
    assert!(issues.is_empty(), "envelope must be clean: {issues:#?}");
    match &env.document {
        EnvelopedDocument::PaymentStatus(doc) => doc
            .fi_to_fi_payment_status_report
            .transaction_information_and_status[0]
            .transaction_status
            .clone()
            .expect("advice carries TxSts"),
        other => panic!("expected a pacs.002 advice, got {other:?}"),
    }
}

#[tokio::test]
async fn send_is_fire_and_forget_and_advice_arrives_on_the_queue() {
    let app = router(SimConfig::default());

    let (status, body) = send(&app, incoming_pacs008(125_000, "MQSETTLE01")).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(body.is_empty(), "MQ send returns no advice in the response");

    let (status, body) = receive(&app).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(advice_status(&body), "ACSC");

    // Queue drained: nothing else waiting.
    let (status, _) = receive(&app).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn amount_trigger_rejection_arrives_asynchronously() {
    let app = router(SimConfig::default());
    let (status, _) = send(&app, incoming_pacs008(100_011, "MQREJECT01")).await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (status, body) = receive(&app).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(advice_status(&body), "RJCT");
}

#[tokio::test]
async fn timeout_leaves_the_queue_empty_until_a_pacs028_asks() {
    let app = router(SimConfig::default());
    let orig_msg_id = fednow_message_id("20260702", PARTICIPANT, "MQTIMEOUT1");

    let (status, _) = send(&app, incoming_pacs008(100_033, "MQTIMEOUT1")).await;
    assert_eq!(status, StatusCode::ACCEPTED);

    // No advice: this is the reconciler's case.
    let (status, _) = receive(&app).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // A status request replays the withheld truth onto the queue.
    let (status, _) = send(&app, incoming_pacs028(&orig_msg_id)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (status, body) = receive(&app).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(advice_status(&body), "ACSC");
}

#[tokio::test]
async fn acwp_follow_up_is_pushed_without_a_pacs028() {
    let app = router(SimConfig::default());
    let (status, _) = send(&app, incoming_pacs008(100_066, "MQACWP0001")).await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (status, body) = receive(&app).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(advice_status(&body), "ACWP");

    // The receiving participant's ACCC is pushed shortly after — the HTTP dev
    // mode needs a pacs.028 for this; MQ mode does not.
    let mut last = (StatusCode::NO_CONTENT, String::new());
    for _ in 0..20 {
        last = receive(&app).await;
        if last.0 == StatusCode::OK {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(last.0, StatusCode::OK, "follow-up advice never arrived");
    assert_eq!(advice_status(&last.1), "ACCC");
}

#[tokio::test]
async fn profile_invalid_message_is_rejected_asynchronously_with_simv() {
    // Missing CtgyPurp/accounts/settlement date -> FedNow-profile violations;
    // in MQ mode even that rejection arrives asynchronously.
    let invalid = Pacs008Builder::new(
        fednow_message_id("20260702", PARTICIPANT, "MQINVALID1"),
        "2026-07-02T15:30:00Z",
        "E2E-MQTEST-0002",
        5_000,
        PARTICIPANT,
        "091000019",
    )
    .to_xml()
    .unwrap();
    let envelope_xml = incoming(
        "FedNowCustomerCreditTransfer",
        "pacs.008.001.08",
        &fednow_message_id("20260702", PARTICIPANT, "MQINVALID1"),
        &invalid,
    );

    let app = router(SimConfig::default());
    let (status, _) = send(&app, envelope_xml).await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (status, body) = receive(&app).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(advice_status(&body), "RJCT");
    assert!(body.contains("SIMV"), "proprietary SIMV reason expected");
}

#[tokio::test]
async fn bare_document_without_envelope_is_a_400() {
    let app = router(SimConfig::default());
    let (status, body) = send(&app, valid_pacs008(125_000, "MQBARE0001")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.contains("envelope"),
        "explains the envelope requirement: {body}"
    );
}
