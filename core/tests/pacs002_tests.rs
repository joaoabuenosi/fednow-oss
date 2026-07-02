//! Integration tests for pacs.002 parsing and validation.
//!
//! Two base fixtures: a settlement advice (ACSC) and a rejection (RJCT) with a
//! reason code. Invalid variants derive from them by targeted string edits.

use fednow_core::validate::{
    validate_pacs002, validate_pacs002_direction, Pacs002Direction, RuleSource,
};
use fednow_core::{pacs002, ParseError};

const VALID_ACSC: &str = include_str!("fixtures/pacs002_valid_acsc.xml");
const VALID_RJCT: &str = include_str!("fixtures/pacs002_valid_rjct.xml");

fn codes(xml: &str) -> Vec<&'static str> {
    let doc = pacs002::parse(xml).expect("fixture variant must stay parseable");
    validate_pacs002(&doc).into_iter().map(|i| i.code).collect()
}

#[test]
fn parses_settlement_advice_into_typed_model() {
    let doc = pacs002::parse(VALID_ACSC).expect("valid fixture must parse");

    assert_eq!(doc.xmlns.as_deref(), Some(pacs002::NAMESPACE));

    let msg = &doc.fi_to_fi_payment_status_report;
    assert_eq!(
        msg.group_header.message_identification,
        "FEDNOWSVCADVICE000000000000001"
    );
    // FedNow reports per transaction, not per group.
    assert!(msg.original_group_information_and_status.is_none());

    assert_eq!(msg.transaction_information_and_status.len(), 1);
    let tx = &msg.transaction_information_and_status[0];
    let orig = tx
        .original_group_information
        .as_ref()
        .expect("fixture references the original message per transaction");
    assert_eq!(
        orig.original_message_identification,
        "20260702021040078FIXTURE001"
    );
    assert_eq!(orig.original_message_name_identification, "pacs.008.001.08");
    assert!(orig.original_creation_date_time.is_some());

    assert_eq!(tx.transaction_status.as_deref(), Some("ACSC"));
    assert_eq!(
        tx.original_end_to_end_identification.as_deref(),
        Some("E2E-20260702-0001")
    );
    assert!(tx.acceptance_date_time.is_some());
    assert!(tx
        .effective_interbank_settlement_date
        .as_ref()
        .and_then(|d| d.date.as_deref())
        .is_some());
    assert!(tx.instructing_agent.is_some());
    assert!(tx.instructed_agent.is_some());
}

#[test]
fn service_advice_validates_clean_for_its_direction() {
    let doc = pacs002::parse(VALID_ACSC).unwrap();
    let issues = validate_pacs002_direction(&doc, Pacs002Direction::ServiceToParticipant);
    assert!(issues.is_empty(), "expected clean, got: {issues:#?}");
}

#[test]
fn participant_response_validates_clean_for_its_direction() {
    let doc = pacs002::parse(VALID_RJCT).unwrap();
    let issues = validate_pacs002_direction(&doc, Pacs002Direction::ParticipantToService);
    assert!(issues.is_empty(), "expected clean, got: {issues:#?}");
}

#[test]
fn service_advice_fails_the_participant_profile() {
    // The advice carries AccptncDtTm/FctvIntrBkSttlmDt and a non-FedNow MsgId —
    // all fine for the service, violations for a participant.
    let doc = pacs002::parse(VALID_ACSC).unwrap();
    let found: Vec<_> = validate_pacs002_direction(&doc, Pacs002Direction::ParticipantToService)
        .into_iter()
        .map(|i| i.code)
        .collect();
    for expected in [
        "fednow.msgid.format",
        "fednow.accptncdttm.absent",
        "fednow.fctvdt.absent",
    ] {
        assert!(found.contains(&expected), "missing {expected} in {found:?}");
    }
}

#[test]
fn missing_orgnlgrpinf_is_flagged_by_the_fednow_profile() {
    let xml = VALID_RJCT.replace(
        "<OrgnlGrpInf>\n        <OrgnlMsgId>20260702021040078FIXTURE001</OrgnlMsgId>\n        <OrgnlMsgNmId>pacs.008.001.08</OrgnlMsgNmId>\n        <OrgnlCreDtTm>2026-07-02T10:30:00-05:00</OrgnlCreDtTm>\n      </OrgnlGrpInf>\n      ",
        "",
    );
    let doc = pacs002::parse(&xml).unwrap();
    let found: Vec<_> = validate_pacs002_direction(&doc, Pacs002Direction::ParticipantToService)
        .into_iter()
        .map(|i| i.code)
        .collect();
    assert!(found.contains(&"fednow.orgnlgrpinf.required"), "{found:?}");
}

#[test]
fn missing_orgnlcredttm_is_flagged_by_the_fednow_profile() {
    let xml = VALID_RJCT.replace(
        "<OrgnlCreDtTm>2026-07-02T10:30:00-05:00</OrgnlCreDtTm>\n      ",
        "",
    );
    let doc = pacs002::parse(&xml).unwrap();
    let found: Vec<_> = validate_pacs002_direction(&doc, Pacs002Direction::ParticipantToService)
        .into_iter()
        .map(|i| i.code)
        .collect();
    assert!(found.contains(&"fednow.orgnlcredttm.required"), "{found:?}");
}

#[test]
fn proprietary_reason_is_rejected_for_participant_direction() {
    let xml = VALID_RJCT.replace("<Cd>AC04</Cd>", "<Prtry>CUSTOM</Prtry>");
    let doc = pacs002::parse(&xml).unwrap();
    let found: Vec<_> = validate_pacs002_direction(&doc, Pacs002Direction::ParticipantToService)
        .into_iter()
        .map(|i| i.code)
        .collect();
    assert!(found.contains(&"fednow.stsrsn.cd"), "{found:?}");
    // But the service direction allows proprietary reasons.
    let service: Vec<_> = validate_pacs002_direction(&doc, Pacs002Direction::ServiceToParticipant)
        .into_iter()
        .map(|i| i.code)
        .collect();
    assert!(!service.contains(&"fednow.stsrsn.cd"), "{service:?}");
}

#[test]
fn missing_instructing_agent_is_flagged_by_the_fednow_profile() {
    let xml = VALID_RJCT.replace(
        "<InstgAgt>\n        <FinInstnId>\n          <ClrSysMmbId>\n            <ClrSysId>\n              <Cd>USABA</Cd>\n            </ClrSysId>\n            <MmbId>091000019</MmbId>\n          </ClrSysMmbId>\n        </FinInstnId>\n      </InstgAgt>\n      ",
        "",
    );
    let doc = pacs002::parse(&xml).unwrap();
    let found: Vec<_> = validate_pacs002_direction(&doc, Pacs002Direction::ParticipantToService)
        .into_iter()
        .map(|i| i.code)
        .collect();
    assert!(found.contains(&"fednow.instgagt.required"), "{found:?}");
}

#[test]
fn parses_rejection_with_reason_code() {
    let doc = pacs002::parse(VALID_RJCT).unwrap();
    let tx = &doc
        .fi_to_fi_payment_status_report
        .transaction_information_and_status[0];
    assert_eq!(tx.transaction_status.as_deref(), Some("RJCT"));
    assert_eq!(tx.status_reason_information.len(), 1);
    assert!(tx.status_reason_information[0].has_reason());
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

#[test]
fn both_fixtures_validate_clean() {
    for xml in [VALID_ACSC, VALID_RJCT] {
        let doc = pacs002::parse(xml).unwrap();
        let issues = validate_pacs002(&doc);
        assert!(
            issues.is_empty(),
            "expected clean validation, got: {issues:#?}"
        );
    }
}

#[test]
fn malformed_xml_is_a_parse_error() {
    let err = pacs002::parse("<Document><FIToFIPmtStsRpt></Document>").unwrap_err();
    assert!(matches!(err, ParseError::Xml(_)));
}

#[test]
fn wrong_namespace_is_flagged() {
    let xml = VALID_ACSC.replace("pacs.002.001.10", "pacs.002.001.03");
    assert!(codes(&xml).contains(&"xsd.namespace"));
}

#[test]
fn missing_txsts_violates_fednow_profile() {
    let xml = VALID_ACSC.replace("<TxSts>ACSC</TxSts>", "");
    let doc = pacs002::parse(&xml).unwrap();
    let issues = validate_pacs002(&doc);
    let issue = issues
        .iter()
        .find(|i| i.code == "fednow.txsts.required")
        .expect("must flag missing TxSts");
    assert_eq!(issue.source, RuleSource::FedNowProfile);
}

#[test]
fn overlong_status_code_violates_xsd_facet() {
    let xml = VALID_ACSC.replace("<TxSts>ACSC</TxSts>", "<TxSts>ACCEPTED</TxSts>");
    assert!(codes(&xml).contains(&"xsd.txsts.length"));
}

#[test]
fn status_outside_the_fednow_set_is_flagged() {
    // AB01 is a valid external code but not part of the FedNow credit-transfer set.
    let xml = VALID_ACSC.replace("<TxSts>ACSC</TxSts>", "<TxSts>AB01</TxSts>");
    assert!(codes(&xml).contains(&"fednow.txsts.known"));
}

#[test]
fn pending_and_blocked_statuses_are_accepted() {
    for status in ["PDNG", "BLCK", "ACCC"] {
        let xml = VALID_ACSC.replace("<TxSts>ACSC</TxSts>", &format!("<TxSts>{status}</TxSts>"));
        assert!(
            !codes(&xml).contains(&"fednow.txsts.known"),
            "{status} must be in the FedNow set"
        );
    }
}

#[test]
fn participant_must_not_send_acsc() {
    let xml = VALID_RJCT.replace("<TxSts>RJCT</TxSts>", "<TxSts>ACSC</TxSts>");
    let doc = pacs002::parse(&xml).unwrap();
    let found: Vec<_> = validate_pacs002_direction(&doc, Pacs002Direction::ParticipantToService)
        .into_iter()
        .map(|i| i.code)
        .collect();
    assert!(found.contains(&"fednow.txsts.participant"), "{found:?}");
    // The service advice direction allows it (the ACSC fixture is exactly that).
    let service: Vec<_> = validate_pacs002_direction(&doc, Pacs002Direction::ServiceToParticipant)
        .into_iter()
        .map(|i| i.code)
        .collect();
    assert!(
        !service.contains(&"fednow.txsts.participant"),
        "{service:?}"
    );
}

#[test]
fn rejection_without_reason_code_is_flagged() {
    let xml = VALID_RJCT.replace(
        "<StsRsnInf>\n        <Rsn>\n          <Cd>AC04</Cd>\n        </Rsn>\n        <AddtlInf>Creditor account closed</AddtlInf>\n      </StsRsnInf>",
        "",
    );
    let doc = pacs002::parse(&xml).unwrap();
    let tx = &doc
        .fi_to_fi_payment_status_report
        .transaction_information_and_status[0];
    assert!(
        tx.status_reason_information.is_empty(),
        "edit must remove the whole StsRsnInf block"
    );
    let found: Vec<_> = validate_pacs002(&doc).into_iter().map(|i| i.code).collect();
    assert!(found.contains(&"fednow.rjct.reason"), "got {found:?}");
}

#[test]
fn reason_block_without_a_code_still_counts_as_missing_reason() {
    let xml = VALID_RJCT.replace(
        "<Rsn>\n          <Cd>AC04</Cd>\n        </Rsn>",
        "<Rsn></Rsn>",
    );
    assert!(codes(&xml).contains(&"fednow.rjct.reason"));
}

#[test]
fn overlong_reason_code_violates_xsd_facet() {
    let xml = VALID_RJCT.replace("<Cd>AC04</Cd>", "<Cd>AC004X</Cd>");
    assert!(codes(&xml).contains(&"xsd.stsrsn.length"));
}

#[test]
fn bad_original_uetr_is_flagged() {
    let xml = VALID_ACSC.replace(
        "8a562c67-ca16-48ba-b074-65581be6f001",
        "8a562c67-ca16-18ba-b074-65581be6f001", // version nibble is not 4
    );
    assert!(codes(&xml).contains(&"xsd.uetr.pattern"));
}

#[test]
fn bad_acceptance_datetime_is_flagged() {
    let xml = VALID_ACSC.replace(
        "<AccptncDtTm>2026-07-02T10:30:04-05:00</AccptncDtTm>",
        "<AccptncDtTm>02/07/2026 10:30</AccptncDtTm>",
    );
    assert!(codes(&xml).contains(&"xsd.accptncdttm.format"));
}

#[test]
fn status_report_without_transactions_parses() {
    // OrgnlGrpInfAndSts-only reports (group-level status) are schema-valid.
    let doc = pacs002::parse(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pacs.002.001.10">
  <FIToFIPmtStsRpt>
    <GrpHdr>
      <MsgId>M20260702STATUSFIXTURE00000003</MsgId>
      <CreDtTm>2026-07-02T10:32:00-05:00</CreDtTm>
    </GrpHdr>
    <OrgnlGrpInfAndSts>
      <OrgnlMsgId>M20260702FIXTURE00000000000001</OrgnlMsgId>
      <OrgnlMsgNmId>pacs.008.001.08</OrgnlMsgNmId>
      <GrpSts>RJCT</GrpSts>
    </OrgnlGrpInfAndSts>
  </FIToFIPmtStsRpt>
</Document>"#,
    )
    .expect("group-level status report must parse");
    assert!(doc
        .fi_to_fi_payment_status_report
        .transaction_information_and_status
        .is_empty());
}
