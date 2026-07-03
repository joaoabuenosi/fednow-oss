//! Round-trip tests for the pacs.028 builder.

use fednow_core::builder::{fednow_message_id, Pacs028Builder};
use fednow_core::pacs028;
use fednow_core::validate::validate_pacs028;

#[test]
fn built_status_request_round_trips_clean() {
    let xml = Pacs028Builder::new(
        fednow_message_id("20260702", "021040078", "QUERY001"),
        "2026-07-02T15:35:00Z",
        "20260702021040078BUILT0001",
        "2026-07-02T15:30:00Z",
        "021040078",
        "021150706",
    )
    .original_end_to_end_identification("E2E-20260702-BUILT-0001")
    .original_uetr("8a562c67-ca16-48ba-b074-65581be6f001")
    .to_xml()
    .expect("serialization");

    let doc = pacs028::parse(&xml).expect("built XML must parse");
    let issues = validate_pacs028(&doc);
    assert!(issues.is_empty(), "expected clean, got: {issues:#?}");

    let tx = &doc.fi_to_fi_payment_status_request.transaction_information[0];
    assert_eq!(
        tx.original_group_information
            .as_ref()
            .unwrap()
            .original_message_identification,
        "20260702021040078BUILT0001"
    );
    assert_eq!(
        tx.original_uetr.as_deref(),
        Some("8a562c67-ca16-48ba-b074-65581be6f001")
    );
}

#[test]
fn unset_optionals_are_omitted() {
    let xml = Pacs028Builder::new(
        fednow_message_id("20260702", "021040078", "QUERY002"),
        "2026-07-02T15:35:00Z",
        "20260702021040078BUILT0002",
        "2026-07-02T15:30:00Z",
        "021040078",
        "021150706",
    )
    .to_xml()
    .unwrap();
    for absent in ["<OrgnlInstrId>", "<OrgnlEndToEndId>", "<OrgnlUETR>"] {
        assert!(!xml.contains(absent), "{absent} must be omitted when unset");
    }
}
