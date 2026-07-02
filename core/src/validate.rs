//! Rule validation for parsed pacs.008 documents.
//!
//! Three rule sources, so a violation can be traced to where the requirement
//! comes from:
//! - [`RuleSource::XsdFacet`] — lexical facets of pacs.008.001.08 (lengths,
//!   patterns, enumerations, numeric limits).
//! - [`RuleSource::IsoRule`] — ISO 20022 cross-field rules the schema cannot
//!   express (e.g. `NbOfTxs` must equal the transaction count).
//! - [`RuleSource::FedNowProfile`] — the FedNow Service profile (USD only,
//!   settlement method CLRG, charge bearer SLEV, ABA routing-number checksum,
//!   cent-precision amounts).
//!
//! All issues are collected; nothing short-circuits.

use crate::pacs008::{ActiveCurrencyAndAmount, CreditTransferTransaction, Document, NAMESPACE};

/// Where a validation requirement comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleSource {
    XsdFacet,
    IsoRule,
    FedNowProfile,
}

/// One rule violation found in a document.
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    /// Stable machine-readable code, e.g. `"fednow.aba.checksum"`.
    pub code: &'static str,
    /// Element path in short-name notation, e.g. `"CdtTrfTxInf[0]/IntrBkSttlmAmt"`.
    pub path: String,
    /// Human-readable explanation.
    pub message: String,
    pub source: RuleSource,
}

impl ValidationIssue {
    fn new(
        code: &'static str,
        path: impl Into<String>,
        message: impl Into<String>,
        source: RuleSource,
    ) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
            source,
        }
    }
}

/// Validate a parsed pacs.008 document, returning every violation found.
///
/// An empty vector means the document passed all implemented checks.
pub fn validate_pacs008(doc: &Document) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    if doc.xmlns.as_deref() != Some(NAMESPACE) {
        issues.push(ValidationIssue::new(
            "xsd.namespace",
            "Document",
            format!(
                "expected namespace {NAMESPACE}, found {}",
                doc.xmlns.as_deref().unwrap_or("(none)")
            ),
            RuleSource::XsdFacet,
        ));
    }

    let msg = &doc.fi_to_fi_customer_credit_transfer;
    let hdr = &msg.group_header;

    check_max35text(
        &mut issues,
        "xsd.msgid.length",
        "GrpHdr/MsgId",
        &hdr.message_identification,
    );

    if !is_iso_date_time(&hdr.creation_date_time) {
        issues.push(ValidationIssue::new(
            "xsd.credttm.format",
            "GrpHdr/CreDtTm",
            format!(
                "'{}' is not a valid ISO 8601 date-time",
                hdr.creation_date_time
            ),
            RuleSource::XsdFacet,
        ));
    }

    // Max15NumericText
    let nb = &hdr.number_of_transactions;
    if nb.is_empty() || nb.len() > 15 || !nb.bytes().all(|b| b.is_ascii_digit()) {
        issues.push(ValidationIssue::new(
            "xsd.nboftxs.pattern",
            "GrpHdr/NbOfTxs",
            format!("'{nb}' does not match [0-9]{{1,15}}"),
            RuleSource::XsdFacet,
        ));
    } else if nb.parse::<u64>().ok()
        != Some(msg.credit_transfer_transaction_information.len() as u64)
    {
        issues.push(ValidationIssue::new(
            "iso.nboftxs.mismatch",
            "GrpHdr/NbOfTxs",
            format!(
                "NbOfTxs is {nb} but the document contains {} CdtTrfTxInf element(s)",
                msg.credit_transfer_transaction_information.len()
            ),
            RuleSource::IsoRule,
        ));
    }

    if hdr.settlement_information.settlement_method != "CLRG" {
        issues.push(ValidationIssue::new(
            "fednow.sttlmmtd.clrg",
            "GrpHdr/SttlmInf/SttlmMtd",
            format!(
                "FedNow settles as a clearing system: SttlmMtd must be CLRG, found '{}'",
                hdr.settlement_information.settlement_method
            ),
            RuleSource::FedNowProfile,
        ));
    }

    for (i, tx) in msg
        .credit_transfer_transaction_information
        .iter()
        .enumerate()
    {
        validate_transaction(&mut issues, i, tx);
    }

    issues
}

fn validate_transaction(
    issues: &mut Vec<ValidationIssue>,
    i: usize,
    tx: &CreditTransferTransaction,
) {
    let base = format!("CdtTrfTxInf[{i}]");

    check_max35text(
        issues,
        "xsd.endtoendid.length",
        format!("{base}/PmtId/EndToEndId"),
        &tx.payment_identification.end_to_end_identification,
    );

    if let Some(uetr) = &tx.payment_identification.uetr {
        if !is_uetr(uetr) {
            issues.push(ValidationIssue::new(
                "xsd.uetr.pattern",
                format!("{base}/PmtId/UETR"),
                format!("'{uetr}' is not a lowercase UUID v4 as required by the UUIDv4Identifier pattern"),
                RuleSource::XsdFacet,
            ));
        }
    }

    validate_amount(issues, &base, &tx.interbank_settlement_amount);

    // ChargeBearerType1Code enumeration, then the FedNow restriction on top.
    const CHARGE_BEARERS: [&str; 4] = ["DEBT", "CRED", "SHAR", "SLEV"];
    if !CHARGE_BEARERS.contains(&tx.charge_bearer.as_str()) {
        issues.push(ValidationIssue::new(
            "xsd.chrgbr.enum",
            format!("{base}/ChrgBr"),
            format!(
                "'{}' is not one of DEBT, CRED, SHAR, SLEV",
                tx.charge_bearer
            ),
            RuleSource::XsdFacet,
        ));
    } else if tx.charge_bearer != "SLEV" {
        issues.push(ValidationIssue::new(
            "fednow.chrgbr.slev",
            format!("{base}/ChrgBr"),
            format!(
                "FedNow requires ChrgBr SLEV (charges follow service level), found '{}'",
                tx.charge_bearer
            ),
            RuleSource::FedNowProfile,
        ));
    }

    for (agent, tag) in [
        (&tx.debtor_agent, "DbtrAgt"),
        (&tx.creditor_agent, "CdtrAgt"),
    ] {
        if let Some(member) = &agent
            .financial_institution_identification
            .clearing_system_member_identification
        {
            validate_routing_number(
                issues,
                format!("{base}/{tag}/FinInstnId/ClrSysMmbId/MmbId"),
                &member.member_identification,
            );
        }
    }
}

fn validate_amount(
    issues: &mut Vec<ValidationIssue>,
    base: &str,
    amount: &ActiveCurrencyAndAmount,
) {
    let path = format!("{base}/IntrBkSttlmAmt");

    let ccy = &amount.currency;
    if ccy.len() != 3 || !ccy.bytes().all(|b| b.is_ascii_uppercase()) {
        issues.push(ValidationIssue::new(
            "xsd.ccy.pattern",
            format!("{path}/@Ccy"),
            format!("'{ccy}' does not match the ActiveCurrencyCode pattern [A-Z]{{3}}"),
            RuleSource::XsdFacet,
        ));
    } else if ccy != "USD" {
        issues.push(ValidationIssue::new(
            "fednow.ccy.usd",
            format!("{path}/@Ccy"),
            format!("FedNow settles in USD only, found '{ccy}'"),
            RuleSource::FedNowProfile,
        ));
    }

    match parse_decimal(&amount.value) {
        None => issues.push(ValidationIssue::new(
            "xsd.amount.format",
            path,
            format!(
                "'{}' is not a valid ActiveCurrencyAndAmount (decimal, max 18 total digits, max 5 fraction digits)",
                amount.value
            ),
            RuleSource::XsdFacet,
        )),
        Some(dec) => {
            if dec.is_zero {
                issues.push(ValidationIssue::new(
                    "fednow.amount.positive",
                    path.clone(),
                    "credit transfer amount must be greater than zero".to_string(),
                    RuleSource::FedNowProfile,
                ));
            }
            if dec.fraction_digits > 2 {
                issues.push(ValidationIssue::new(
                    "fednow.amount.cents",
                    path,
                    format!("USD amounts carry at most 2 fraction digits, found {}", dec.fraction_digits),
                    RuleSource::FedNowProfile,
                ));
            }
        }
    }
}

fn validate_routing_number(issues: &mut Vec<ValidationIssue>, path: String, member_id: &str) {
    if member_id.len() != 9 || !member_id.bytes().all(|b| b.is_ascii_digit()) {
        issues.push(ValidationIssue::new(
            "fednow.aba.format",
            path,
            format!("'{member_id}' is not a 9-digit ABA routing number"),
            RuleSource::FedNowProfile,
        ));
    } else if !aba_checksum_ok(member_id) {
        issues.push(ValidationIssue::new(
            "fednow.aba.checksum",
            path,
            format!(
                "'{member_id}' fails the ABA routing-number check digit (weights 3-7-1, mod 10)"
            ),
            RuleSource::FedNowProfile,
        ));
    }
}

fn check_max35text(
    issues: &mut Vec<ValidationIssue>,
    code: &'static str,
    path: impl Into<String>,
    value: &str,
) {
    if value.is_empty() || value.chars().count() > 35 {
        issues.push(ValidationIssue::new(
            code,
            path,
            format!("'{value}' violates Max35Text (1..35 characters)"),
            RuleSource::XsdFacet,
        ));
    }
}

/// ABA routing-number check digit: 3·(d1+d4+d7) + 7·(d2+d5+d8) + 1·(d3+d6+d9) ≡ 0 (mod 10).
fn aba_checksum_ok(digits: &str) -> bool {
    const WEIGHTS: [u32; 9] = [3, 7, 1, 3, 7, 1, 3, 7, 1];
    let sum: u32 = digits
        .bytes()
        .zip(WEIGHTS)
        .map(|(b, w)| (b - b'0') as u32 * w)
        .sum();
    sum % 10 == 0
}

/// UUIDv4Identifier pattern from the schema:
/// `[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}`.
fn is_uetr(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    let hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    for (idx, &b) in bytes.iter().enumerate() {
        match idx {
            8 | 13 | 18 | 23 => {
                if b != b'-' {
                    return false;
                }
            }
            14 => {
                if b != b'4' {
                    return false;
                }
            }
            19 => {
                if !matches!(b, b'8' | b'9' | b'a' | b'b') {
                    return false;
                }
            }
            _ => {
                if !hex(b) {
                    return false;
                }
            }
        }
    }
    true
}

/// XSD `dateTime`: RFC 3339 with offset, `Z`, or no timezone at all.
fn is_iso_date_time(s: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(s).is_ok()
        || chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").is_ok()
}

struct DecimalFacets {
    is_zero: bool,
    fraction_digits: usize,
}

/// Checks the ActiveCurrencyAndAmount facets: non-negative decimal, no sign or
/// exponent, at most 18 total digits and 5 fraction digits.
fn parse_decimal(s: &str) -> Option<DecimalFacets> {
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    if int_part.len() + frac_part.len() > 18 || frac_part.len() > 5 {
        return None;
    }
    Some(DecimalFacets {
        is_zero: s.bytes().all(|b| matches!(b, b'0' | b'.')),
        fraction_digits: frac_part.len(),
    })
}
