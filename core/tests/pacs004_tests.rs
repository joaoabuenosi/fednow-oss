//! Integration tests for pacs.004 (payment return) under the FedNow-shared
//! lexical rules. Full profile calibration is pending the PaymentReturn
//! usage-guideline export.

use fednow_core::validate::validate_pacs004;
use fednow_core::{pacs004, ParseError};

const VALID: &str = include_str!("fixtures/pacs004_valid.xml");

fn codes(xml: &str) -> Vec<&'static str> {
    let doc = pacs004::parse(xml).expect("fixture variant must stay parseable");
    validate_pacs004(&doc).into_iter().map(|i| i.code).collect()
}

#[test]
fn parses_payment_return_into_typed_model() {
    let doc = pacs004::parse(VALID).expect("valid fixture must parse");

    assert_eq!(doc.xmlns.as_deref(), Some(pacs004::NAMESPACE));

    let msg = &doc.payment_return;
    assert_eq!(
        msg.group_header.message_identification,
        "20260702091000019RETURN0001"
    );
    assert_eq!(msg.transaction_information.len(), 1);

    let tx = &msg.transaction_information[0];
    assert_eq!(
        tx.return_identification.as_deref(),
        Some("RTR-20260703-0001")
    );
    assert_eq!(tx.returned_interbank_settlement_amount.value, "1250.00");
    assert_eq!(
        tx.original_group_information
            .as_ref()
            .unwrap()
            .original_message_identification,
        "20260702021040078FIXTURE001"
    );
    assert_eq!(
        tx.return_reason_information[0]
            .reason
            .as_ref()
            .unwrap()
            .code
            .as_deref(),
        Some("AC04")
    );
}

#[test]
fn valid_fixture_has_no_validation_issues() {
    let doc = pacs004::parse(VALID).unwrap();
    let issues = validate_pacs004(&doc);
    assert!(
        issues.is_empty(),
        "expected clean validation, got: {issues:#?}"
    );
}

#[test]
fn malformed_xml_is_a_parse_error() {
    let err = pacs004::parse("<Document><PmtRtr></Document>").unwrap_err();
    assert!(matches!(err, ParseError::Xml(_)));
}

#[test]
fn wrong_namespace_is_flagged() {
    let xml = VALID.replace("pacs.004.001.10", "pacs.004.001.02");
    assert!(codes(&xml).contains(&"xsd.namespace"));
}

#[test]
fn non_fednow_msgid_is_flagged() {
    let xml = VALID.replace(
        "<MsgId>20260702091000019RETURN0001</MsgId>",
        "<MsgId>RETURN-0001</MsgId>",
    );
    assert!(codes(&xml).contains(&"fednow.msgid.format"));
}

#[test]
fn missing_clearing_system_is_flagged() {
    let xml = VALID.replace(
        "<ClrSys>\n          <Cd>FDN</Cd>\n        </ClrSys>\n      ",
        "",
    );
    assert!(codes(&xml).contains(&"fednow.clrsys.fdn"));
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
fn missing_return_reason_is_flagged() {
    let xml = VALID.replace(
        "<RtrRsnInf>\n        <Rsn>\n          <Cd>AC04</Cd>\n        </Rsn>\n        <AddtlInf>Funds returned after post-ACWP rejection</AddtlInf>\n      </RtrRsnInf>\n    ",
        "",
    );
    assert!(codes(&xml).contains(&"fednow.rtrrsn.required"));
}

#[test]
fn returned_amount_rules_apply() {
    let xml = VALID.replace(
        r#"<RtrdIntrBkSttlmAmt Ccy="USD">1250.00</RtrdIntrBkSttlmAmt>"#,
        r#"<RtrdIntrBkSttlmAmt Ccy="EUR">1250.001</RtrdIntrBkSttlmAmt>"#,
    );
    let found = codes(&xml);
    assert!(found.contains(&"fednow.ccy.usd"), "{found:?}");
    assert!(found.contains(&"fednow.amount.cents"), "{found:?}");
}

#[test]
fn bad_agent_checksum_is_flagged() {
    let xml = VALID.replace("<MmbId>021040078</MmbId>", "<MmbId>021040079</MmbId>");
    assert!(codes(&xml).contains(&"fednow.aba.checksum"));
}
