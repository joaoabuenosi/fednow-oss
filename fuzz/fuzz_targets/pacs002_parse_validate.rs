//! pacs.002.001.10 — parse, then validate in both FedNow directions.
//!
//! Same contract as the pacs.008 target: parse and validate must return errors,
//! never panic, on any input at all.
//!
//! Both directions are validated because they are different code paths over the
//! same document. `validate_pacs002` checks the facets and rules common to
//! both; `validate_pacs002_direction` layers the participant-to-service profile
//! (FedNow message-id format, reason-code shape) or the service-to-participant
//! one (proprietary reasons, acceptance and settlement timestamps) on top. A
//! fuzzer that only called the common half would never reach either.

#![no_main]

use fednow_core::Pacs002Direction;
use libfuzzer_sys::fuzz_target;
use std::hint::black_box;

fuzz_target!(|data: &[u8]| {
    let Ok(xml) = std::str::from_utf8(data) else {
        return;
    };

    if let Ok(doc) = fednow_core::pacs002::parse(xml) {
        // `black_box` rather than `let _ =`: the issue lists are genuinely
        // unused, and without it the optimiser may delete the calls that
        // produced them.
        black_box(fednow_core::validate_pacs002(&doc));
        for direction in [
            Pacs002Direction::ParticipantToService,
            Pacs002Direction::ServiceToParticipant,
        ] {
            black_box(fednow_core::validate_pacs002_direction(&doc, direction));
        }
    }
});
