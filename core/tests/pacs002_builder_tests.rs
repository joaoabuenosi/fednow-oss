//! Round-trip tests for the pacs.002 builder: build → serialize → parse with our
//! own parser → validate against the intended FedNow direction.

use fednow_core::builder::{fednow_message_id, Pacs002Builder};
use fednow_core::pacs002;
use fednow_core::validate::{validate_pacs002_direction, Pacs002Direction};

/// The service advice fednow-sim answers with: settled (ACSC).
fn service_advice() -> Pacs002Builder {
    Pacs002Builder::new(
        "FEDNOWSIMADVICE000000000000001",
        "2026-07-02T15:30:05Z",
        "20260702021040078BUILT0001",
        "2026-07-02T15:30:00Z",
        "ACSC",
        "021040078",
        "091000019",
    )
    .original_end_to_end_identification("E2E-20260702-BUILT-0001")
    .original_uetr("8a562c67-ca16-48ba-b074-65581be6f001")
    .acceptance_date_time("2026-07-02T15:30:04Z")
    .effective_interbank_settlement_date("2026-07-02")
}

/// The accept/reject response a participant sends.
fn participant_reject() -> Pacs002Builder {
    Pacs002Builder::new(
        fednow_message_id("20260702", "091000019", "REJECT001"),
        "2026-07-02T15:30:03Z",
        "20260702021040078BUILT0001",
        "2026-07-02T15:30:00Z",
        "RJCT",
        "091000019",
        "021040078",
    )
    .original_end_to_end_identification("E2E-20260702-BUILT-0001")
    .reason_code("AC04")
    .additional_information("Creditor account closed")
}

#[test]
fn built_service_advice_round_trips_clean() {
    let xml = service_advice().to_xml().expect("serialization");
    let doc = pacs002::parse(&xml).expect("built XML must parse");
    let issues = validate_pacs002_direction(&doc, Pacs002Direction::ServiceToParticipant);
    assert!(issues.is_empty(), "expected clean, got: {issues:#?}");
}

#[test]
fn built_participant_reject_round_trips_clean() {
    let xml = participant_reject().to_xml().expect("serialization");
    let doc = pacs002::parse(&xml).expect("built XML must parse");
    let issues = validate_pacs002_direction(&doc, Pacs002Direction::ParticipantToService);
    assert!(issues.is_empty(), "expected clean, got: {issues:#?}");
}

#[test]
fn built_fields_survive_the_round_trip() {
    let xml = service_advice().to_xml().unwrap();
    let doc = pacs002::parse(&xml).unwrap();
    let tx = &doc
        .fi_to_fi_payment_status_report
        .transaction_information_and_status[0];

    assert_eq!(tx.transaction_status.as_deref(), Some("ACSC"));
    let orig = tx.original_group_information.as_ref().unwrap();
    assert_eq!(
        orig.original_message_identification,
        "20260702021040078BUILT0001"
    );
    assert_eq!(orig.original_message_name_identification, "pacs.008.001.08");
    assert_eq!(
        tx.effective_interbank_settlement_date
            .as_ref()
            .and_then(|d| d.date.as_deref()),
        Some("2026-07-02")
    );
    assert_eq!(
        tx.original_uetr.as_deref(),
        Some("8a562c67-ca16-48ba-b074-65581be6f001")
    );
}

#[test]
fn reject_without_reason_is_diagnosed_by_the_validator() {
    // The builder does not invent a reason; the validator names the gap.
    let xml = Pacs002Builder::new(
        fednow_message_id("20260702", "091000019", "REJECT002"),
        "2026-07-02T15:30:03Z",
        "20260702021040078BUILT0001",
        "2026-07-02T15:30:00Z",
        "RJCT",
        "091000019",
        "021040078",
    )
    .to_xml()
    .unwrap();
    let doc = pacs002::parse(&xml).unwrap();
    let found: Vec<_> = validate_pacs002_direction(&doc, Pacs002Direction::ParticipantToService)
        .into_iter()
        .map(|i| i.code)
        .collect();
    assert!(found.contains(&"fednow.rjct.reason"), "{found:?}");
}

#[test]
fn unset_optionals_are_omitted_from_the_wire() {
    let xml = Pacs002Builder::new(
        "FEDNOWSIMADVICE000000000000002",
        "2026-07-02T15:30:05Z",
        "20260702021040078BUILT0001",
        "2026-07-02T15:30:00Z",
        "ACWP",
        "021040078",
        "091000019",
    )
    .to_xml()
    .unwrap();
    for absent in [
        "<StsRsnInf>",
        "<AccptncDtTm>",
        "<FctvIntrBkSttlmDt>",
        "<OrgnlUETR>",
        "<OrgnlGrpInfAndSts>",
    ] {
        assert!(!xml.contains(absent), "{absent} must be omitted when unset");
    }
}
