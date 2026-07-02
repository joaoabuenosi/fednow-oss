//! Integration tests for pacs.028 (payment status request) under the FedNow
//! profile — the reconciler's message: query the status of an unresolved
//! payment instead of blindly resending it.

use fednow_core::validate::validate_pacs028;
use fednow_core::{pacs028, ParseError};

const VALID: &str = include_str!("fixtures/pacs028_valid.xml");

fn codes(xml: &str) -> Vec<&'static str> {
    let doc = pacs028::parse(xml).expect("fixture variant must stay parseable");
    validate_pacs028(&doc).into_iter().map(|i| i.code).collect()
}

#[test]
fn parses_status_request_into_typed_model() {
    let doc = pacs028::parse(VALID).expect("valid fixture must parse");

    assert_eq!(doc.xmlns.as_deref(), Some(pacs028::NAMESPACE));

    let msg = &doc.fi_to_fi_payment_status_request;
    assert_eq!(
        msg.group_header.message_identification,
        "20260702021040078STATUSREQ01"
    );
    assert_eq!(msg.transaction_information.len(), 1);

    let tx = &msg.transaction_information[0];
    let orig = tx
        .original_group_information
        .as_ref()
        .expect("request identifies the original message");
    assert_eq!(
        orig.original_message_identification,
        "20260702021040078FIXTURE001"
    );
    assert_eq!(orig.original_message_name_identification, "pacs.008.001.08");
    assert!(tx.instructing_agent.is_some());
    assert!(tx.instructed_agent.is_some());
}

#[test]
fn valid_fixture_has_no_validation_issues() {
    let doc = pacs028::parse(VALID).unwrap();
    let issues = validate_pacs028(&doc);
    assert!(
        issues.is_empty(),
        "expected clean validation, got: {issues:#?}"
    );
}

#[test]
fn malformed_xml_is_a_parse_error() {
    let err = pacs028::parse("<Document><FIToFIPmtStsReq></Document>").unwrap_err();
    assert!(matches!(err, ParseError::Xml(_)));
}

#[test]
fn wrong_namespace_is_flagged() {
    let xml = VALID.replace(
        "urn:iso:std:iso:20022:tech:xsd:pacs.028.001.03",
        "urn:iso:std:iso:20022:tech:xsd:pacs.028.001.01",
    );
    assert!(codes(&xml).contains(&"xsd.namespace"));
}

#[test]
fn non_fednow_msgid_is_flagged() {
    let xml = VALID.replace(
        "<MsgId>20260702021040078STATUSREQ01</MsgId>",
        "<MsgId>STATUSREQ-0001</MsgId>",
    );
    assert!(codes(&xml).contains(&"fednow.msgid.format"));
}

#[test]
fn missing_orgnlgrpinf_is_flagged() {
    let xml = VALID.replace(
        "<OrgnlGrpInf>\n        <OrgnlMsgId>20260702021040078FIXTURE001</OrgnlMsgId>\n        <OrgnlMsgNmId>pacs.008.001.08</OrgnlMsgNmId>\n        <OrgnlCreDtTm>2026-07-02T10:30:00-05:00</OrgnlCreDtTm>\n      </OrgnlGrpInf>\n      ",
        "",
    );
    assert!(codes(&xml).contains(&"fednow.orgnlgrpinf.required"));
}

#[test]
fn non_frs_original_message_name_is_flagged() {
    let xml = VALID.replace(
        "<OrgnlMsgNmId>pacs.008.001.08</OrgnlMsgNmId>",
        "<OrgnlMsgNmId>pacs.008.002.08</OrgnlMsgNmId>",
    );
    assert!(codes(&xml).contains(&"fednow.orgnlmsgnmid.format"));
}

#[test]
fn missing_instructed_agent_is_flagged() {
    let xml = VALID.replace(
        "<InstdAgt>\n        <FinInstnId>\n          <ClrSysMmbId>\n            <ClrSysId>\n              <Cd>USABA</Cd>\n            </ClrSysId>\n            <MmbId>091000019</MmbId>\n          </ClrSysMmbId>\n        </FinInstnId>\n      </InstdAgt>\n    ",
        "",
    );
    assert!(codes(&xml).contains(&"fednow.instdagt.required"));
}

#[test]
fn bad_routing_checksum_in_agent_is_flagged() {
    let xml = VALID.replace("<MmbId>021040078</MmbId>", "<MmbId>021040079</MmbId>");
    assert!(codes(&xml).contains(&"fednow.aba.checksum"));
}
