//! Tests for the FedNow technical envelope (FedNowIncoming / FedNowOutgoing).

use fednow_core::envelope::{self, Direction, EnvelopedDocument};
use fednow_core::validate_envelope;

const INCOMING: &str = include_str!("fixtures/envelope_incoming_pacs008.xml");
const OUTGOING: &str = include_str!("fixtures/envelope_outgoing_pacs002.xml");

#[test]
fn split_slices_incoming_envelope_byte_exact() {
    let raw = envelope::split(INCOMING).expect("split");
    assert_eq!(raw.direction, Direction::Incoming);
    assert_eq!(raw.root_namespace, Some("urn:fednow:incoming:v001"));
    assert_eq!(raw.wrapper, "FedNowCustomerCreditTransfer");
    assert_eq!(
        raw.technical_header.map(str::trim),
        Some("<ConnectionId>FIXTURECONN01</ConnectionId>")
    );
    // Byte-exact: the slices appear verbatim in the source text.
    assert!(raw.app_header.starts_with("<AppHdr"));
    assert!(raw.app_header.ends_with("</AppHdr>"));
    assert!(raw.document.starts_with("<Document"));
    assert!(raw.document.ends_with("</Document>"));
    assert!(INCOMING.contains(raw.app_header));
    assert!(INCOMING.contains(raw.document));
}

#[test]
fn parse_and_validate_incoming_pacs008() {
    let env = envelope::parse(INCOMING).expect("parse");
    assert_eq!(env.direction, Direction::Incoming);
    assert_eq!(env.header.message_definition_identifier, "pacs.008.001.08");
    match &env.document {
        EnvelopedDocument::CustomerCreditTransfer(doc) => {
            let tx = &doc
                .fi_to_fi_customer_credit_transfer
                .credit_transfer_transaction_information[0];
            assert_eq!(tx.interbank_settlement_amount.value, "1250.00");
        }
        other => panic!("wrong document variant: {other:?}"),
    }
    let issues = validate_envelope(&env);
    assert!(issues.is_empty(), "expected clean, got: {issues:#?}");
}

#[test]
fn parse_and_validate_outgoing_pacs002_advice() {
    let env = envelope::parse(OUTGOING).expect("parse");
    assert_eq!(env.direction, Direction::Outgoing);
    assert!(matches!(env.document, EnvelopedDocument::PaymentStatus(_)));
    let issues = validate_envelope(&env);
    assert!(issues.is_empty(), "expected clean, got: {issues:#?}");
}

#[test]
fn wrapper_document_mismatch_is_flagged() {
    let mutated = INCOMING.replace("FedNowCustomerCreditTransfer", "FedNowPaymentStatus");
    let env = envelope::parse(&mutated).expect("parse survives wrapper mismatch");
    let issues = validate_envelope(&env);
    assert!(
        issues.iter().any(|i| i.code == "fednow.env.wrapper.match"),
        "missing fednow.env.wrapper.match in: {issues:#?}"
    );
}

#[test]
fn msgdefidr_document_mismatch_is_flagged() {
    let mutated = INCOMING.replace(
        "<MsgDefIdr>pacs.008.001.08</MsgDefIdr>",
        "<MsgDefIdr>pacs.002.001.10</MsgDefIdr>",
    );
    let env = envelope::parse(&mutated).expect("parse");
    let issues = validate_envelope(&env);
    assert!(
        issues
            .iter()
            .any(|i| i.code == "fednow.env.msgdefidr.match"),
        "missing fednow.env.msgdefidr.match in: {issues:#?}"
    );
}

#[test]
fn wrong_root_namespace_is_flagged() {
    let mutated = INCOMING.replace("urn:fednow:incoming:v001", "urn:fednow:outgoing:v001");
    let env = envelope::parse(&mutated).expect("parse");
    // Direction comes from the root element name; only the namespace is off.
    assert_eq!(env.direction, Direction::Incoming);
    let issues = validate_envelope(&env);
    assert!(
        issues.iter().any(|i| i.code == "fednow.env.namespace"),
        "missing fednow.env.namespace in: {issues:#?}"
    );
}

#[test]
fn outgoing_from_participant_is_flagged() {
    // The service application id must be in Fr of an outgoing envelope.
    let mutated = OUTGOING.replace("<MmbId>021150706</MmbId>", "<MmbId>091000019</MmbId>");
    let env = envelope::parse(&mutated).expect("parse");
    let issues = validate_envelope(&env);
    assert!(
        issues.iter().any(|i| i.code == "fednow.env.fr.service"),
        "missing fednow.env.fr.service in: {issues:#?}"
    );
}

#[test]
fn build_round_trips_byte_exact() {
    let raw = envelope::split(INCOMING).expect("split");
    let built = envelope::build(
        Direction::Incoming,
        "FedNowCustomerCreditTransfer",
        raw.app_header,
        raw.document,
        None,
    );
    let raw2 = envelope::split(&built).expect("split built");
    // The embedded header/document are preserved byte-for-byte.
    assert_eq!(raw2.app_header, raw.app_header);
    assert_eq!(raw2.document, raw.document);
    assert_eq!(raw2.technical_header, None);

    let env = envelope::parse(&built).expect("parse built");
    let issues = validate_envelope(&env);
    assert!(issues.is_empty(), "expected clean, got: {issues:#?}");
}

#[test]
fn bare_document_is_not_an_envelope() {
    let bare = include_str!("fixtures/pacs008_valid.xml");
    assert!(envelope::split(bare).is_err());
}

#[test]
fn wrapper_mapping_is_bidirectional() {
    for (wrapper, ns) in envelope::WRAPPERS {
        assert_eq!(envelope::wrapper_for_namespace(ns), Some(*wrapper));
        assert_eq!(envelope::namespace_for_wrapper(wrapper), Some(*ns));
    }
}
