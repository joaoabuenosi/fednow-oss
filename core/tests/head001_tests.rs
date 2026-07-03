//! Integration tests for head.001.001.02 (Business Application Header) under the
//! FedNow profile.
//!
//! Two base fixtures: a FedNow-conformant header (no Sgntr — the FedNow profile
//! removes it; signatures travel outside the XML) and a base-ISO header carrying
//! an XMLDSig skeleton in Sgntr, used to exercise the presence flag and raw
//! extraction. Invalid variants derive from them by targeted string edits.

use fednow_core::validate::{validate_head001, RuleSource};
use fednow_core::{head001, ParseError};

const VALID: &str = include_str!("fixtures/head001_valid.xml");
const VALID_SIGNED: &str = include_str!("fixtures/head001_valid_signed.xml");

const MKTPRCTC_BLOCK: &str = "<MktPrctc>\n    <Regy>www2.swift.com/mystandards/#/group/Federal_Reserve_Financial_Services/FedNow_Service</Regy>\n    <Id>frb.fednow.01</Id>\n  </MktPrctc>\n  ";

fn codes(xml: &str) -> Vec<&'static str> {
    let hdr = head001::parse(xml).expect("fixture variant must stay parseable");
    validate_head001(&hdr).into_iter().map(|i| i.code).collect()
}

#[test]
fn parses_fednow_header_into_typed_model() {
    let hdr = head001::parse(VALID).expect("valid fixture must parse");

    assert_eq!(hdr.xmlns.as_deref(), Some(head001::NAMESPACE));
    assert_eq!(
        hdr.business_message_identifier,
        "B20260702FIXTURE00000000000001"
    );
    assert_eq!(hdr.message_definition_identifier, "pacs.008.001.08");
    assert_eq!(hdr.creation_date, "2026-07-02T15:30:00Z");
    assert!(hdr.signature.is_none());

    let mp = hdr.market_practice.as_ref().expect("MktPrctc is mandatory");
    assert_eq!(mp.identification, "frb.fednow.01");

    let from_member = hdr
        .from
        .financial_institution
        .as_ref()
        .expect("Fr is an FIId")
        .financial_institution_identification
        .clearing_system_member_identification
        .as_ref()
        .expect("Fr carries a connection party id");
    assert_eq!(from_member.member_identification, "021040078");
    // FedNow BAH carries only MmbId — no ClrSysId.
    assert!(from_member.clearing_system_identification.is_none());
}

#[test]
fn fednow_conformant_header_validates_clean() {
    let hdr = head001::parse(VALID).unwrap();
    let issues = validate_head001(&hdr);
    assert!(
        issues.is_empty(),
        "expected clean validation, got: {issues:#?}"
    );
}

#[test]
fn sgntr_presence_is_flagged_as_out_of_band_violation() {
    // Base-ISO-valid, but the FedNow profile removed Sgntr from the BAH.
    let hdr = head001::parse(VALID_SIGNED).unwrap();
    assert!(hdr.signature.is_some());
    let issues = validate_head001(&hdr);
    assert_eq!(
        issues.len(),
        1,
        "only the Sgntr issue expected: {issues:#?}"
    );
    assert_eq!(issues[0].code, "fednow.sgntr.outofband");
    assert_eq!(issues[0].source, RuleSource::FedNowProfile);
}

#[test]
fn sgntr_raw_extracts_exact_wire_bytes() {
    let raw = head001::sgntr_raw(VALID_SIGNED).expect("signed fixture has Sgntr");
    assert!(raw.contains("<ds:Signature"));
    assert!(raw.contains("</ds:Signature>"));
    // The slice is taken verbatim from the input, not re-serialized.
    assert!(VALID_SIGNED.contains(raw));
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
fn non_001_message_variant_violates_fednow_profile() {
    let xml = VALID.replace(
        "<MsgDefIdr>pacs.008.001.08</MsgDefIdr>",
        "<MsgDefIdr>pacs.008.002.08</MsgDefIdr>",
    );
    assert!(codes(&xml).contains(&"fednow.msgdefidr.variant"));
}

#[test]
fn missing_market_practice_violates_fednow_profile() {
    let xml = VALID.replace(MKTPRCTC_BLOCK, "");
    let hdr = head001::parse(&xml).unwrap();
    assert!(
        hdr.market_practice.is_none(),
        "edit must remove the MktPrctc block"
    );
    assert!(codes(&xml).contains(&"fednow.mktprctc.required"));
}

#[test]
fn wrong_market_practice_registry_is_flagged() {
    let xml = VALID.replace(
        "www2.swift.com/mystandards/#/group/Federal_Reserve_Financial_Services/FedNow_Service",
        "example.com/registry",
    );
    assert!(codes(&xml).contains(&"fednow.mktprctc.regy"));
}

#[test]
fn market_practice_id_pattern_is_enforced() {
    for (id, ok) in [
        ("frb.fednow.01", true),
        ("frb.fednow.rrr.01", true),
        ("frb.fednow.99", false),
        ("frb.fednow.RRR.01", false),
        ("frb.fedwire.01", false),
        ("frb.fednow.ab.01", false),
    ] {
        let xml = VALID.replace("<Id>frb.fednow.01</Id>", &format!("<Id>{id}</Id>"));
        let found = codes(&xml).contains(&"fednow.mktprctc.id");
        assert_eq!(found, !ok, "id '{id}' expected ok={ok}");
    }
}

#[test]
fn creation_date_with_utc_offset_is_accepted() {
    // CreationDateRule allows UTC or local time with a UTC offset.
    let xml = VALID.replace(
        "<CreDt>2026-07-02T15:30:00Z</CreDt>",
        "<CreDt>2026-07-02T10:30:00-05:00</CreDt>",
    );
    let hdr = head001::parse(&xml).unwrap();
    let issues = validate_head001(&hdr);
    assert!(issues.is_empty(), "offset must be accepted: {issues:#?}");
}

#[test]
fn creation_date_without_timezone_is_flagged() {
    let xml = VALID.replace(
        "<CreDt>2026-07-02T15:30:00Z</CreDt>",
        "<CreDt>2026-07-02T15:30:00</CreDt>",
    );
    let hdr = head001::parse(&xml).unwrap();
    let issues = validate_head001(&hdr);
    let issue = issues
        .iter()
        .find(|i| i.code == "fednow.credt.timezone")
        .expect("must flag timezone-less CreDt");
    assert_eq!(issue.source, RuleSource::FedNowProfile);
}

#[test]
fn to_party_must_address_the_service_application() {
    let xml = VALID.replace("<MmbId>021150706</MmbId>", "<MmbId>091000019</MmbId>");
    assert!(codes(&xml).contains(&"fednow.to.service"));
}

#[test]
fn business_service_must_not_be_used_in_release_1() {
    let xml = VALID.replace(
        "<MsgDefIdr>pacs.008.001.08</MsgDefIdr>",
        "<MsgDefIdr>pacs.008.001.08</MsgDefIdr>\n  <BizSvc>SOMESVC</BizSvc>",
    );
    assert!(codes(&xml).contains(&"fednow.bizsvc.absent"));
}

#[test]
fn business_processing_date_is_service_only() {
    let xml = VALID.replace(
        "<CreDt>2026-07-02T15:30:00Z</CreDt>",
        "<CreDt>2026-07-02T15:30:00Z</CreDt>\n  <BizPrcgDt>2026-07-02T15:31:00Z</BizPrcgDt>",
    );
    assert!(codes(&xml).contains(&"fednow.bizprcgdt.serviceonly"));
}

#[test]
fn copy_duplicate_from_participant_is_flagged_as_service_only() {
    let xml = VALID.replace(
        "<CreDt>2026-07-02T15:30:00Z</CreDt>",
        "<CreDt>2026-07-02T15:30:00Z</CreDt>\n  <CpyDplct>DUPL</CpyDplct>",
    );
    assert!(codes(&xml).contains(&"fednow.cpydplct.serviceonly"));
}

#[test]
fn market_practice_id_must_match_the_enclosed_message() {
    // rrr belongs to camt.029, not pacs.008.
    let xml = VALID.replace("<Id>frb.fednow.01</Id>", "<Id>frb.fednow.rrr.01</Id>");
    assert!(codes(&xml).contains(&"fednow.mktprctc.match"));

    // And with a camt.029 message it is the expected value.
    let xml = VALID
        .replace(
            "<MsgDefIdr>pacs.008.001.08</MsgDefIdr>",
            "<MsgDefIdr>camt.029.001.09</MsgDefIdr>",
        )
        .replace("<Id>frb.fednow.01</Id>", "<Id>frb.fednow.rrr.01</Id>");
    assert!(!codes(&xml).contains(&"fednow.mktprctc.match"));
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
        "<Fr>\n    <FIId>\n      <FinInstnId>\n        <ClrSysMmbId>\n          <MmbId>021040078</MmbId>\n        </ClrSysMmbId>\n      </FinInstnId>\n    </FIId>\n  </Fr>",
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
fn clrsysid_in_bah_party_violates_fednow_profile() {
    let xml = VALID.replace(
        "<ClrSysMmbId>\n          <MmbId>021040078</MmbId>",
        "<ClrSysMmbId>\n          <ClrSysId><Cd>USABA</Cd></ClrSysId>\n          <MmbId>021040078</MmbId>",
    );
    assert!(codes(&xml).contains(&"fednow.party.clrsysid"));
}

#[test]
fn eti_style_connection_party_id_is_accepted() {
    // Connection party ids may be alphanumeric (ETI or FedNow-assigned).
    let xml = VALID.replace("021040078", "A1B2C3D4E");
    let hdr = head001::parse(&xml).unwrap();
    let issues = validate_head001(&hdr);
    assert!(issues.is_empty(), "ETI must be accepted, got: {issues:#?}");
}

#[test]
fn short_connection_party_id_is_flagged() {
    let xml = VALID.replace("021040078", "12345");
    assert!(codes(&xml).contains(&"fednow.connparty.format"));
}

#[test]
fn lowercase_connection_party_id_is_flagged() {
    let xml = VALID.replace("021040078", "a1b2c3d4e");
    assert!(codes(&xml).contains(&"fednow.connparty.format"));
}

#[test]
fn all_digit_connection_party_id_with_bad_checksum_is_flagged() {
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

#[test]
fn non_dupl_copy_duplicate_violates_fednow_profile() {
    let xml = VALID.replace(
        "<CreDt>2026-07-02T15:30:00Z</CreDt>",
        "<CreDt>2026-07-02T15:30:00Z</CreDt>\n  <CpyDplct>COPY</CpyDplct>",
    );
    let found = codes(&xml);
    assert!(found.contains(&"fednow.cpydplct.dupl"), "got {found:?}");
    assert!(!found.contains(&"xsd.cpydplct.enum"));
}
