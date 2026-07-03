//! fednow-conformance — prove an implementation speaks FedNow Release 1.
//!
//! Three capabilities, also exposed by the `fednow-conformance` CLI:
//!
//! - **Vectors** ([`vectors`]): a language-agnostic corpus of messages with
//!   expected verdicts. Any implementation can consume the XML + `valid`
//!   fields; implementations adopting fednow-core's rule taxonomy can also
//!   assert the exact `codes`.
//! - **Validate** ([`check`]): judge any message file against the FedNow
//!   Release 1 profiles (message type detected from the namespace).
//! - **Scenarios** ([`scenarios`]): drive a live FedNow-Service-side endpoint
//!   (e.g. `fednow-sim`) through the official CTP credit-transfer scenarios
//!   and report pass/fail.

pub mod check;
pub mod scenarios;
pub mod vectors;
