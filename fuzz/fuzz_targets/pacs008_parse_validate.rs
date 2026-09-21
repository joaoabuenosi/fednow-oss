//! pacs.008.001.08 — parse, then validate.
//!
//! `fednow_core::pacs008::parse` is the widest untrusted-input surface in this
//! project: on a real deployment its argument is an ISO 20022 message that
//! arrived over the wire. `validate_pacs008` is run on the result because the
//! two fail differently — `parse` rejects XML the typed model cannot represent,
//! `validate_pacs008` walks a *successfully parsed* document applying XSD
//! facets and FedNow profile rules, and a validator is exactly the kind of code
//! that indexes into a string it assumes `parse` already bounded.
//!
//! The contract under test is that neither ever panics, however malformed,
//! truncated, deeply nested or adversarially encoded the input is: the only
//! acceptable failure is a returned `Err`/`Vec<ValidationIssue>`. A panic in a
//! library is a denial of service in the gateway that embeds it.

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::hint::black_box;

fuzz_target!(|data: &[u8]| {
    // The public API takes `&str`. Non-UTF-8 is rejected by the caller's
    // decoder, not by this parser, so feeding it here would only exercise
    // `from_utf8`. libFuzzer's mutations stay mostly inside the seeded XML.
    let Ok(xml) = std::str::from_utf8(data) else {
        return;
    };

    // A parse error is a correct outcome, not a finding: most mutations produce
    // XML the typed model cannot represent. What matters is reaching the
    // validator with the ones that do parse.
    if let Ok(doc) = fednow_core::pacs008::parse(xml) {
        // `black_box` rather than `let _ =`: the issue list is genuinely unused,
        // and without it the optimiser is free to delete the call that produced
        // it — which would leave the target fuzzing nothing.
        black_box(fednow_core::validate_pacs008(&doc));
    }
});
