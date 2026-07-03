//! FedNow MQ technical envelope: `FedNowIncoming` / `FedNowOutgoing`.
//!
//! On the wire (IBM MQ), a FedNow business message is not a bare ISO 20022
//! `Document`: it travels inside a technical wrapper that pairs a Business
//! Application Header (head.001.001.02) with the `Document`, under a
//! message-type-specific wrapper element, optionally preceded by an open
//! technical header:
//!
//! ```text
//! <FedNowIncoming xmlns="urn:fednow:incoming:v001">
//!   <FedNowTechnicalHeader>…</FedNowTechnicalHeader>      (optional, lax)
//!   <FedNowIncomingMessage>
//!     <FedNowCustomerCreditTransfer>                      (one wrapper per type)
//!       <AppHdr xmlns="…head.001.001.02">…</AppHdr>
//!       <Document xmlns="…pacs.008.001.08">…</Document>
//!     </FedNowCustomerCreditTransfer>
//!   </FedNowIncomingMessage>
//! </FedNowIncoming>
//! ```
//!
//! "Incoming" and "outgoing" are named from the FedNow Service's point of
//! view: a participant *sends* `FedNowIncoming` and *receives*
//! `FedNowOutgoing`.
//!
//! The Fed's envelope schemas are confidential material and are not vendored
//! in this repo; this module implements the minimal interoperability facts
//! recorded in `docs/design/` (wrapper names, element order, namespaces).
//!
//! Parsing is deliberately split in two layers:
//! - [`split`] slices the raw XML into its parts **without re-serializing** —
//!   the `AppHdr` and `Document` byte ranges are returned exactly as they
//!   appear on the wire, which is what a future signing/verification layer
//!   must digest.
//! - [`parse`] builds the typed [`Envelope`] on top of those slices, using the
//!   per-message modules ([`crate::pacs008`], [`crate::pacs002`], …).

use crate::error::ParseError;
use crate::{camt029, camt056, head001, pacs002, pacs004, pacs008, pacs028};

/// Envelope namespace for participant → service messages.
pub const NAMESPACE_INCOMING: &str = "urn:fednow:incoming:v001";
/// Envelope namespace for service → participant messages.
pub const NAMESPACE_OUTGOING: &str = "urn:fednow:outgoing:v001";

/// Which way the envelope travels, named from the service's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Participant → FedNow Service (`FedNowIncoming`).
    Incoming,
    /// FedNow Service → participant (`FedNowOutgoing`).
    Outgoing,
}

impl Direction {
    /// Root element local name for this direction.
    pub fn root_element(self) -> &'static str {
        match self {
            Direction::Incoming => "FedNowIncoming",
            Direction::Outgoing => "FedNowOutgoing",
        }
    }

    /// Message-list element local name for this direction.
    pub fn message_element(self) -> &'static str {
        match self {
            Direction::Incoming => "FedNowIncomingMessage",
            Direction::Outgoing => "FedNowOutgoingMessage",
        }
    }

    /// Envelope namespace for this direction.
    pub fn namespace(self) -> &'static str {
        match self {
            Direction::Incoming => NAMESPACE_INCOMING,
            Direction::Outgoing => NAMESPACE_OUTGOING,
        }
    }
}

/// Wrapper element name ↔ enclosed `Document` namespace, for the message
/// types this crate models. The same wrapper names exist in both directions.
pub const WRAPPERS: &[(&str, &str)] = &[
    ("FedNowCustomerCreditTransfer", pacs008::NAMESPACE),
    ("FedNowPaymentStatus", pacs002::NAMESPACE),
    ("FedNowPaymentStatusRequest", pacs028::NAMESPACE),
    ("FedNowPaymentReturn", pacs004::NAMESPACE),
    ("FedNowReturnRequest", camt056::NAMESPACE),
    ("FedNowReturnRequestResponse", camt029::NAMESPACE),
];

/// Expected wrapper element for a `Document` namespace, if modeled.
pub fn wrapper_for_namespace(ns: &str) -> Option<&'static str> {
    WRAPPERS.iter().find(|(_, n)| *n == ns).map(|(w, _)| *w)
}

/// Expected `Document` namespace for a wrapper element, if modeled.
pub fn namespace_for_wrapper(wrapper: &str) -> Option<&'static str> {
    WRAPPERS
        .iter()
        .find(|(w, _)| *w == wrapper)
        .map(|(_, n)| *n)
}

/// The raw parts of an envelope, sliced out of the wire text without
/// re-serialization.
#[derive(Debug, Clone)]
pub struct RawEnvelope<'a> {
    pub direction: Direction,
    /// The default namespace declared on the root element, if any.
    pub root_namespace: Option<&'a str>,
    /// Local name of the message wrapper element (e.g.
    /// `FedNowCustomerCreditTransfer`).
    pub wrapper: &'a str,
    /// Raw inner XML of `FedNowTechnicalHeader`, if present.
    pub technical_header: Option<&'a str>,
    /// The complete `<AppHdr …>…</AppHdr>` slice, byte-exact.
    pub app_header: &'a str,
    /// The complete `<Document …>…</Document>` slice, byte-exact.
    pub document: &'a str,
}

/// Slice an envelope into its raw parts.
///
/// Fails only on malformed XML or a structure that is not an envelope at all
/// (unknown root, missing `AppHdr`/`Document`). Namespace or wrapper
/// mismatches do **not** fail here — they are reported by
/// [`crate::validate::validate_envelope`] so callers get a full diagnosis.
pub fn split(xml: &str) -> Result<RawEnvelope<'_>, ParseError> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(xml);

    let mut direction: Option<Direction> = None;
    let mut root_namespace: Option<&str> = None;
    let mut wrapper: Option<&str> = None;
    let mut technical_header: Option<&str> = None;
    let mut app_header: Option<&str> = None;
    let mut document: Option<&str> = None;

    // Depths (after the increment for the current start tag):
    // 1 = root, 2 = technical header / message list, 3 = wrapper,
    // 4 = AppHdr / Document.
    let mut depth = 0usize;
    let mut in_technical_header = false;
    let mut tech_inner_start: Option<usize> = None;
    let mut element_start: Option<usize> = None; // start offset of AppHdr/Document tag

    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                depth += 1;
                let local = e.local_name();
                let local = local.as_ref();
                match depth {
                    1 => {
                        direction = match local {
                            b"FedNowIncoming" => Some(Direction::Incoming),
                            b"FedNowOutgoing" => Some(Direction::Outgoing),
                            _ => {
                                return Err(ParseError::Envelope(format!(
                                    "root element '{}' is not FedNowIncoming/FedNowOutgoing",
                                    String::from_utf8_lossy(local)
                                )))
                            }
                        };
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"xmlns" {
                                if let Ok(v) = attr.unescape_value() {
                                    // Borrow from the source text: find the value
                                    // inside the tag slice to keep a &str.
                                    let tag = &xml[pos_before..reader.buffer_position() as usize];
                                    if let Some(found) = tag.find(v.as_ref()) {
                                        root_namespace = Some(&tag[found..found + v.len()]);
                                    }
                                }
                            }
                        }
                    }
                    2 => {
                        in_technical_header = local == b"FedNowTechnicalHeader";
                        if in_technical_header {
                            tech_inner_start = Some(reader.buffer_position() as usize);
                        }
                    }
                    3 => {
                        if wrapper.is_none() && !in_technical_header {
                            // Borrow the local name out of the source text.
                            let tag = &xml[pos_before..reader.buffer_position() as usize];
                            let name = tag
                                .trim_start_matches('<')
                                .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
                                .next()
                                .unwrap_or("");
                            let name = name.rsplit(':').next().unwrap_or(name);
                            let offset = tag.find(name).unwrap_or(1);
                            wrapper =
                                Some(&xml[pos_before + offset..pos_before + offset + name.len()]);
                        }
                    }
                    4 if local == b"AppHdr" || local == b"Document" => {
                        element_start = Some(pos_before);
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let local = e.local_name();
                let local = local.as_ref();
                let pos_after = reader.buffer_position() as usize;
                match depth {
                    2 => {
                        if local == b"FedNowTechnicalHeader" {
                            if let Some(start) = tech_inner_start.take() {
                                technical_header = Some(&xml[start..pos_before]);
                            }
                        }
                        in_technical_header = false;
                    }
                    4 => {
                        if let Some(start) = element_start.take() {
                            if local == b"AppHdr" {
                                app_header = Some(&xml[start..pos_after]);
                            } else if local == b"Document" {
                                document = Some(&xml[start..pos_after]);
                            }
                        }
                    }
                    _ => {}
                }
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(ParseError::Envelope(format!("malformed XML: {e}"))),
            _ => {}
        }
    }

    let direction = direction.ok_or_else(|| ParseError::Envelope("empty document".into()))?;
    let wrapper =
        wrapper.ok_or_else(|| ParseError::Envelope("no message wrapper element found".into()))?;
    let app_header =
        app_header.ok_or_else(|| ParseError::Envelope("no AppHdr element found".into()))?;
    let document =
        document.ok_or_else(|| ParseError::Envelope("no Document element found".into()))?;

    Ok(RawEnvelope {
        direction,
        root_namespace,
        wrapper,
        technical_header,
        app_header,
        document,
    })
}

/// The typed business content of an envelope, for the message types this
/// crate models.
#[derive(Debug, Clone)]
pub enum EnvelopedDocument {
    CustomerCreditTransfer(pacs008::Document),
    PaymentStatus(pacs002::Document),
    PaymentStatusRequest(pacs028::Document),
    PaymentReturn(pacs004::Document),
    ReturnRequest(camt056::Document),
    ReturnRequestResponse(camt029::Document),
}

impl EnvelopedDocument {
    /// The `Document` namespace this variant was parsed from.
    pub fn namespace(&self) -> &'static str {
        match self {
            EnvelopedDocument::CustomerCreditTransfer(_) => pacs008::NAMESPACE,
            EnvelopedDocument::PaymentStatus(_) => pacs002::NAMESPACE,
            EnvelopedDocument::PaymentStatusRequest(_) => pacs028::NAMESPACE,
            EnvelopedDocument::PaymentReturn(_) => pacs004::NAMESPACE,
            EnvelopedDocument::ReturnRequest(_) => camt056::NAMESPACE,
            EnvelopedDocument::ReturnRequestResponse(_) => camt029::NAMESPACE,
        }
    }

    /// The ISO message name (e.g. `pacs.008.001.08`).
    pub fn message_name(&self) -> &'static str {
        message_name_of(self.namespace())
    }
}

/// The ISO message name portion of a `Document` namespace.
fn message_name_of(ns: &'static str) -> &'static str {
    ns.rsplit(':').next().unwrap_or(ns)
}

/// A fully parsed envelope: typed header and business document, plus the raw
/// facts validation needs.
#[derive(Debug, Clone)]
pub struct Envelope {
    pub direction: Direction,
    /// Default namespace found on the root element, if any.
    pub root_namespace: Option<String>,
    /// Wrapper element local name as found on the wire.
    pub wrapper: String,
    /// Raw inner XML of the technical header, if present.
    pub technical_header: Option<String>,
    pub header: head001::AppHdr,
    pub document: EnvelopedDocument,
}

/// Parse an envelope into typed content.
///
/// The business `Document` is dispatched on **its own namespace** (not the
/// wrapper element), so a mismatched wrapper still parses and is then flagged
/// by [`crate::validate::validate_envelope`].
pub fn parse(xml: &str) -> Result<Envelope, ParseError> {
    let raw = split(xml)?;

    let header = head001::parse(raw.app_header)?;

    let doc_ns = sniff_default_namespace(raw.document).unwrap_or("");
    let document = if doc_ns == pacs008::NAMESPACE {
        EnvelopedDocument::CustomerCreditTransfer(pacs008::parse(raw.document)?)
    } else if doc_ns == pacs002::NAMESPACE {
        EnvelopedDocument::PaymentStatus(pacs002::parse(raw.document)?)
    } else if doc_ns == pacs028::NAMESPACE {
        EnvelopedDocument::PaymentStatusRequest(pacs028::parse(raw.document)?)
    } else if doc_ns == pacs004::NAMESPACE {
        EnvelopedDocument::PaymentReturn(pacs004::parse(raw.document)?)
    } else if doc_ns == camt056::NAMESPACE {
        EnvelopedDocument::ReturnRequest(camt056::parse(raw.document)?)
    } else if doc_ns == camt029::NAMESPACE {
        EnvelopedDocument::ReturnRequestResponse(camt029::parse(raw.document)?)
    } else {
        return Err(ParseError::Envelope(format!(
            "unsupported Document namespace '{doc_ns}'"
        )));
    };

    Ok(Envelope {
        direction: raw.direction,
        root_namespace: raw.root_namespace.map(str::to_owned),
        wrapper: raw.wrapper.to_owned(),
        technical_header: raw.technical_header.map(str::to_owned),
        header,
        document,
    })
}

/// Read the default `xmlns` off the first start tag of an XML slice.
fn sniff_default_namespace(xml: &str) -> Option<&str> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == b"xmlns" {
                        let v = attr.unescape_value().ok()?;
                        let tag = &xml[pos_before..reader.buffer_position() as usize];
                        let found = tag.find(v.as_ref())?;
                        return Some(&tag[found..found + v.len()]);
                    }
                }
                return None;
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

/// Build envelope XML around already-serialized `AppHdr` and `Document` text.
///
/// The header and document are embedded byte-for-byte (each must carry its own
/// `xmlns`), so what a signer digested is exactly what travels. `wrapper` is
/// the message-type wrapper element (see [`WRAPPERS`]).
pub fn build(
    direction: Direction,
    wrapper: &str,
    app_header_xml: &str,
    document_xml: &str,
    technical_header_inner_xml: Option<&str>,
) -> String {
    let root = direction.root_element();
    let list = direction.message_element();
    let ns = direction.namespace();
    let mut out = String::with_capacity(app_header_xml.len() + document_xml.len() + 256);
    out.push_str(&format!("<{root} xmlns=\"{ns}\">"));
    if let Some(tech) = technical_header_inner_xml {
        out.push_str("<FedNowTechnicalHeader>");
        out.push_str(tech);
        out.push_str("</FedNowTechnicalHeader>");
    }
    out.push_str(&format!("<{list}><{wrapper}>"));
    out.push_str(app_header_xml);
    out.push_str(document_xml);
    out.push_str(&format!("</{wrapper}></{list}></{root}>"));
    out
}
