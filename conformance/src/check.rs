//! Judge one message against the FedNow Release 1 profiles.

use fednow_core::validate::{
    validate_camt029, validate_camt056, validate_envelope, validate_head001,
    validate_pacs002_direction, validate_pacs004, validate_pacs008, validate_pacs028,
    Pacs002Direction, ValidationIssue,
};
use fednow_core::{camt029, camt056, envelope, head001, pacs002, pacs004, pacs008, pacs028};

/// Which FedNow direction a pacs.002 must be judged against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Participant,
    Service,
    /// Accept the message if it is clean for either direction.
    Either,
}

/// The verdict for one message.
#[derive(Debug)]
pub struct Verdict {
    pub message_type: &'static str,
    pub issues: Vec<ValidationIssue>,
}

#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error("unrecognized message namespace")]
    UnknownNamespace,
    #[error("parse error: {0}")]
    Parse(#[from] fednow_core::ParseError),
}

/// Detect the message type from the namespace, parse and validate.
pub fn check(xml: &str, direction: Direction) -> Result<Verdict, CheckError> {
    // Envelopes first: they contain the BAH and Document namespaces too, so
    // the wrapper namespace must win the dispatch.
    if xml.contains(envelope::NAMESPACE_INCOMING) || xml.contains(envelope::NAMESPACE_OUTGOING) {
        return Ok(Verdict {
            message_type: "envelope",
            issues: validate_envelope(&envelope::parse(xml)?),
        });
    }
    if xml.contains(pacs008::NAMESPACE) {
        return Ok(Verdict {
            message_type: "pacs.008",
            issues: validate_pacs008(&pacs008::parse(xml)?),
        });
    }
    if xml.contains(pacs002::NAMESPACE) {
        let doc = pacs002::parse(xml)?;
        let issues = match direction {
            Direction::Participant => {
                validate_pacs002_direction(&doc, Pacs002Direction::ParticipantToService)
            }
            Direction::Service => {
                validate_pacs002_direction(&doc, Pacs002Direction::ServiceToParticipant)
            }
            Direction::Either => {
                let p = validate_pacs002_direction(&doc, Pacs002Direction::ParticipantToService);
                if p.is_empty() {
                    p
                } else {
                    let s =
                        validate_pacs002_direction(&doc, Pacs002Direction::ServiceToParticipant);
                    if s.is_empty() {
                        s
                    } else {
                        p
                    }
                }
            }
        };
        return Ok(Verdict {
            message_type: "pacs.002",
            issues,
        });
    }
    if xml.contains(pacs028::NAMESPACE) {
        return Ok(Verdict {
            message_type: "pacs.028",
            issues: validate_pacs028(&pacs028::parse(xml)?),
        });
    }
    if xml.contains(pacs004::NAMESPACE) {
        return Ok(Verdict {
            message_type: "pacs.004",
            issues: validate_pacs004(&pacs004::parse(xml)?),
        });
    }
    if xml.contains(camt056::NAMESPACE) {
        return Ok(Verdict {
            message_type: "camt.056",
            issues: validate_camt056(&camt056::parse(xml)?),
        });
    }
    if xml.contains(camt029::NAMESPACE) {
        return Ok(Verdict {
            message_type: "camt.029",
            issues: validate_camt029(&camt029::parse(xml)?),
        });
    }
    if xml.contains(head001::NAMESPACE) {
        return Ok(Verdict {
            message_type: "head.001",
            issues: validate_head001(&head001::parse(xml)?),
        });
    }
    Err(CheckError::UnknownNamespace)
}
