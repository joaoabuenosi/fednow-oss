use thiserror::Error;

/// Errors produced while turning raw XML into a typed message.
///
/// A `ParseError` means the document could not be represented at all (malformed
/// XML, missing required elements, wrong structure). Content that is structurally
/// present but violates facets or FedNow profile rules is reported by
/// [`crate::validate::validate_pacs008`] instead, so callers can distinguish
/// "not a pacs.008" from "a pacs.008 with invalid content".
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("malformed XML or unexpected document structure: {0}")]
    Xml(#[from] quick_xml::DeError),
}
