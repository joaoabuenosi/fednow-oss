//! Round-trip tests for the pacs.008 builder: build → serialize → parse with our
//! own parser → validate with the full rule set. A message the builder emits must
//! come back clean.

use fednow_core::builder::{fednow_message_id, Pacs008Builder};
use fednow_core::pacs008;
use fednow_core::validate::validate_pacs008;

fn full_builder() -> Pacs008Builder {
    Pacs008Builder::new(
        fednow_message_id("20260702", "021040078", "BUILT0001"),
        "2026-07-02T15:30:00Z",
        "E2E-20260702-BUILT-0001",
        125_000, // $1,250.00
        "021040078",
        "091000019",
    )
    .instruction_identification("INSTR-BUILT-0001")
    .uetr("8a562c67-ca16-48ba-b074-65581be6f001")
    .interbank_settlement_date("2026-07-02")
    .local_instrument("EXAMPLE")
    .category_purpose("EXAMPLE")
    .debtor_name("Jane Example Debtor")
    .debtor_account("123456789012")
    .creditor_name("John Example Creditor")
    .creditor_account("987654321000")
}

#[test]
fn built_document_round_trips_clean_through_parse_and_validate() {
    let xml = full_builder().to_xml().expect("serialization must succeed");

    let doc = pacs008::parse(&xml).expect("built XML must parse with our own parser");
    let issues = validate_pacs008(&doc);
    assert!(
        issues.is_empty(),
        "built message must pass all validation rules, got: {issues:#?}"
    );
}

#[test]
fn built_document_carries_the_fednow_profile_constants() {
    let xml = full_builder().to_xml().unwrap();

    assert!(xml.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
    assert!(xml.contains(r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pacs.008.001.08">"#));
    assert!(xml.contains("<NbOfTxs>1</NbOfTxs>"));
    assert!(xml.contains("<SttlmMtd>CLRG</SttlmMtd>"));
    assert!(xml.contains("<ClrSys><Cd>FDN</Cd></ClrSys>"));
    assert!(xml.contains("<ChrgBr>SLEV</ChrgBr>"));
    assert!(xml.contains(r#"<IntrBkSttlmAmt Ccy="USD">1250.00</IntrBkSttlmAmt>"#));
    assert!(xml.contains("<Cd>USABA</Cd>"));
    // Instructing/instructed agents default to debtor/creditor agents.
    assert!(xml.contains("<InstgAgt>"));
    assert!(xml.contains("<InstdAgt>"));
}

#[test]
fn amounts_format_from_cents_without_floating_point() {
    let cases = [
        (1u64, "0.01"),
        (10, "0.10"),
        (100, "1.00"),
        (99, "0.99"),
        (123_456, "1234.56"),
        (10_000_000_000, "100000000.00"), // $100M in cents — still exact
    ];
    for (cents, lexical) in cases {
        let xml = Pacs008Builder::new(
            "M1",
            "2026-07-02T15:30:00Z",
            "E2E-1",
            cents,
            "021040078",
            "091000019",
        )
        .to_xml()
        .unwrap();
        assert!(
            xml.contains(&format!(r#"Ccy="USD">{lexical}</IntrBkSttlmAmt>"#)),
            "cents {cents} must serialize as {lexical}"
        );
    }
}

#[test]
fn minimal_builder_omits_unset_elements_and_reports_missing_profile_fields() {
    let xml = Pacs008Builder::new(
        fednow_message_id("20260702", "021040078", "BUILT0002"),
        "2026-07-02T15:30:00Z",
        "E2E-20260702-BUILT-0002",
        5_000, // $50.00
        "021040078",
        "091000019",
    )
    .to_xml()
    .unwrap();

    for absent in [
        "<InstrId>",
        "<UETR>",
        "<IntrBkSttlmDt>",
        "<PmtTpInf>",
        "<DbtrAcct>",
        "<CdtrAcct>",
        "<Nm>",
    ] {
        assert!(!xml.contains(absent), "{absent} must be omitted when unset");
    }

    // The validator names exactly what the FedNow profile still needs.
    let doc = pacs008::parse(&xml).unwrap();
    let found: Vec<_> = validate_pacs008(&doc).into_iter().map(|i| i.code).collect();
    for expected in [
        "fednow.pmttpinf.required",
        "fednow.intrbksttlmdt.required",
        "fednow.dbtracct.required",
        "fednow.cdtracct.required",
    ] {
        assert!(found.contains(&expected), "missing {expected} in {found:?}");
    }
}

#[test]
fn built_fields_survive_the_round_trip() {
    let xml = full_builder().to_xml().unwrap();
    let doc = pacs008::parse(&xml).unwrap();
    let msg = &doc.fi_to_fi_customer_credit_transfer;

    assert_eq!(
        msg.group_header.message_identification,
        "20260702021040078BUILT0001"
    );
    let tx = &msg.credit_transfer_transaction_information[0];
    assert_eq!(
        tx.payment_identification.end_to_end_identification,
        "E2E-20260702-BUILT-0001"
    );
    assert_eq!(
        tx.payment_identification.uetr.as_deref(),
        Some("8a562c67-ca16-48ba-b074-65581be6f001")
    );
    assert_eq!(tx.interbank_settlement_amount.value, "1250.00");
    assert_eq!(tx.debtor.name.as_deref(), Some("Jane Example Debtor"));
    assert_eq!(
        tx.creditor_account
            .as_ref()
            .and_then(|a| a.identification.other.as_ref())
            .map(|o| o.identification.as_str()),
        Some("987654321000")
    );
}
