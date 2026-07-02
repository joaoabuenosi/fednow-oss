//! Build a FedNow-profile pacs.008 and print it to stdout.
//!
//! ```sh
//! cargo run -p fednow-core --example build_pacs008
//! ```

use fednow_core::builder::Pacs008Builder;
use fednow_core::pacs008;
use fednow_core::validate::validate_pacs008;

fn main() {
    let xml = Pacs008Builder::new(
        "M20260702EXAMPLE00000000000001",
        "2026-07-02T15:30:00Z",
        "E2E-20260702-EXAMPLE-0001",
        125_000, // $1,250.00 in cents
        "021040078",
        "091000019",
    )
    .uetr("8a562c67-ca16-48ba-b074-65581be6f001")
    .interbank_settlement_date("2026-07-02")
    .debtor_name("Jane Example Debtor")
    .debtor_account("123456789012")
    .creditor_name("John Example Creditor")
    .creditor_account("987654321000")
    .to_xml()
    .expect("serialization");

    // Sanity: what we emit must round-trip clean through our own rules.
    let issues = validate_pacs008(&pacs008::parse(&xml).expect("parse"));
    assert!(issues.is_empty(), "{issues:#?}");

    println!("{xml}");
}
