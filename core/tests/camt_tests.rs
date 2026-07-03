//! Integration tests for the return-request flow: camt.056 (ask for the money
//! back) and camt.029 (the answer), under the FedNow Release 1 profiles.

use fednow_core::validate::{validate_camt029, validate_camt056};
use fednow_core::{camt029, camt056};

const REQ: &str = include_str!("fixtures/camt056_valid.xml");
const RESP_RJCR: &str = include_str!("fixtures/camt029_valid_rjcr.xml");

fn codes_056(xml: &str) -> Vec<&'static str> {
    let doc = camt056::parse(xml).expect("must stay parseable");
    validate_camt056(&doc).into_iter().map(|i| i.code).collect()
}

fn codes_029(xml: &str) -> Vec<&'static str> {
    let doc = camt029::parse(xml).expect("must stay parseable");
    validate_camt029(&doc).into_iter().map(|i| i.code).collect()
}

#[test]
fn return_request_parses_and_validates_clean() {
    let doc = camt056::parse(REQ).unwrap();
    assert_eq!(doc.xmlns.as_deref(), Some(camt056::NAMESPACE));
    let msg = &doc.cancellation_request;
    assert_eq!(msg.assignment.identification, "20260703021040078RETREQ0001");
    assert_eq!(
        msg.case.as_ref().unwrap().identification,
        "CASE-20260703-0001"
    );
    let tx = &msg.underlying[0].transaction_information[0];
    assert_eq!(
        tx.cancellation_reason_information[0]
            .reason
            .as_ref()
            .unwrap()
            .code
            .as_deref(),
        Some("DUPL")
    );

    let issues = validate_camt056(&doc);
    assert!(issues.is_empty(), "expected clean, got: {issues:#?}");
}

#[test]
fn return_request_response_parses_and_validates_clean() {
    let doc = camt029::parse(RESP_RJCR).unwrap();
    assert_eq!(doc.xmlns.as_deref(), Some(camt029::NAMESPACE));
    assert_eq!(doc.resolution.status.confirmation.as_deref(), Some("RJCR"));

    let issues = validate_camt029(&doc);
    assert!(issues.is_empty(), "expected clean, got: {issues:#?}");
}

#[test]
fn missing_cancellation_reason_is_flagged() {
    let xml = REQ.replace(
        "<CxlRsnInf>\n          <Rsn>\n            <Cd>DUPL</Cd>\n          </Rsn>\n          <AddtlInf>Duplicate payment sent in error</AddtlInf>\n        </CxlRsnInf>\n      ",
        "",
    );
    assert!(codes_056(&xml).contains(&"fednow.cxlrsn.required"));
}

#[test]
fn proprietary_cancellation_reason_is_flagged() {
    let xml = REQ.replace("<Cd>DUPL</Cd>", "<Prtry>CUSTOM</Prtry>");
    assert!(codes_056(&xml).contains(&"fednow.cxlrsn.cd"));
}

#[test]
fn non_fednow_assignment_id_is_flagged() {
    let xml = REQ.replace(
        "<Id>20260703021040078RETREQ0001</Id>",
        "<Id>RETREQ-0001</Id>",
    );
    assert!(codes_056(&xml).contains(&"fednow.msgid.format"));
}

#[test]
fn missing_original_amount_is_flagged() {
    let xml = REQ.replace(
        "<OrgnlIntrBkSttlmAmt Ccy=\"USD\">1250.00</OrgnlIntrBkSttlmAmt>\n        ",
        "",
    );
    assert!(codes_056(&xml).contains(&"fednow.orgnlamt.required"));
}

#[test]
fn unknown_confirmation_is_flagged() {
    let xml = RESP_RJCR.replace("<Conf>RJCR</Conf>", "<Conf>XXXX</Conf>");
    assert!(codes_029(&xml).contains(&"fednow.conf.known"));
}

#[test]
fn accepted_confirmations_pass_without_reason() {
    for conf in ["IPAY", "PDCR", "PECR"] {
        let xml = RESP_RJCR
            .replace("<Conf>RJCR</Conf>", &format!("<Conf>{conf}</Conf>"))
            .replace(
                "<CxlStsRsnInf>\n          <Rsn>\n            <Cd>LEGL</Cd>\n          </Rsn>\n          <AddtlInf>Funds no longer available</AddtlInf>\n        </CxlStsRsnInf>\n      ",
                "",
            );
        let found = codes_029(&xml);
        assert!(found.is_empty(), "{conf} must be clean, got {found:?}");
    }
}

#[test]
fn rejected_response_without_reason_is_flagged() {
    let xml = RESP_RJCR.replace(
        "<CxlStsRsnInf>\n          <Rsn>\n            <Cd>LEGL</Cd>\n          </Rsn>\n          <AddtlInf>Funds no longer available</AddtlInf>\n        </CxlStsRsnInf>\n      ",
        "",
    );
    assert!(codes_029(&xml).contains(&"fednow.rjcr.reason"));
}

#[test]
fn bad_assignment_agent_checksum_is_flagged() {
    let xml = RESP_RJCR.replace("<MmbId>091000019</MmbId>", "<MmbId>091000018</MmbId>");
    assert!(codes_029(&xml).contains(&"fednow.aba.checksum"));
}
