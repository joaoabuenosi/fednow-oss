//! Integration tests for head.001.001.02 (Business Application Header).
//!
//! Two base fixtures: an unsigned header and one carrying an XMLDSig skeleton in
//! Sgntr (placeholder digest/signature values — structure only). Invalid variants
//! derive from them by targeted string edits.

use fednow_core::validate::{validate_head001, RuleSource};
use fednow_core::{head001, ParseError};

const VALID: &str = include_str!("fixtures/head001_valid.xml");
const VALID_SIGNED: &str = include_str!("fixtures/head001_valid_signed.xml");

fn codes(xml: &str) -> Vec<&'static str> {
    let hdr = head001::parse(xml).expect("fixture variant must stay parseable");
    validate_head001(&hdr).into_iter().map(|i| i.code).collect()
}

#[test]
fn parses_unsigned_header_into_typed_model() {
    let hdr = head001::parse(VALID).expect("valid fixture must parse");

    assert_eq!(hdr.xmlns.as_deref(), Some(head001::NAMESPACE));
    assert_eq!(
        hdr.business_message_identifier,
        "B20260702FIXTURE00000000000001"
    );
    assert_eq!(hdr.message_definition_identifier, "pacs.008.001.08");
    assert_eq!(hdr.creation_date, "2026-07-02T15:30:00Z");
    assert!(hdr.signature.is_none());

    let from_member = hdr
        .from
        .financial_institution
        .as_ref()
        .expect("Fr is an FIId")
        .financial_institution_identification
        .clearing_system_member_identification
        .as_ref()
        .expect("Fr carries a routing number");
    assert_eq!(from_member.member_identification, "021040078");
}

#[test]
fn both_fixtures_validate_clean() {
    for xml in [VALID, VALID_SIGNED] {
        let hdr = head001::parse(xml).unwrap();
        let issues = validate_head001(&hdr);
        assert!(
            issues.is_empty(),
            "expected clean validation, got: {issues:#?}"
        );
    }
}

#[test]
fn signed_header_reports_signature_presence() {
    let hdr = head001::parse(VALID_SIGNED).unwrap();
    assert!(hdr.signature.is_some());
}

#[test]
fn sgntr_raw_extracts_exact_wire_bytes() {
    let raw = head001::sgntr_raw(VALID_SIGNED).expect("signed fixture has Sgntr");
    assert!(raw.contains("<ds:Signature"));
    assert!(raw.contains("</ds:Signature>"));
    assert!(raw.contains("rsa-sha256"));
    // The slice is taken verbatim from the input, not re-serialized.
    let start = VALID_SIGNED
        .find(raw)
        .expect("raw slice must be a substring");
    assert!(start > 0);
    // And it excludes the envelope tags themselves.
    assert!(!raw.contains("<Sgntr>"));
    assert!(!raw.contains("</Sgntr>"));
}

#[test]
fn sgntr_raw_is_none_for_unsigned_header() {
    assert!(head001::sgntr_raw(VALID).is_none());
}

#[test]
fn malformed_xml_is_a_parse_error() {
    let err = head001::parse("<AppHdr><Fr></AppHdr>").unwrap_err();
    assert!(matches!(err, ParseError::Xml(_)));
}

#[test]
fn wrong_namespace_is_flagged() {
    let xml = VALID.replace("head.001.001.02", "head.001.001.01");
    assert!(codes(&xml).contains(&"xsd.namespace"));
}

#[test]
fn overlong_bizmsgidr_violates_max35text() {
    let xml = VALID.replace(
        "B20260702FIXTURE00000000000001",
        "B20260702FIXTURE00000000000001XXXXXX", // 36 chars
    );
    assert!(codes(&xml).contains(&"xsd.bizmsgidr.length"));
}

#[test]
fn msgdefidr_outside_iso_convention_is_flagged() {
    let xml = VALID.replace(
        "<MsgDefIdr>pacs.008.001.08</MsgDefIdr>",
        "<MsgDefIdr>not-a-message-id</MsgDefIdr>",
    );
    assert!(codes(&xml).contains(&"iso.msgdefidr.format"));
}

#[test]
fn non_utc_creation_date_is_flagged() {
    let xml = VALID.replace(
        "<CreDt>2026-07-02T15:30:00Z</CreDt>",
        "<CreDt>2026-07-02T10:30:00-05:00</CreDt>",
    );
    let hdr = head001::parse(&xml).unwrap();
    let issues = validate_head001(&hdr);
    let issue = issues
        .iter()
        .find(|i| i.code == "iso.credt.utc")
        .expect("must flag non-UTC CreDt");
    assert_eq!(issue.source, RuleSource::IsoRule);
}

#[test]
fn garbage_creation_date_violates_xsd_facet() {
    let xml = VALID.replace(
        "<CreDt>2026-07-02T15:30:00Z</CreDt>",
        "<CreDt>02/07/2026</CreDt>",
    );
    assert!(codes(&xml).contains(&"xsd.credt.format"));
}

#[test]
fn orgid_party_violates_fednow_profile() {
    let xml = VALID.replace(
        "<Fr>\n    <FIId>\n      <FinInstnId>\n        <ClrSysMmbId>\n          <ClrSysId>\n            <Cd>USABA</Cd>\n          </ClrSysId>\n          <MmbId>021040078</MmbId>\n        </ClrSysMmbId>\n      </FinInstnId>\n    </FIId>\n  </Fr>",
        "<Fr>\n    <OrgId>\n      <Nm>Some Corporate</Nm>\n    </OrgId>\n  </Fr>",
    );
    let hdr = head001::parse(&xml).unwrap();
    assert!(
        hdr.from.organisation.is_some() && hdr.from.financial_institution.is_none(),
        "edit must swap FIId for OrgId"
    );
    assert!(codes(&xml).contains(&"fednow.party.fiid"));
}

#[test]
fn bad_routing_checksum_in_from_is_flagged() {
    let xml = VALID.replace("021040078", "021040079");
    assert!(codes(&xml).contains(&"fednow.aba.checksum"));
}

#[test]
fn invalid_copy_duplicate_violates_xsd_enum() {
    let xml = VALID.replace(
        "<CreDt>2026-07-02T15:30:00Z</CreDt>",
        "<CreDt>2026-07-02T15:30:00Z</CreDt>\n  <CpyDplct>FAKE</CpyDplct>",
    );
    assert!(codes(&xml).contains(&"xsd.cpydplct.enum"));
}
