//! Integration tests for pacs.008 parsing and validation (milestone M1).
//!
//! Invalid variants are derived from the valid fixture by targeted string edits,
//! so each test states exactly which rule it exercises.

use fednow_core::validate::{validate_pacs008, RuleSource};
use fednow_core::{pacs008, ParseError};

const VALID: &str = include_str!("fixtures/pacs008_valid.xml");

fn codes(xml: &str) -> Vec<&'static str> {
    let doc = pacs008::parse(xml).expect("fixture variant must stay parseable");
    validate_pacs008(&doc).into_iter().map(|i| i.code).collect()
}

#[test]
fn parses_valid_fixture_into_typed_model() {
    let doc = pacs008::parse(VALID).expect("valid fixture must parse");

    assert_eq!(doc.xmlns.as_deref(), Some(pacs008::NAMESPACE));

    let msg = &doc.fi_to_fi_customer_credit_transfer;
    assert_eq!(
        msg.group_header.message_identification,
        "20260702021040078FIXTURE001"
    );
    assert_eq!(
        msg.group_header.settlement_information.settlement_method,
        "CLRG"
    );
    assert_eq!(msg.credit_transfer_transaction_information.len(), 1);

    let tx = &msg.credit_transfer_transaction_information[0];
    assert_eq!(
        tx.payment_identification.end_to_end_identification,
        "E2E-20260702-0001"
    );
    assert_eq!(tx.interbank_settlement_amount.currency, "USD");
    assert_eq!(tx.interbank_settlement_amount.value, "1250.00");
    assert_eq!(tx.charge_bearer, "SLEV");

    let debtor_agent_member = tx
        .debtor_agent
        .financial_institution_identification
        .clearing_system_member_identification
        .as_ref()
        .expect("fixture carries a routing number");
    assert_eq!(debtor_agent_member.member_identification, "021040078");
}

#[test]
fn valid_fixture_has_no_validation_issues() {
    let doc = pacs008::parse(VALID).unwrap();
    let issues = validate_pacs008(&doc);
    assert!(
        issues.is_empty(),
        "expected clean validation, got: {issues:#?}"
    );
}

#[test]
fn malformed_xml_is_a_parse_error() {
    let err = pacs008::parse("<Document><FIToFICstmrCdtTrf></Document>").unwrap_err();
    assert!(matches!(err, ParseError::Xml(_)));
}

#[test]
fn missing_required_element_is_a_parse_error() {
    // Removing GrpHdr/MsgId breaks required cardinality -> structural error.
    let xml = VALID.replace("<MsgId>20260702021040078FIXTURE001</MsgId>", "");
    assert!(pacs008::parse(&xml).is_err());
}

#[test]
fn wrong_namespace_is_flagged() {
    let xml = VALID.replace("pacs.008.001.08", "pacs.008.001.02");
    assert!(codes(&xml).contains(&"xsd.namespace"));
}

#[test]
fn msgid_longer_than_35_chars_violates_max35text() {
    let xml = VALID.replace(
        "20260702021040078FIXTURE001",
        "20260702021040078FIXTURE001XXXXXXXXX", // 36 chars
    );
    assert!(codes(&xml).contains(&"xsd.msgid.length"));
}

#[test]
fn non_fednow_msgid_shape_violates_the_profile() {
    // Letter in the date part breaks CCYYMMDD + connection party + reference.
    let xml = VALID.replace(
        "<MsgId>20260702021040078FIXTURE001</MsgId>",
        "<MsgId>M20260702FIXTURE00000001</MsgId>",
    );
    assert!(codes(&xml).contains(&"fednow.msgid.format"));
}

#[test]
fn missing_clearing_system_fdn_is_flagged() {
    let xml = VALID.replace(
        "<ClrSys>\n          <Cd>FDN</Cd>\n        </ClrSys>\n      ",
        "",
    );
    assert!(codes(&xml).contains(&"fednow.clrsys.fdn"));
}

#[test]
fn missing_payment_type_information_is_flagged() {
    let xml = VALID.replace(
        "<PmtTpInf>\n        <LclInstrm>\n          <Prtry>FDNA</Prtry>\n        </LclInstrm>\n        <CtgyPurp>\n          <Prtry>CONS</Prtry>\n        </CtgyPurp>\n      </PmtTpInf>\n      ",
        "",
    );
    assert!(codes(&xml).contains(&"fednow.pmttpinf.required"));
}

#[test]
fn payment_type_information_without_ctgypurp_is_flagged() {
    let xml = VALID.replace(
        "<CtgyPurp>\n          <Prtry>CONS</Prtry>\n        </CtgyPurp>\n      ",
        "",
    );
    assert!(codes(&xml).contains(&"fednow.pmttpinf.ctgypurp"));
}

#[test]
fn missing_instructing_agent_is_flagged() {
    let xml = VALID.replace(
        "<InstgAgt>\n        <FinInstnId>\n          <ClrSysMmbId>\n            <ClrSysId>\n              <Cd>USABA</Cd>\n            </ClrSysId>\n            <MmbId>021040078</MmbId>\n          </ClrSysMmbId>\n        </FinInstnId>\n      </InstgAgt>\n      ",
        "",
    );
    assert!(codes(&xml).contains(&"fednow.instgagt.required"));
}

#[test]
fn agent_without_usaba_scheme_is_flagged() {
    let xml = VALID.replace(
        "<ClrSysId>\n              <Cd>USABA</Cd>\n            </ClrSysId>\n            <MmbId>091000019</MmbId>",
        "<MmbId>091000019</MmbId>",
    );
    assert!(codes(&xml).contains(&"fednow.agent.usaba"));
}

#[test]
fn missing_settlement_date_is_flagged() {
    let xml = VALID.replace("<IntrBkSttlmDt>2026-07-02</IntrBkSttlmDt>\n      ", "");
    assert!(codes(&xml).contains(&"fednow.intrbksttlmdt.required"));
}

#[test]
fn missing_creditor_account_is_flagged() {
    let xml = VALID.replace(
        "<CdtrAcct>\n        <Id>\n          <Othr>\n            <Id>987654321000</Id>\n          </Othr>\n        </Id>\n      </CdtrAcct>\n    ",
        "",
    );
    assert!(codes(&xml).contains(&"fednow.cdtracct.required"));
}

#[test]
fn amount_above_14_total_digits_violates_fednow_profile() {
    let xml = VALID.replace(
        ">1250.00</IntrBkSttlmAmt>",
        ">1234567890123.45</IntrBkSttlmAmt>", // 15 total digits
    );
    assert!(codes(&xml).contains(&"fednow.amount.digits"));
}

#[test]
fn nboftxs_must_match_transaction_count() {
    let xml = VALID.replace("<NbOfTxs>1</NbOfTxs>", "<NbOfTxs>2</NbOfTxs>");
    assert!(codes(&xml).contains(&"iso.nboftxs.mismatch"));
}

#[test]
fn non_usd_currency_violates_fednow_profile() {
    let xml = VALID.replace(r#"Ccy="USD""#, r#"Ccy="EUR""#);
    let doc = pacs008::parse(&xml).unwrap();
    let issues = validate_pacs008(&doc);
    let issue = issues
        .iter()
        .find(|i| i.code == "fednow.ccy.usd")
        .expect("must flag EUR");
    assert_eq!(issue.source, RuleSource::FedNowProfile);
}

#[test]
fn zero_amount_is_rejected() {
    let xml = VALID.replace(">1250.00</IntrBkSttlmAmt>", ">0.00</IntrBkSttlmAmt>");
    assert!(codes(&xml).contains(&"fednow.amount.positive"));
}

#[test]
fn sub_cent_precision_is_rejected_for_usd() {
    let xml = VALID.replace(">1250.00</IntrBkSttlmAmt>", ">1250.001</IntrBkSttlmAmt>");
    assert!(codes(&xml).contains(&"fednow.amount.cents"));
}

#[test]
fn non_numeric_amount_violates_xsd_facets() {
    let xml = VALID.replace(">1250.00</IntrBkSttlmAmt>", ">12,50</IntrBkSttlmAmt>");
    assert!(codes(&xml).contains(&"xsd.amount.format"));
}

#[test]
fn bad_aba_check_digit_is_flagged() {
    // 021040079: last digit off by one -> checksum fails.
    let xml = VALID.replace("021040078", "021040079");
    assert!(codes(&xml).contains(&"fednow.aba.checksum"));
}

#[test]
fn non_nine_digit_routing_number_is_flagged() {
    let xml = VALID.replace("021040078", "12345");
    assert!(codes(&xml).contains(&"fednow.aba.format"));
}

#[test]
fn uppercase_uetr_violates_the_uuidv4_pattern() {
    let xml = VALID.replace(
        "8a562c67-ca16-48ba-b074-65581be6f001",
        "8A562C67-CA16-48BA-B074-65581BE6F001",
    );
    assert!(codes(&xml).contains(&"xsd.uetr.pattern"));
}

#[test]
fn non_slev_charge_bearer_violates_fednow_profile() {
    let xml = VALID.replace("<ChrgBr>SLEV</ChrgBr>", "<ChrgBr>SHAR</ChrgBr>");
    assert!(codes(&xml).contains(&"fednow.chrgbr.slev"));
}

#[test]
fn unknown_charge_bearer_violates_the_xsd_enum() {
    let xml = VALID.replace("<ChrgBr>SLEV</ChrgBr>", "<ChrgBr>FREE</ChrgBr>");
    assert!(codes(&xml).contains(&"xsd.chrgbr.enum"));
}

#[test]
fn settlement_method_other_than_clrg_is_flagged() {
    let xml = VALID.replace("<SttlmMtd>CLRG</SttlmMtd>", "<SttlmMtd>INDA</SttlmMtd>");
    assert!(codes(&xml).contains(&"fednow.sttlmmtd.clrg"));
}

#[test]
fn all_issues_are_collected_not_just_the_first() {
    let xml = VALID
        .replace(r#"Ccy="USD""#, r#"Ccy="EUR""#)
        .replace("021040078", "021040079")
        .replace("<ChrgBr>SLEV</ChrgBr>", "<ChrgBr>SHAR</ChrgBr>");
    let found = codes(&xml);
    for expected in [
        "fednow.ccy.usd",
        "fednow.aba.checksum",
        "fednow.chrgbr.slev",
    ] {
        assert!(found.contains(&expected), "missing {expected} in {found:?}");
    }
}
