//! Validate a FedNow ISO 20022 message file against fednow-core's rules.
//!
//! ```sh
//! cargo run -p fednow-core --example validate -- path/to/message.xml
//! ```
//!
//! The message type is detected from the XML namespace. pacs.002 is validated
//! against both FedNow directions; it passes if either direction is clean.
//! Exit code 0 = clean, 1 = violations found, 2 = parse/usage error.

use std::process::ExitCode;

use fednow_core::validate::{
    validate_head001, validate_pacs002_direction, validate_pacs008, validate_pacs028,
    Pacs002Direction, ValidationIssue,
};
use fednow_core::{head001, pacs002, pacs008, pacs028};

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: validate <message.xml>");
        return ExitCode::from(2);
    };
    let xml = match std::fs::read_to_string(&path) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{path}: cannot read: {e}");
            return ExitCode::from(2);
        }
    };

    let issues: Vec<ValidationIssue> = if xml.contains(pacs008::NAMESPACE) {
        match pacs008::parse(&xml) {
            Ok(doc) => validate_pacs008(&doc),
            Err(e) => return parse_error(&path, e),
        }
    } else if xml.contains(pacs002::NAMESPACE) {
        match pacs002::parse(&xml) {
            Ok(doc) => {
                let participant =
                    validate_pacs002_direction(&doc, Pacs002Direction::ParticipantToService);
                if participant.is_empty() {
                    participant
                } else {
                    let service =
                        validate_pacs002_direction(&doc, Pacs002Direction::ServiceToParticipant);
                    if service.is_empty() {
                        service
                    } else {
                        participant
                    }
                }
            }
            Err(e) => return parse_error(&path, e),
        }
    } else if xml.contains(pacs028::NAMESPACE) {
        match pacs028::parse(&xml) {
            Ok(doc) => validate_pacs028(&doc),
            Err(e) => return parse_error(&path, e),
        }
    } else if xml.contains(head001::NAMESPACE) {
        match head001::parse(&xml) {
            Ok(hdr) => validate_head001(&hdr),
            Err(e) => return parse_error(&path, e),
        }
    } else {
        eprintln!("{path}: unrecognized message namespace");
        return ExitCode::from(2);
    };

    if issues.is_empty() {
        println!("{path}: OK");
        ExitCode::SUCCESS
    } else {
        println!("{path}: {} issue(s)", issues.len());
        for i in &issues {
            println!("  [{}] {} — {}", i.code, i.path, i.message);
        }
        ExitCode::from(1)
    }
}

fn parse_error(path: &str, e: fednow_core::ParseError) -> ExitCode {
    eprintln!("{path}: parse error: {e}");
    ExitCode::from(2)
}
