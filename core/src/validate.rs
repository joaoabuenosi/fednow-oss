//! Rule validation for parsed ISO 20022 documents (pacs.008, pacs.002).
//!
//! Three rule sources, so a violation can be traced to where the requirement
//! comes from:
//! - [`RuleSource::XsdFacet`] — lexical facets of the message schema (lengths,
//!   patterns, enumerations, numeric limits).
//! - [`RuleSource::IsoRule`] — ISO 20022 cross-field rules the schema cannot
//!   express (e.g. `NbOfTxs` must equal the transaction count).
//! - [`RuleSource::FedNowProfile`] — the FedNow Service profile (USD only,
//!   settlement method CLRG, charge bearer SLEV, ABA routing-number checksum,
//!   cent-precision amounts, mandatory transaction status, reject reasons).
//!
//! All issues are collected; nothing short-circuits.

use crate::pacs008::{ActiveCurrencyAndAmount, CreditTransferTransaction, Document, NAMESPACE};
use crate::{camt029, camt056, envelope, head001, pacs002, pacs004, pacs028};

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
    if !is_fednow_message_id(&hdr.message_identification) {
        issues.push(ValidationIssue::new(
            "fednow.msgid.format",
            "GrpHdr/MsgId",
            format!(
                "'{}' is not a FedNow message id (CCYYMMDD + 9-char connection party id + 1..18-char reference)",
                hdr.message_identification
            ),
            RuleSource::FedNowProfile,
        ));
    }

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
    } else if nb != "1" {
        issues.push(ValidationIssue::new(
            "fednow.nboftxs.one",
            "GrpHdr/NbOfTxs",
            "the FedNow profile fixes NbOfTxs at 1 (one transaction per message)".to_string(),
            RuleSource::FedNowProfile,
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

    let clr_sys_cd = hdr
        .settlement_information
        .clearing_system
        .as_ref()
        .and_then(|c| c.code.as_deref());
    if clr_sys_cd != Some("FDN") {
        issues.push(ValidationIssue::new(
            "fednow.clrsys.fdn",
            "GrpHdr/SttlmInf/ClrSys/Cd",
            format!(
                "the FedNow profile requires ClrSys/Cd 'FDN', found {}",
                clr_sys_cd.unwrap_or("(none)")
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

/// Validate a parsed pacs.002 document, returning every violation found.
///
/// An empty vector means the document passed all implemented checks.
pub fn validate_pacs002(doc: &pacs002::Document) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    if doc.xmlns.as_deref() != Some(pacs002::NAMESPACE) {
        issues.push(ValidationIssue::new(
            "xsd.namespace",
            "Document",
            format!(
                "expected namespace {}, found {}",
                pacs002::NAMESPACE,
                doc.xmlns.as_deref().unwrap_or("(none)")
            ),
            RuleSource::XsdFacet,
        ));
    }

    let msg = &doc.fi_to_fi_payment_status_report;
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

    if let Some(orig) = &msg.original_group_information_and_status {
        check_max35text(
            &mut issues,
            "xsd.orgnlmsgid.length",
            "OrgnlGrpInfAndSts/OrgnlMsgId",
            &orig.original_message_identification,
        );
        check_max35text(
            &mut issues,
            "xsd.orgnlmsgnmid.length",
            "OrgnlGrpInfAndSts/OrgnlMsgNmId",
            &orig.original_message_name_identification,
        );
    }

    for (i, tx) in msg.transaction_information_and_status.iter().enumerate() {
        validate_status_transaction(&mut issues, i, tx);
    }

    issues
}

fn validate_status_transaction(
    issues: &mut Vec<ValidationIssue>,
    i: usize,
    tx: &pacs002::PaymentTransaction,
) {
    let base = format!("TxInfAndSts[{i}]");

    if let Some(e2e) = &tx.original_end_to_end_identification {
        check_max35text(
            issues,
            "xsd.orgnlendtoendid.length",
            format!("{base}/OrgnlEndToEndId"),
            e2e,
        );
    }

    if let Some(uetr) = &tx.original_uetr {
        if !is_uetr(uetr) {
            issues.push(ValidationIssue::new(
                "xsd.uetr.pattern",
                format!("{base}/OrgnlUETR"),
                format!("'{uetr}' is not a lowercase UUID v4 as required by the UUIDv4Identifier pattern"),
                RuleSource::XsdFacet,
            ));
        }
    }

    match tx.transaction_status.as_deref() {
        None => issues.push(ValidationIssue::new(
            "fednow.txsts.required",
            format!("{base}/TxSts"),
            "the FedNow profile requires TxSts on every transaction status entry".to_string(),
            RuleSource::FedNowProfile,
        )),
        Some(status) => {
            // ExternalPaymentTransactionStatus1Code is an external code list;
            // the schema itself only constrains the length.
            if status.is_empty() || status.chars().count() > 4 {
                issues.push(ValidationIssue::new(
                    "xsd.txsts.length",
                    format!("{base}/TxSts"),
                    format!(
                        "'{status}' violates the external status code length (1..4 characters)"
                    ),
                    RuleSource::XsdFacet,
                ));
            } else if !FEDNOW_TX_STATUSES.contains(&status) {
                // Credit-transfer statuses used by the FedNow flows: participant
                // accept/reject (ACTC/RJCT), service advice settled (ACSC), and
                // accept-without-posting (ACWP).
                issues.push(ValidationIssue::new(
                    "fednow.txsts.known",
                    format!("{base}/TxSts"),
                    format!(
                        "'{status}' is not a FedNow credit-transfer status ({})",
                        FEDNOW_TX_STATUSES.join(", ")
                    ),
                    RuleSource::FedNowProfile,
                ));
            }

            if status == "RJCT" && !tx.status_reason_information.iter().any(|s| s.has_reason()) {
                issues.push(ValidationIssue::new(
                    "fednow.rjct.reason",
                    format!("{base}/StsRsnInf"),
                    "a rejection (TxSts RJCT) must carry a status reason code".to_string(),
                    RuleSource::FedNowProfile,
                ));
            }
        }
    }

    for (r, rsn) in tx.status_reason_information.iter().enumerate() {
        if let Some(code) = rsn.reason.as_ref().and_then(|x| x.code.as_deref()) {
            // ExternalStatusReason1Code: external list, schema constrains 1..4 chars.
            if code.is_empty() || code.chars().count() > 4 {
                issues.push(ValidationIssue::new(
                    "xsd.stsrsn.length",
                    format!("{base}/StsRsnInf[{r}]/Rsn/Cd"),
                    format!("'{code}' violates the external reason code length (1..4 characters)"),
                    RuleSource::XsdFacet,
                ));
            }
        }
    }

    if let Some(dt) = &tx.acceptance_date_time {
        if !is_iso_date_time(dt) {
            issues.push(ValidationIssue::new(
                "xsd.accptncdttm.format",
                format!("{base}/AccptncDtTm"),
                format!("'{dt}' is not a valid ISO 8601 date-time"),
                RuleSource::XsdFacet,
            ));
        }
    }
}

/// Transaction statuses used by FedNow credit-transfer flows, as observed
/// across the Release 1 sample set: accepted (ACTC), accepted with creditor
/// account credited (ACCC), settlement completed (ACSC, service advice only),
/// accepted without posting (ACWP), pending (PDNG), blocked (BLCK) and
/// rejected (RJCT).
const FEDNOW_TX_STATUSES: [&str; 7] = ["ACTC", "ACCC", "ACSC", "ACWP", "PDNG", "BLCK", "RJCT"];

/// `PmtTpInf/LclInstrm/Prtry` for customer credit transfers (uniform across
/// every Release 1 sample message).
const FEDNOW_LOCAL_INSTRUMENT: &str = "FDNA";

/// Known `PmtTpInf/CtgyPurp/Prtry` values for customer credit transfers,
/// observed in the Release 1 sample set (consumer / business). The full code
/// list lives in the FedNow Technical Specifications and may extend this.
const FEDNOW_CATEGORY_PURPOSES: [&str; 2] = ["CONS", "BIZZ"];

/// Which side sent a pacs.002 — the two FedNow profiles differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pacs002Direction {
    /// `ParticipantPaymentStatus`: the accept/reject response a participant
    /// sends. FedNow message id, at most one `StsRsnInf` with `Rsn/Cd` only,
    /// no acceptance/settlement timestamps.
    ParticipantToService,
    /// `FedNowPaymentStatus`: the advice the service sends. Plain Max35Text
    /// message id; proprietary reasons and acceptance/settlement timestamps
    /// allowed.
    ServiceToParticipant,
}

/// Validate a pacs.002 against the FedNow profile for the given direction.
///
/// Runs [`validate_pacs002`] first (base facets and common FedNow rules) and
/// layers the direction-specific profile on top.
pub fn validate_pacs002_direction(
    doc: &pacs002::Document,
    direction: Pacs002Direction,
) -> Vec<ValidationIssue> {
    let mut issues = validate_pacs002(doc);
    let msg = &doc.fi_to_fi_payment_status_report;

    if direction == Pacs002Direction::ParticipantToService
        && !is_fednow_message_id(&msg.group_header.message_identification)
    {
        issues.push(ValidationIssue::new(
            "fednow.msgid.format",
            "GrpHdr/MsgId",
            format!(
                "'{}' is not a FedNow message id (CCYYMMDD + 9-char connection party id + 1..18-char reference)",
                msg.group_header.message_identification
            ),
            RuleSource::FedNowProfile,
        ));
    }

    if msg.original_group_information_and_status.is_some() {
        issues.push(ValidationIssue::new(
            "fednow.orgnlgrpinfandsts.absent",
            "OrgnlGrpInfAndSts",
            "the FedNow profiles report per transaction (TxInfAndSts/OrgnlGrpInf), not per group"
                .to_string(),
            RuleSource::FedNowProfile,
        ));
    }

    if msg.transaction_information_and_status.len() != 1 {
        issues.push(ValidationIssue::new(
            "fednow.txinfandsts.one",
            "TxInfAndSts",
            format!(
                "the FedNow profiles carry exactly one TxInfAndSts, found {}",
                msg.transaction_information_and_status.len()
            ),
            RuleSource::FedNowProfile,
        ));
    }

    for (i, tx) in msg.transaction_information_and_status.iter().enumerate() {
        let base = format!("TxInfAndSts[{i}]");

        validate_original_group_information(
            &mut issues,
            &base,
            tx.original_group_information.as_ref(),
        );

        for (agent, code, tag) in [
            (
                &tx.instructing_agent,
                "fednow.instgagt.required",
                "InstgAgt",
            ),
            (&tx.instructed_agent, "fednow.instdagt.required", "InstdAgt"),
        ] {
            match agent {
                None => issues.push(ValidationIssue::new(
                    code,
                    format!("{base}/{tag}"),
                    format!("the FedNow profile requires {tag}"),
                    RuleSource::FedNowProfile,
                )),
                Some(a) => validate_frs_agent(&mut issues, &format!("{base}/{tag}"), a),
            }
        }

        if direction == Pacs002Direction::ParticipantToService {
            if tx.transaction_status.as_deref() == Some("ACSC") {
                // Settlement completion is advised by the service, never
                // reported by a participant (service-only in the sample set).
                issues.push(ValidationIssue::new(
                    "fednow.txsts.participant",
                    format!("{base}/TxSts"),
                    "participants do not send ACSC; settlement completion is a service advice"
                        .to_string(),
                    RuleSource::FedNowProfile,
                ));
            }
            if tx.status_reason_information.len() > 1 {
                issues.push(ValidationIssue::new(
                    "fednow.stsrsninf.one",
                    format!("{base}/StsRsnInf"),
                    "the participant status carries at most one StsRsnInf".to_string(),
                    RuleSource::FedNowProfile,
                ));
            }
            if tx
                .status_reason_information
                .iter()
                .any(|s| s.reason.as_ref().is_some_and(|r| r.proprietary.is_some()))
            {
                issues.push(ValidationIssue::new(
                    "fednow.stsrsn.cd",
                    format!("{base}/StsRsnInf/Rsn"),
                    "the participant status uses Rsn/Cd only (no proprietary reasons)".to_string(),
                    RuleSource::FedNowProfile,
                ));
            }
            if tx.acceptance_date_time.is_some() {
                issues.push(ValidationIssue::new(
                    "fednow.accptncdttm.absent",
                    format!("{base}/AccptncDtTm"),
                    "AccptncDtTm is not part of the participant status profile".to_string(),
                    RuleSource::FedNowProfile,
                ));
            }
            if tx.effective_interbank_settlement_date.is_some() {
                issues.push(ValidationIssue::new(
                    "fednow.fctvdt.absent",
                    format!("{base}/FctvIntrBkSttlmDt"),
                    "FctvIntrBkSttlmDt is not part of the participant status profile".to_string(),
                    RuleSource::FedNowProfile,
                ));
            }
        }
    }

    issues
}

fn validate_original_group_information(
    issues: &mut Vec<ValidationIssue>,
    base: &str,
    orig: Option<&pacs002::OriginalGroupInformation>,
) {
    match orig {
        None => issues.push(ValidationIssue::new(
            "fednow.orgnlgrpinf.required",
            format!("{base}/OrgnlGrpInf"),
            "the FedNow profile requires OrgnlGrpInf identifying the original message".to_string(),
            RuleSource::FedNowProfile,
        )),
        Some(o) => {
            check_max35text(
                issues,
                "xsd.orgnlmsgid.length",
                format!("{base}/OrgnlGrpInf/OrgnlMsgId"),
                &o.original_message_identification,
            );
            if !is_message_definition_identifier(&o.original_message_name_identification)
                || o.original_message_name_identification.split('.').nth(2) != Some("001")
            {
                issues.push(ValidationIssue::new(
                    "fednow.orgnlmsgnmid.format",
                    format!("{base}/OrgnlGrpInf/OrgnlMsgNmId"),
                    format!(
                        "'{}' does not follow the FRS message name pattern (aaaa.nnn.001.nn)",
                        o.original_message_name_identification
                    ),
                    RuleSource::FedNowProfile,
                ));
            }
            match &o.original_creation_date_time {
                None => issues.push(ValidationIssue::new(
                    "fednow.orgnlcredttm.required",
                    format!("{base}/OrgnlGrpInf/OrgnlCreDtTm"),
                    "the FedNow profile requires OrgnlCreDtTm".to_string(),
                    RuleSource::FedNowProfile,
                )),
                Some(dt) if !is_iso_date_time(dt) => issues.push(ValidationIssue::new(
                    "xsd.orgnlcredttm.format",
                    format!("{base}/OrgnlGrpInf/OrgnlCreDtTm"),
                    format!("'{dt}' is not a valid ISO 8601 date-time"),
                    RuleSource::XsdFacet,
                )),
                Some(_) => {}
            }
        }
    }
}

/// Validate a parsed pacs.004 payment return.
///
/// Enforces the base XSD facets plus the FedNow-shared lexical rules (message
/// id shape, single transaction, FDN clearing system, USABA agents, USD cent
/// amounts, original-message identification). The full Release 1 profile is
/// pending calibration against the PaymentReturn usage-guideline export.
pub fn validate_pacs004(doc: &pacs004::Document) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    if doc.xmlns.as_deref() != Some(pacs004::NAMESPACE) {
        issues.push(ValidationIssue::new(
            "xsd.namespace",
            "Document",
            format!(
                "expected namespace {}, found {}",
                pacs004::NAMESPACE,
                doc.xmlns.as_deref().unwrap_or("(none)")
            ),
            RuleSource::XsdFacet,
        ));
    }

    let msg = &doc.payment_return;
    let hdr = &msg.group_header;

    check_max35text(
        &mut issues,
        "xsd.msgid.length",
        "GrpHdr/MsgId",
        &hdr.message_identification,
    );
    if !is_fednow_message_id(&hdr.message_identification) {
        issues.push(ValidationIssue::new(
            "fednow.msgid.format",
            "GrpHdr/MsgId",
            format!(
                "'{}' is not a FedNow message id (CCYYMMDD + 9-char connection party id + 1..18-char reference)",
                hdr.message_identification
            ),
            RuleSource::FedNowProfile,
        ));
    }

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

    if hdr.number_of_transactions != "1" {
        issues.push(ValidationIssue::new(
            "fednow.nboftxs.one",
            "GrpHdr/NbOfTxs",
            "the FedNow flows carry one returned transaction per message".to_string(),
            RuleSource::FedNowProfile,
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
    let clr_sys_cd = hdr
        .settlement_information
        .clearing_system
        .as_ref()
        .and_then(|c| c.code.as_deref());
    if clr_sys_cd != Some("FDN") {
        issues.push(ValidationIssue::new(
            "fednow.clrsys.fdn",
            "GrpHdr/SttlmInf/ClrSys/Cd",
            format!(
                "the FedNow profile requires ClrSys/Cd 'FDN', found {}",
                clr_sys_cd.unwrap_or("(none)")
            ),
            RuleSource::FedNowProfile,
        ));
    }

    if msg.transaction_information.len() != 1 {
        issues.push(ValidationIssue::new(
            "fednow.txinf.one",
            "TxInf",
            format!(
                "expected exactly one TxInf, found {}",
                msg.transaction_information.len()
            ),
            RuleSource::FedNowProfile,
        ));
    }

    for (i, tx) in msg.transaction_information.iter().enumerate() {
        let base = format!("TxInf[{i}]");

        validate_original_group_information(
            &mut issues,
            &base,
            tx.original_group_information.as_ref(),
        );

        if let Some(uetr) = &tx.original_uetr {
            if !is_uetr(uetr) {
                issues.push(ValidationIssue::new(
                    "xsd.uetr.pattern",
                    format!("{base}/OrgnlUETR"),
                    format!("'{uetr}' is not a lowercase UUID v4 as required by the UUIDv4Identifier pattern"),
                    RuleSource::XsdFacet,
                ));
            }
        }

        validate_amount(
            &mut issues,
            &format!("{base}/RtrdIntrBkSttlmAmt"),
            &tx.returned_interbank_settlement_amount,
        );

        for (agent, code, tag) in [
            (
                &tx.instructing_agent,
                "fednow.instgagt.required",
                "InstgAgt",
            ),
            (&tx.instructed_agent, "fednow.instdagt.required", "InstdAgt"),
        ] {
            match agent {
                None => issues.push(ValidationIssue::new(
                    code,
                    format!("{base}/{tag}"),
                    format!("the FedNow flows require {tag}"),
                    RuleSource::FedNowProfile,
                )),
                Some(a) => validate_frs_agent(&mut issues, &format!("{base}/{tag}"), a),
            }
        }

        if !tx.return_reason_information.iter().any(|r| {
            r.reason
                .as_ref()
                .is_some_and(|x| x.code.is_some() || x.proprietary.is_some())
        }) {
            issues.push(ValidationIssue::new(
                "fednow.rtrrsn.required",
                format!("{base}/RtrRsnInf"),
                "a payment return must carry a return reason".to_string(),
                RuleSource::FedNowProfile,
            ));
        }
        if tx.return_reason_information.len() > 1 {
            issues.push(ValidationIssue::new(
                "fednow.rtrrsninf.one",
                format!("{base}/RtrRsnInf"),
                "the FedNow profile carries exactly one RtrRsnInf".to_string(),
                RuleSource::FedNowProfile,
            ));
        }
        if tx
            .return_reason_information
            .iter()
            .any(|r| r.reason.as_ref().is_some_and(|x| x.proprietary.is_some()))
        {
            // ReturnReason5Choice in the FedNow profile is Cd-only.
            issues.push(ValidationIssue::new(
                "fednow.rtrrsn.cd",
                format!("{base}/RtrRsnInf/Rsn"),
                "return reasons use external codes only (Rsn/Cd, no Prtry)".to_string(),
                RuleSource::FedNowProfile,
            ));
        }

        // Profile-mandatory elements beyond the base schema.
        for (present, code, tag) in [
            (
                tx.original_interbank_settlement_amount.is_some(),
                "fednow.orgnlamt.required",
                "OrgnlIntrBkSttlmAmt",
            ),
            (
                tx.original_interbank_settlement_date.is_some(),
                "fednow.orgnlsttlmdt.required",
                "OrgnlIntrBkSttlmDt",
            ),
            (
                tx.interbank_settlement_date.is_some(),
                "fednow.intrbksttlmdt.required",
                "IntrBkSttlmDt",
            ),
            (
                tx.return_chain.is_some(),
                "fednow.rtrchain.required",
                "RtrChain",
            ),
            (
                tx.original_transaction_reference.is_some(),
                "fednow.orgnltxref.required",
                "OrgnlTxRef",
            ),
        ] {
            if !present {
                issues.push(ValidationIssue::new(
                    code,
                    format!("{base}/{tag}"),
                    format!("the FedNow profile requires {tag}"),
                    RuleSource::FedNowProfile,
                ));
            }
        }
        if let Some(amount) = &tx.original_interbank_settlement_amount {
            validate_amount(&mut issues, &format!("{base}/OrgnlIntrBkSttlmAmt"), amount);
        }
        match tx.charge_bearer.as_deref() {
            Some("SLEV") => {}
            Some(other) => issues.push(ValidationIssue::new(
                "fednow.chrgbr.slev",
                format!("{base}/ChrgBr"),
                format!("the FedNow profile fixes ChrgBr at SLEV, found '{other}'"),
                RuleSource::FedNowProfile,
            )),
            None => issues.push(ValidationIssue::new(
                "fednow.chrgbr.slev",
                format!("{base}/ChrgBr"),
                "the FedNow profile requires ChrgBr SLEV".to_string(),
                RuleSource::FedNowProfile,
            )),
        }
    }

    issues
}

/// camt.029 confirmation codes used by the FedNow Release 1 profile.
const FEDNOW_CONFIRMATIONS: [&str; 4] = ["IPAY", "RJCR", "PDCR", "PECR"];

/// Validate a parsed camt.056 return request against the FedNow profile.
pub fn validate_camt056(doc: &camt056::Document) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    if doc.xmlns.as_deref() != Some(camt056::NAMESPACE) {
        issues.push(ValidationIssue::new(
            "xsd.namespace",
            "Document",
            format!(
                "expected namespace {}, found {}",
                camt056::NAMESPACE,
                doc.xmlns.as_deref().unwrap_or("(none)")
            ),
            RuleSource::XsdFacet,
        ));
    }

    let msg = &doc.cancellation_request;
    validate_case_assignment(&mut issues, &msg.assignment);
    if msg.case.is_none() {
        issues.push(ValidationIssue::new(
            "fednow.case.required",
            "FIToFIPmtCxlReq/Case",
            "the FedNow profile requires the Case block".to_string(),
            RuleSource::FedNowProfile,
        ));
    }

    let txs: Vec<&camt056::PaymentTransaction> = msg
        .underlying
        .iter()
        .flat_map(|u| u.transaction_information.iter())
        .collect();
    if txs.len() != 1 {
        issues.push(ValidationIssue::new(
            "fednow.txinf.one",
            "Undrlyg/TxInf",
            format!("expected exactly one TxInf, found {}", txs.len()),
            RuleSource::FedNowProfile,
        ));
    }
    for (i, tx) in txs.iter().enumerate() {
        let base = format!("Undrlyg/TxInf[{i}]");
        validate_original_group_information(
            &mut issues,
            &base,
            tx.original_group_information.as_ref(),
        );
        for (present, code, tag) in [
            (
                tx.original_interbank_settlement_amount.is_some(),
                "fednow.orgnlamt.required",
                "OrgnlIntrBkSttlmAmt",
            ),
            (
                tx.original_interbank_settlement_date.is_some(),
                "fednow.orgnlsttlmdt.required",
                "OrgnlIntrBkSttlmDt",
            ),
        ] {
            if !present {
                issues.push(ValidationIssue::new(
                    code,
                    format!("{base}/{tag}"),
                    format!("the FedNow profile requires {tag}"),
                    RuleSource::FedNowProfile,
                ));
            }
        }
        if let Some(uetr) = &tx.original_uetr {
            if !is_uetr(uetr) {
                issues.push(ValidationIssue::new(
                    "xsd.uetr.pattern",
                    format!("{base}/OrgnlUETR"),
                    format!("'{uetr}' is not a lowercase UUID v4"),
                    RuleSource::XsdFacet,
                ));
            }
        }
        if !tx.cancellation_reason_information.iter().any(|r| {
            r.reason
                .as_ref()
                .is_some_and(|x| x.code.is_some() || x.proprietary.is_some())
        }) {
            issues.push(ValidationIssue::new(
                "fednow.cxlrsn.required",
                format!("{base}/CxlRsnInf"),
                "a return request must carry a cancellation reason".to_string(),
                RuleSource::FedNowProfile,
            ));
        }
        if tx
            .cancellation_reason_information
            .iter()
            .any(|r| r.reason.as_ref().is_some_and(|x| x.proprietary.is_some()))
        {
            issues.push(ValidationIssue::new(
                "fednow.cxlrsn.cd",
                format!("{base}/CxlRsnInf/Rsn"),
                "cancellation reasons use external codes only (Rsn/Cd, no Prtry)".to_string(),
                RuleSource::FedNowProfile,
            ));
        }
    }

    issues
}

/// Validate a parsed camt.029 return request response against the FedNow profile.
pub fn validate_camt029(doc: &camt029::Document) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    if doc.xmlns.as_deref() != Some(camt029::NAMESPACE) {
        issues.push(ValidationIssue::new(
            "xsd.namespace",
            "Document",
            format!(
                "expected namespace {}, found {}",
                camt029::NAMESPACE,
                doc.xmlns.as_deref().unwrap_or("(none)")
            ),
            RuleSource::XsdFacet,
        ));
    }

    let msg = &doc.resolution;
    validate_case_assignment(&mut issues, &msg.assignment);
    if msg.resolved_case.is_none() {
        issues.push(ValidationIssue::new(
            "fednow.rslvdcase.required",
            "RsltnOfInvstgtn/RslvdCase",
            "the FedNow profile requires RslvdCase".to_string(),
            RuleSource::FedNowProfile,
        ));
    }

    let conf = msg.status.confirmation.as_deref();
    match conf {
        None => issues.push(ValidationIssue::new(
            "fednow.conf.required",
            "RsltnOfInvstgtn/Sts/Conf",
            "the FedNow profile requires Sts/Conf".to_string(),
            RuleSource::FedNowProfile,
        )),
        Some(c) if !FEDNOW_CONFIRMATIONS.contains(&c) => issues.push(ValidationIssue::new(
            "fednow.conf.known",
            "RsltnOfInvstgtn/Sts/Conf",
            format!(
                "'{c}' is not a FedNow confirmation ({})",
                FEDNOW_CONFIRMATIONS.join(", ")
            ),
            RuleSource::FedNowProfile,
        )),
        Some(_) => {}
    }

    // A rejected return request must say why.
    if conf == Some("RJCR") {
        let has_reason = msg
            .cancellation_details
            .iter()
            .flat_map(|d| d.transaction_information.iter())
            .flat_map(|t| t.cancellation_status_reason_information.iter())
            .any(|r| {
                r.reason
                    .as_ref()
                    .is_some_and(|x| x.code.is_some() || x.proprietary.is_some())
            });
        if !has_reason {
            issues.push(ValidationIssue::new(
                "fednow.rjcr.reason",
                "CxlDtls/TxInfAndSts/CxlStsRsnInf",
                "a rejected return request (RJCR) must carry a status reason".to_string(),
                RuleSource::FedNowProfile,
            ));
        }
    }

    issues
}

fn validate_case_assignment(
    issues: &mut Vec<ValidationIssue>,
    assignment: &camt056::CaseAssignment,
) {
    check_max35text(
        issues,
        "xsd.msgid.length",
        "Assgnmt/Id",
        &assignment.identification,
    );
    if !is_fednow_message_id(&assignment.identification) {
        issues.push(ValidationIssue::new(
            "fednow.msgid.format",
            "Assgnmt/Id",
            format!(
                "'{}' is not a FedNow message id (CCYYMMDD + 9-char connection party id + 1..18-char reference)",
                assignment.identification
            ),
            RuleSource::FedNowProfile,
        ));
    }
    if !is_iso_date_time(&assignment.creation_date_time) {
        issues.push(ValidationIssue::new(
            "xsd.credttm.format",
            "Assgnmt/CreDtTm",
            format!(
                "'{}' is not a valid ISO 8601 date-time",
                assignment.creation_date_time
            ),
            RuleSource::XsdFacet,
        ));
    }
    for (party, tag) in [
        (&assignment.assigner, "Assgnr"),
        (&assignment.assignee, "Assgne"),
    ] {
        match &party.agent {
            None => issues.push(ValidationIssue::new(
                "fednow.assignment.agent",
                format!("Assgnmt/{tag}"),
                format!("the FedNow profile identifies {tag} as an agent (Agt)"),
                RuleSource::FedNowProfile,
            )),
            Some(a) => validate_frs_agent(issues, &format!("Assgnmt/{tag}/Agt"), a),
        }
    }
}

/// Validate a parsed pacs.028 payment status request against the FedNow profile.
///
/// An empty vector means the document passed all implemented checks.
pub fn validate_pacs028(doc: &pacs028::Document) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    if doc.xmlns.as_deref() != Some(pacs028::NAMESPACE) {
        issues.push(ValidationIssue::new(
            "xsd.namespace",
            "Document",
            format!(
                "expected namespace {}, found {}",
                pacs028::NAMESPACE,
                doc.xmlns.as_deref().unwrap_or("(none)")
            ),
            RuleSource::XsdFacet,
        ));
    }

    let msg = &doc.fi_to_fi_payment_status_request;
    let hdr = &msg.group_header;

    check_max35text(
        &mut issues,
        "xsd.msgid.length",
        "GrpHdr/MsgId",
        &hdr.message_identification,
    );
    if !is_fednow_message_id(&hdr.message_identification) {
        issues.push(ValidationIssue::new(
            "fednow.msgid.format",
            "GrpHdr/MsgId",
            format!(
                "'{}' is not a FedNow message id (CCYYMMDD + 9-char connection party id + 1..18-char reference)",
                hdr.message_identification
            ),
            RuleSource::FedNowProfile,
        ));
    }

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

    if msg.transaction_information.len() != 1 {
        issues.push(ValidationIssue::new(
            "fednow.txinf.one",
            "TxInf",
            format!(
                "the FedNow profile carries exactly one TxInf, found {}",
                msg.transaction_information.len()
            ),
            RuleSource::FedNowProfile,
        ));
    }

    for (i, tx) in msg.transaction_information.iter().enumerate() {
        let base = format!("TxInf[{i}]");

        validate_original_group_information(
            &mut issues,
            &base,
            tx.original_group_information.as_ref(),
        );

        if let Some(uetr) = &tx.original_uetr {
            if !is_uetr(uetr) {
                issues.push(ValidationIssue::new(
                    "xsd.uetr.pattern",
                    format!("{base}/OrgnlUETR"),
                    format!("'{uetr}' is not a lowercase UUID v4 as required by the UUIDv4Identifier pattern"),
                    RuleSource::XsdFacet,
                ));
            }
        }

        for (agent, code, tag) in [
            (
                &tx.instructing_agent,
                "fednow.instgagt.required",
                "InstgAgt",
            ),
            (&tx.instructed_agent, "fednow.instdagt.required", "InstdAgt"),
        ] {
            match agent {
                None => issues.push(ValidationIssue::new(
                    code,
                    format!("{base}/{tag}"),
                    format!("the FedNow profile requires {tag}"),
                    RuleSource::FedNowProfile,
                )),
                Some(a) => validate_frs_agent(&mut issues, &format!("{base}/{tag}"), a),
            }
        }
    }

    issues
}

/// Validate a parsed head.001.001.02 Business Application Header, returning
/// every violation found.
///
/// An empty vector means the header passed all implemented checks.
pub fn validate_head001(hdr: &head001::AppHdr) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    if hdr.xmlns.as_deref() != Some(head001::NAMESPACE) {
        issues.push(ValidationIssue::new(
            "xsd.namespace",
            "AppHdr",
            format!(
                "expected namespace {}, found {}",
                head001::NAMESPACE,
                hdr.xmlns.as_deref().unwrap_or("(none)")
            ),
            RuleSource::XsdFacet,
        ));
    }

    check_max35text(
        &mut issues,
        "xsd.bizmsgidr.length",
        "AppHdr/BizMsgIdr",
        &hdr.business_message_identifier,
    );

    check_max35text(
        &mut issues,
        "xsd.msgdefidr.length",
        "AppHdr/MsgDefIdr",
        &hdr.message_definition_identifier,
    );
    if !is_message_definition_identifier(&hdr.message_definition_identifier) {
        issues.push(ValidationIssue::new(
            "iso.msgdefidr.format",
            "AppHdr/MsgDefIdr",
            format!(
                "'{}' does not follow the ISO 20022 message identifier convention (e.g. pacs.008.001.08)",
                hdr.message_definition_identifier
            ),
            RuleSource::IsoRule,
        ));
    } else if hdr.message_definition_identifier.split('.').nth(2) != Some("001") {
        // Federal Reserve Financial Services fix the message variant at 001.
        issues.push(ValidationIssue::new(
            "fednow.msgdefidr.variant",
            "AppHdr/MsgDefIdr",
            format!(
                "'{}' has a message variant other than 001, which FedNow does not use",
                hdr.message_definition_identifier
            ),
            RuleSource::FedNowProfile,
        ));
    }

    match &hdr.market_practice {
        None => issues.push(ValidationIssue::new(
            "fednow.mktprctc.required",
            "AppHdr/MktPrctc",
            "the FedNow profile requires MktPrctc (market practice) on every BAH".to_string(),
            RuleSource::FedNowProfile,
        )),
        Some(mp) => {
            if mp.registry != FEDNOW_MARKET_PRACTICE_REGISTRY {
                issues.push(ValidationIssue::new(
                    "fednow.mktprctc.regy",
                    "AppHdr/MktPrctc/Regy",
                    format!(
                        "registry must be '{FEDNOW_MARKET_PRACTICE_REGISTRY}', found '{}'",
                        mp.registry
                    ),
                    RuleSource::FedNowProfile,
                ));
            }
            if !is_fednow_market_practice_id(&mp.identification) {
                issues.push(ValidationIssue::new(
                    "fednow.mktprctc.id",
                    "AppHdr/MktPrctc/Id",
                    format!(
                        "'{}' does not match the FedNow market practice identifier (frb.fednow[.xxx].01)",
                        mp.identification
                    ),
                    RuleSource::FedNowProfile,
                ));
            }
        }
    }

    if hdr.signature.is_some() {
        // The FedNow profile removes Sgntr from the BAH; signatures travel
        // outside the XML business message.
        issues.push(ValidationIssue::new(
            "fednow.sgntr.outofband",
            "AppHdr/Sgntr",
            "the FedNow profile does not use the BAH signature envelope".to_string(),
            RuleSource::FedNowProfile,
        ));
    }

    // CreationDateRule: UTC or local time with UTC offset — a timezone is
    // required either way (RFC 3339 parse implies one is present).
    if chrono::DateTime::parse_from_rfc3339(&hdr.creation_date).is_err() {
        if is_iso_date_time(&hdr.creation_date) {
            issues.push(ValidationIssue::new(
                "fednow.credt.timezone",
                "AppHdr/CreDt",
                format!(
                    "CreDt must be UTC (Z) or local time with a UTC offset, found '{}'",
                    hdr.creation_date
                ),
                RuleSource::FedNowProfile,
            ));
        } else {
            issues.push(ValidationIssue::new(
                "xsd.credt.format",
                "AppHdr/CreDt",
                format!("'{}' is not a valid ISO 8601 date-time", hdr.creation_date),
                RuleSource::XsdFacet,
            ));
        }
    }

    if hdr.business_service.is_some() {
        // BusinessServiceRule: no codes are defined in Release 1.
        issues.push(ValidationIssue::new(
            "fednow.bizsvc.absent",
            "AppHdr/BizSvc",
            "BizSvc must not be used in FedNow Service Release 1 (no codes defined)".to_string(),
            RuleSource::FedNowProfile,
        ));
    }

    if let Some(cpy) = &hdr.copy_duplicate {
        if !["CODU", "COPY", "DUPL"].contains(&cpy.as_str()) {
            issues.push(ValidationIssue::new(
                "xsd.cpydplct.enum",
                "AppHdr/CpyDplct",
                format!("'{cpy}' is not one of CODU, COPY, DUPL"),
                RuleSource::XsdFacet,
            ));
        } else if cpy != "DUPL" {
            issues.push(ValidationIssue::new(
                "fednow.cpydplct.dupl",
                "AppHdr/CpyDplct",
                format!("the FedNow profile restricts CpyDplct to DUPL, found '{cpy}'"),
                RuleSource::FedNowProfile,
            ));
        }
    }

    for (party, tag) in [(&hdr.from, "Fr"), (&hdr.to, "To")] {
        validate_party44(&mut issues, tag, party);
    }

    // Direction-dependent rules, keyed on the FedNow Service application
    // identifier: participant-sent headers address the service, and the
    // service-only elements must be absent.
    let from_id = party_member_id(&hdr.from);
    let to_id = party_member_id(&hdr.to);
    if from_id != Some(FEDNOW_SERVICE_CONNECTION_PARTY) {
        if to_id != Some(FEDNOW_SERVICE_CONNECTION_PARTY) {
            issues.push(ValidationIssue::new(
                "fednow.to.service",
                "AppHdr/To",
                format!(
                    "participant-sent messages address the FedNow Service application \
                     ({FEDNOW_SERVICE_CONNECTION_PARTY}) in To"
                ),
                RuleSource::FedNowProfile,
            ));
        }
        if hdr.business_processing_date.is_some() {
            issues.push(ValidationIssue::new(
                "fednow.bizprcgdt.serviceonly",
                "AppHdr/BizPrcgDt",
                "BizPrcgDt is only present in service-delivered messages".to_string(),
                RuleSource::FedNowProfile,
            ));
        }
        if hdr.copy_duplicate.is_some() {
            issues.push(ValidationIssue::new(
                "fednow.cpydplct.serviceonly",
                "AppHdr/CpyDplct",
                "CpyDplct is only present when the service delivers a retrieved message"
                    .to_string(),
                RuleSource::FedNowProfile,
            ));
        }
        if !hdr.related.is_empty() {
            issues.push(ValidationIssue::new(
                "fednow.rltd.serviceonly",
                "AppHdr/Rltd",
                "Rltd is only present in service-delivered retrieval responses".to_string(),
                RuleSource::FedNowProfile,
            ));
        }
    }
    if hdr.related.len() > 1 {
        issues.push(ValidationIssue::new(
            "fednow.rltd.one",
            "AppHdr/Rltd",
            format!(
                "the FedNow profile restricts Rltd to at most 1, found {}",
                hdr.related.len()
            ),
            RuleSource::FedNowProfile,
        ));
    }

    // MarketPracticeIdentificationRule: the id must match the enclosed message.
    if let Some(mp) = &hdr.market_practice {
        if is_fednow_market_practice_id(&mp.identification) {
            let expected: &[&str] = match hdr.message_definition_identifier.as_str() {
                "camt.029.001.09" => &[
                    "frb.fednow.rrr.01",
                    "frb.fednow.irr.01",
                    "frb.fednow.rcr.01",
                ],
                "camt.052.001.08" => &[
                    "frb.fednow.aat.01",
                    "frb.fednow.aad.01",
                    "frb.fednow.aba.01",
                ],
                _ => &["frb.fednow.01"],
            };
            if !expected.contains(&mp.identification.as_str()) {
                issues.push(ValidationIssue::new(
                    "fednow.mktprctc.match",
                    "AppHdr/MktPrctc/Id",
                    format!(
                        "'{}' does not match the guideline for {} (expected one of: {})",
                        mp.identification,
                        hdr.message_definition_identifier,
                        expected.join(", ")
                    ),
                    RuleSource::FedNowProfile,
                ));
            }
        }
    }

    issues
}

/// Validate a parsed FedNow technical envelope: envelope-level rules, the
/// BAH, the business document, and the cross-checks between them.
///
/// pacs.002 content is validated for the direction implied by the envelope
/// (`FedNowIncoming` → participant-sent, `FedNowOutgoing` → service advice).
pub fn validate_envelope(env: &envelope::Envelope) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    let expected_ns = env.direction.namespace();
    if env.root_namespace.as_deref() != Some(expected_ns) {
        issues.push(ValidationIssue::new(
            "fednow.env.namespace",
            env.direction.root_element(),
            format!(
                "expected namespace {expected_ns}, found {}",
                env.root_namespace.as_deref().unwrap_or("(none)")
            ),
            RuleSource::FedNowProfile,
        ));
    }

    let doc_ns = env.document.namespace();
    match envelope::namespace_for_wrapper(&env.wrapper) {
        None => issues.push(ValidationIssue::new(
            "fednow.env.wrapper.unknown",
            env.direction.message_element(),
            format!("'{}' is not a modeled FedNow message wrapper", env.wrapper),
            RuleSource::FedNowProfile,
        )),
        Some(expected) if expected != doc_ns => issues.push(ValidationIssue::new(
            "fednow.env.wrapper.match",
            env.direction.message_element(),
            format!(
                "wrapper '{}' must enclose a {} Document, found {}",
                env.wrapper,
                expected.rsplit(':').next().unwrap_or(expected),
                env.document.message_name()
            ),
            RuleSource::FedNowProfile,
        )),
        Some(_) => {}
    }

    if env.header.message_definition_identifier != env.document.message_name() {
        issues.push(ValidationIssue::new(
            "fednow.env.msgdefidr.match",
            "AppHdr/MsgDefIdr",
            format!(
                "MsgDefIdr '{}' does not name the enclosed Document ({})",
                env.header.message_definition_identifier,
                env.document.message_name()
            ),
            RuleSource::FedNowProfile,
        ));
    }

    // Direction cross-check: an outgoing envelope carries a service-sent BAH.
    // (The participant-sent side — To must address the service — is already
    // enforced by validate_head001.)
    if env.direction == envelope::Direction::Outgoing
        && party_member_id(&env.header.from) != Some(FEDNOW_SERVICE_CONNECTION_PARTY)
    {
        issues.push(ValidationIssue::new(
            "fednow.env.fr.service",
            "AppHdr/Fr",
            format!(
                "FedNowOutgoing messages are sent by the FedNow Service application \
                 ({FEDNOW_SERVICE_CONNECTION_PARTY}) in Fr"
            ),
            RuleSource::FedNowProfile,
        ));
    }

    issues.extend(validate_head001(&env.header));

    match &env.document {
        envelope::EnvelopedDocument::CustomerCreditTransfer(d) => {
            issues.extend(validate_pacs008(d))
        }
        envelope::EnvelopedDocument::PaymentStatus(d) => {
            let dir = match env.direction {
                envelope::Direction::Incoming => Pacs002Direction::ParticipantToService,
                envelope::Direction::Outgoing => Pacs002Direction::ServiceToParticipant,
            };
            issues.extend(validate_pacs002_direction(d, dir));
        }
        envelope::EnvelopedDocument::PaymentStatusRequest(d) => issues.extend(validate_pacs028(d)),
        envelope::EnvelopedDocument::PaymentReturn(d) => issues.extend(validate_pacs004(d)),
        envelope::EnvelopedDocument::ReturnRequest(d) => issues.extend(validate_camt056(d)),
        envelope::EnvelopedDocument::ReturnRequestResponse(d) => issues.extend(validate_camt029(d)),
    }

    issues
}

/// The FedNow Service application connection party identifier.
const FEDNOW_SERVICE_CONNECTION_PARTY: &str = "021150706";

fn party_member_id(party: &head001::Party44Choice) -> Option<&str> {
    party
        .financial_institution
        .as_ref()?
        .financial_institution_identification
        .clearing_system_member_identification
        .as_ref()
        .map(|m| m.member_identification.as_str())
}

fn validate_party44(issues: &mut Vec<ValidationIssue>, tag: &str, party: &head001::Party44Choice) {
    match (&party.organisation, &party.financial_institution) {
        (Some(_), Some(_)) | (None, None) => {
            issues.push(ValidationIssue::new(
                "xsd.party44.choice",
                format!("AppHdr/{tag}"),
                "Party44Choice requires exactly one of OrgId or FIId".to_string(),
                RuleSource::XsdFacet,
            ));
        }
        (Some(_), None) => {
            // Schema-valid, but FedNow participants are financial institutions
            // addressed by routing number.
            issues.push(ValidationIssue::new(
                "fednow.party.fiid",
                format!("AppHdr/{tag}"),
                "FedNow addresses participants via FIId (routing number), not OrgId".to_string(),
                RuleSource::FedNowProfile,
            ));
        }
        (None, Some(fi)) => {
            if let Some(member) = &fi
                .financial_institution_identification
                .clearing_system_member_identification
            {
                if member.clearing_system_identification.is_some() {
                    // The FedNow BAH profile strips ClrSysId; only MmbId remains.
                    issues.push(ValidationIssue::new(
                        "fednow.party.clrsysid",
                        format!("AppHdr/{tag}/FIId/FinInstnId/ClrSysMmbId/ClrSysId"),
                        "the FedNow BAH carries only MmbId inside ClrSysMmbId (no ClrSysId)"
                            .to_string(),
                        RuleSource::FedNowProfile,
                    ));
                }
                validate_connection_party_id(
                    issues,
                    format!("AppHdr/{tag}/FIId/FinInstnId/ClrSysMmbId/MmbId"),
                    &member.member_identification,
                );
            }
        }
    }
}

/// FedNow BAH `MmbId` is a Connection Party Identifier: 9 uppercase
/// alphanumerics (a routing number, an ETI, or a FedNow-assigned id). When it is
/// all digits it is a routing number and the ABA check digit must hold.
fn validate_connection_party_id(issues: &mut Vec<ValidationIssue>, path: String, id: &str) {
    let is_conn_party = id.len() == 9
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase());
    if !is_conn_party {
        issues.push(ValidationIssue::new(
            "fednow.connparty.format",
            path,
            format!("'{id}' is not a 9-character connection party identifier ([A-Z0-9]{{9}})"),
            RuleSource::FedNowProfile,
        ));
    } else if id.bytes().all(|b| b.is_ascii_digit()) && !aba_checksum_ok(id) {
        issues.push(ValidationIssue::new(
            "fednow.aba.checksum",
            path,
            format!("'{id}' fails the ABA routing-number check digit (weights 3-7-1, mod 10)"),
            RuleSource::FedNowProfile,
        ));
    }
}

/// Fixed `MktPrctc/Regy` value in the FedNow profile.
pub const FEDNOW_MARKET_PRACTICE_REGISTRY: &str =
    "www2.swift.com/mystandards/#/group/Federal_Reserve_Financial_Services/FedNow_Service";

/// FedNow `MktPrctc/Id`: `frb.fednow.01` or `frb.fednow.<3 lowercase letters>.01`.
fn is_fednow_market_practice_id(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    match parts.as_slice() {
        ["frb", "fednow", "01"] => true,
        ["frb", "fednow", ctx, "01"] => {
            ctx.len() == 3 && ctx.bytes().all(|b| b.is_ascii_lowercase())
        }
        _ => false,
    }
}

/// ISO 20022 message identifier convention: `aaaa.nnn.nnn.nn`.
fn is_message_definition_identifier(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts[0].len() == 4
        && parts[0].bytes().all(|b| b.is_ascii_lowercase())
        && parts[1].len() == 3
        && parts[1].bytes().all(|b| b.is_ascii_digit())
        && parts[2].len() == 3
        && parts[2].bytes().all(|b| b.is_ascii_digit())
        && parts[3].len() == 2
        && parts[3].bytes().all(|b| b.is_ascii_digit())
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

    match &tx.payment_type_information {
        None => issues.push(ValidationIssue::new(
            "fednow.pmttpinf.required",
            format!("{base}/PmtTpInf"),
            "the FedNow profile requires PmtTpInf with LclInstrm and CtgyPurp".to_string(),
            RuleSource::FedNowProfile,
        )),
        Some(pt) => {
            match pt
                .local_instrument
                .as_ref()
                .and_then(|c| c.proprietary.as_deref())
            {
                None => issues.push(ValidationIssue::new(
                    "fednow.pmttpinf.lclinstrm",
                    format!("{base}/PmtTpInf/LclInstrm"),
                    "the FedNow profile requires LclInstrm/Prtry".to_string(),
                    RuleSource::FedNowProfile,
                )),
                Some(v) if v != FEDNOW_LOCAL_INSTRUMENT => issues.push(ValidationIssue::new(
                    "fednow.lclinstrm.fdna",
                    format!("{base}/PmtTpInf/LclInstrm/Prtry"),
                    format!(
                        "customer credit transfers use LclInstrm/Prtry '{FEDNOW_LOCAL_INSTRUMENT}', found '{v}'"
                    ),
                    RuleSource::FedNowProfile,
                )),
                Some(_) => {}
            }
            match pt
                .category_purpose
                .as_ref()
                .and_then(|c| c.proprietary.as_deref())
            {
                None => issues.push(ValidationIssue::new(
                    "fednow.pmttpinf.ctgypurp",
                    format!("{base}/PmtTpInf/CtgyPurp"),
                    "the FedNow profile requires CtgyPurp/Prtry".to_string(),
                    RuleSource::FedNowProfile,
                )),
                Some(v) if !FEDNOW_CATEGORY_PURPOSES.contains(&v) => {
                    issues.push(ValidationIssue::new(
                        "fednow.ctgypurp.known",
                        format!("{base}/PmtTpInf/CtgyPurp/Prtry"),
                        format!(
                            "'{v}' is not a known FedNow category purpose ({})",
                            FEDNOW_CATEGORY_PURPOSES.join(", ")
                        ),
                        RuleSource::FedNowProfile,
                    ))
                }
                Some(_) => {}
            }
        }
    }

    validate_amount(
        issues,
        &format!("{base}/IntrBkSttlmAmt"),
        &tx.interbank_settlement_amount,
    );

    if tx.interbank_settlement_date.is_none() {
        issues.push(ValidationIssue::new(
            "fednow.intrbksttlmdt.required",
            format!("{base}/IntrBkSttlmDt"),
            "the FedNow profile requires IntrBkSttlmDt".to_string(),
            RuleSource::FedNowProfile,
        ));
    }

    for (agent, code, tag) in [
        (
            &tx.instructing_agent,
            "fednow.instgagt.required",
            "InstgAgt",
        ),
        (&tx.instructed_agent, "fednow.instdagt.required", "InstdAgt"),
    ] {
        match agent {
            None => issues.push(ValidationIssue::new(
                code,
                format!("{base}/{tag}"),
                format!("the FedNow profile requires {tag}"),
                RuleSource::FedNowProfile,
            )),
            Some(a) => validate_frs_agent(issues, &format!("{base}/{tag}"), a),
        }
    }

    for (account, code, tag) in [
        (&tx.debtor_account, "fednow.dbtracct.required", "DbtrAcct"),
        (&tx.creditor_account, "fednow.cdtracct.required", "CdtrAcct"),
    ] {
        if account.is_none() {
            issues.push(ValidationIssue::new(
                code,
                format!("{base}/{tag}"),
                format!("the FedNow profile requires {tag}"),
                RuleSource::FedNowProfile,
            ));
        }
    }

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
        validate_frs_agent(issues, &format!("{base}/{tag}"), agent);
    }
}

/// Agents in FedNow payment messages carry `ClrSysMmbId` with `ClrSysId/Cd`
/// fixed at `USABA` and a 9-digit routing number (`RoutingNumber_FRS`).
fn validate_frs_agent(
    issues: &mut Vec<ValidationIssue>,
    path: &str,
    agent: &crate::pacs008::BranchAndFinancialInstitutionIdentification,
) {
    match &agent
        .financial_institution_identification
        .clearing_system_member_identification
    {
        None => issues.push(ValidationIssue::new(
            "fednow.agent.clrsysmmbid",
            format!("{path}/FinInstnId/ClrSysMmbId"),
            "the FedNow profile identifies agents via ClrSysMmbId".to_string(),
            RuleSource::FedNowProfile,
        )),
        Some(member) => {
            let scheme = member
                .clearing_system_identification
                .as_ref()
                .and_then(|c| c.code.as_deref());
            if scheme != Some("USABA") {
                issues.push(ValidationIssue::new(
                    "fednow.agent.usaba",
                    format!("{path}/FinInstnId/ClrSysMmbId/ClrSysId/Cd"),
                    format!(
                        "the FedNow profile requires ClrSysId/Cd 'USABA', found {}",
                        scheme.unwrap_or("(none)")
                    ),
                    RuleSource::FedNowProfile,
                ));
            }
            validate_routing_number(
                issues,
                format!("{path}/FinInstnId/ClrSysMmbId/MmbId"),
                &member.member_identification,
            );
        }
    }
}

/// FedNow message id: CCYYMMDD + 9-char connection party id + 1..18-char
/// sender reference (18..35 alphanumerics total, first 8 numeric).
fn is_fednow_message_id(s: &str) -> bool {
    (18..=35).contains(&s.len())
        && s.bytes().take(8).all(|b| b.is_ascii_digit())
        && s.len() >= 8
        && s.bytes().skip(8).all(|b| b.is_ascii_alphanumeric())
}

fn validate_amount(
    issues: &mut Vec<ValidationIssue>,
    path: &str,
    amount: &ActiveCurrencyAndAmount,
) {
    let path = path.to_string();

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
                    path.clone(),
                    format!("USD amounts carry at most 2 fraction digits, found {}", dec.fraction_digits),
                    RuleSource::FedNowProfile,
                ));
            }
            if dec.total_digits > 14 {
                issues.push(ValidationIssue::new(
                    "fednow.amount.digits",
                    path,
                    format!("FedNow amounts carry at most 14 total digits, found {}", dec.total_digits),
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
    sum.is_multiple_of(10)
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
    total_digits: usize,
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
        total_digits: int_part.len() + frac_part.len(),
    })
}
