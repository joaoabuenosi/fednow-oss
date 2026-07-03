//! The southbound port: how the gateway reaches the FedNow Service.
//!
//! In production this is IBM MQ + mTLS + signed messages; in development it is
//! HTTP against `fednow-sim`. The trait is the seam: the service layer never
//! knows which one it is talking to.

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
