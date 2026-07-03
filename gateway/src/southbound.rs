//! The southbound port: how the gateway reaches the FedNow Service.
//!
//! In production this is IBM MQ + mTLS + signed messages; in development it is
//! `fednow-sim` — either its synchronous HTTP dev mode ([`HttpSimPort`]) or
//! its MQ mode ([`MqSimPort`]), which has the real connection's semantics:
//! fire-and-forget sends of `FedNowIncoming` envelopes, advices polled off a
//! receive queue as `FedNowOutgoing` envelopes. The trait is the seam: the
//! service layer never knows which one it is talking to.

use fednow_core::builder::Head001Builder;
use fednow_core::envelope::{self, Direction};
use fednow_core::{pacs008, pacs028};
use thiserror::Error;

/// What came back from the far side.
#[derive(Debug)]
pub enum SubmitOutcome {
    /// A pacs.002 advice arrived synchronously.
    Advice(String),
    /// The message was accepted but no advice arrived (yet) — the timeout
    /// clock is running.
    Accepted,
}

#[derive(Debug, Error)]
pub enum PortError {
    /// The far side rejected the request at the transport level.
    #[error("transport rejected the message ({status}): {body}")]
    Rejected { status: u16, body: String },
    /// The transport failed — the message may or may not have arrived.
    /// This ambiguity is exactly what the reconciler resolves.
    #[error("transport failure: {0}")]
    Transport(String),
}

/// The FedNow-facing port.
pub trait FedNowPort {
    /// Send a pacs.008.
    fn submit(&self, pacs008_xml: &str) -> Result<SubmitOutcome, PortError>;
    /// Send a pacs.028 payment status request.
    fn query(&self, pacs028_xml: &str) -> Result<SubmitOutcome, PortError>;
    /// Poll for the next asynchronously delivered advice (a `FedNowOutgoing`
    /// envelope). Request/response transports have none — the default says so.
    fn poll_advice(&self) -> Result<Option<String>, PortError> {
        Ok(None)
    }
}

/// HTTP adapter for `fednow-sim` (development mode).
pub struct HttpSimPort {
    base_url: String,
}

impl HttpSimPort {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    fn post(&self, xml: &str) -> Result<SubmitOutcome, PortError> {
        let url = format!("{}/fednow/messages", self.base_url);
        match ureq::post(&url)
            .set("content-type", "application/xml")
            .send_string(xml)
        {
            Ok(resp) => {
                let status = resp.status();
                let body = resp
                    .into_string()
                    .map_err(|e| PortError::Transport(e.to_string()))?;
                if status == 202 {
                    Ok(SubmitOutcome::Accepted)
                } else {
                    Ok(SubmitOutcome::Advice(body))
                }
            }
            Err(ureq::Error::Status(status, resp)) => Err(PortError::Rejected {
                status,
                body: resp.into_string().unwrap_or_default(),
            }),
            Err(e) => Err(PortError::Transport(e.to_string())),
        }
    }
}

impl FedNowPort for HttpSimPort {
    fn submit(&self, pacs008_xml: &str) -> Result<SubmitOutcome, PortError> {
        self.post(pacs008_xml)
    }

    fn query(&self, pacs028_xml: &str) -> Result<SubmitOutcome, PortError> {
        self.post(pacs028_xml)
    }
}

/// MQ-mode adapter for `fednow-sim`: the production connection's semantics
/// over HTTP. Sends wrap the business message in a `FedNowIncoming` envelope
/// (BAH included) and are fire-and-forget; advices arrive on the participant's
/// receive queue and are drained with [`FedNowPort::poll_advice`].
///
/// The real FedNow adapter (IBM MQ + mTLS + signed envelopes) will replace the
/// HTTP calls here; everything above this seam already lives with the
/// asynchrony.
pub struct MqSimPort {
    base_url: String,
    participant_routing_number: String,
}

/// The FedNow Service application identifier (BAH `To` of every send).
const FEDNOW_SERVICE_RTN: &str = "021150706";

impl MqSimPort {
    pub fn new(base_url: impl Into<String>, participant_routing_number: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            participant_routing_number: participant_routing_number.into(),
        }
    }

    fn send(&self, envelope_xml: &str) -> Result<SubmitOutcome, PortError> {
        let url = format!(
            "{}/mq/participants/{}/send",
            self.base_url, self.participant_routing_number
        );
        match ureq::post(&url)
            .set("content-type", "application/xml")
            .send_string(envelope_xml)
        {
            // MQ semantics: a successful PUT never carries an advice.
            Ok(_) => Ok(SubmitOutcome::Accepted),
            Err(ureq::Error::Status(status, resp)) => Err(PortError::Rejected {
                status,
                body: resp.into_string().unwrap_or_default(),
            }),
            Err(e) => Err(PortError::Transport(e.to_string())),
        }
    }

    /// Wrap a business document in a `FedNowIncoming` envelope. The BAH
    /// reuses the document's own identifiers — the adapter owns no clock.
    fn wrap(
        &self,
        wrapper: &str,
        message_definition_identifier: &str,
        business_message_identifier: &str,
        creation_date_time: &str,
        document_xml: &str,
    ) -> Result<String, PortError> {
        let bah = Head001Builder::new(
            self.participant_routing_number.clone(),
            FEDNOW_SERVICE_RTN,
            business_message_identifier,
            message_definition_identifier,
            creation_date_time,
        )
        .to_xml()
        .map_err(|e| PortError::Transport(format!("BAH construction failed: {e}")))?;
        Ok(envelope::build(
            Direction::Incoming,
            wrapper,
            &bah,
            strip_xml_declaration(document_xml),
            None,
        ))
    }
}

impl FedNowPort for MqSimPort {
    fn submit(&self, pacs008_xml: &str) -> Result<SubmitOutcome, PortError> {
        let doc = pacs008::parse(pacs008_xml)
            .map_err(|e| PortError::Transport(format!("not a pacs.008: {e}")))?;
        let hdr = &doc.fi_to_fi_customer_credit_transfer.group_header;
        let envelope_xml = self.wrap(
            "FedNowCustomerCreditTransfer",
            "pacs.008.001.08",
            &hdr.message_identification,
            &hdr.creation_date_time,
            pacs008_xml,
        )?;
        self.send(&envelope_xml)
    }

    fn query(&self, pacs028_xml: &str) -> Result<SubmitOutcome, PortError> {
        let doc = pacs028::parse(pacs028_xml)
            .map_err(|e| PortError::Transport(format!("not a pacs.028: {e}")))?;
        let hdr = &doc.fi_to_fi_payment_status_request.group_header;
        let envelope_xml = self.wrap(
            "FedNowPaymentStatusRequest",
            "pacs.028.001.03",
            &hdr.message_identification,
            &hdr.creation_date_time,
            pacs028_xml,
        )?;
        self.send(&envelope_xml)
    }

    fn poll_advice(&self) -> Result<Option<String>, PortError> {
        let url = format!(
            "{}/mq/participants/{}/receive",
            self.base_url, self.participant_routing_number
        );
        match ureq::get(&url).call() {
            Ok(resp) if resp.status() == 204 => Ok(None),
            Ok(resp) => resp
                .into_string()
                .map(Some)
                .map_err(|e| PortError::Transport(e.to_string())),
            Err(ureq::Error::Status(status, resp)) => Err(PortError::Rejected {
                status,
                body: resp.into_string().unwrap_or_default(),
            }),
            Err(e) => Err(PortError::Transport(e.to_string())),
        }
    }
}

/// Either development adapter, selected at runtime (`FEDNOW_GW_SOUTHBOUND`).
pub enum AnyPort {
    Http(HttpSimPort),
    Mq(MqSimPort),
}

impl FedNowPort for AnyPort {
    fn submit(&self, pacs008_xml: &str) -> Result<SubmitOutcome, PortError> {
        match self {
            AnyPort::Http(p) => p.submit(pacs008_xml),
            AnyPort::Mq(p) => p.submit(pacs008_xml),
        }
    }

    fn query(&self, pacs028_xml: &str) -> Result<SubmitOutcome, PortError> {
        match self {
            AnyPort::Http(p) => p.query(pacs028_xml),
            AnyPort::Mq(p) => p.query(pacs028_xml),
        }
    }

    fn poll_advice(&self) -> Result<Option<String>, PortError> {
        match self {
            AnyPort::Http(p) => p.poll_advice(),
            AnyPort::Mq(p) => p.poll_advice(),
        }
    }
}

/// Drop a leading `<?xml …?>` declaration: envelope children are embedded in
/// a larger document and must not carry one.
fn strip_xml_declaration(xml: &str) -> &str {
    let trimmed = xml.trim_start();
    match trimmed.strip_prefix("<?xml") {
        Some(rest) => rest
            .split_once("?>")
            .map(|(_, tail)| tail.trim_start())
            .unwrap_or(trimmed),
        None => trimmed,
    }
}
