# Fuzzing

[`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz) targets for the ISO 20022
parsers and validators in [`core/`](../core).

## What is being tested, and why these functions

`fednow_core::pacs008::parse` and `fednow_core::pacs002::parse` are the two
functions a real deployment points at bytes that arrived over a network. Every
other entry point in this workspace is reached from a message that one of them
already accepted.

The property under test is not "does it parse correctly" — the fixture tests in
[`core/tests/`](../core/tests) cover that, against the official XSDs. It is the
weaker and more important one: **neither parsing nor validating ever panics**,
whatever the input. A returned `Err` or a `Vec<ValidationIssue>` is a correct
outcome; an index out of bounds, an unwrap on `None`, an arithmetic overflow or
an unbounded allocation is not. `fednow-core` is a library, so a panic in it is
an outage in whatever embeds it.

Each target parses and then validates, because the two fail in different places.
`parse` rejects XML the typed model cannot represent. The validators walk a
*successfully parsed* document applying XSD facets, ISO 20022 cross-field rules
and the FedNow profile — code that slices strings and compares lengths on the
assumption that the parser already bounded them. That assumption is exactly what
a fuzzer is good at breaking.

| Target | Covers |
|---|---|
| `pacs008_parse_validate` | `pacs008::parse` → `validate_pacs008` |
| `pacs002_parse_validate` | `pacs002::parse` → `validate_pacs002`, then `validate_pacs002_direction` in both FedNow directions |

## Running it

Nightly Rust only: `cargo fuzz` reaches libFuzzer through `-Zsanitizer`, which
stable does not have. This directory is the only part of the repository that is
not built with the stable toolchain, which is why it is excluded from the
workspace in the root `Cargo.toml` — `cargo build`, `cargo clippy` and
`cargo test` never come near it.

```console
$ rustup toolchain install nightly
$ cargo +nightly install cargo-fuzz --locked
$ ./fuzz/seed-corpus.sh
$ cargo +nightly fuzz run pacs008_parse_validate -- -max_total_time=120
```

## The corpus is generated, not committed

`seed-corpus.sh` copies `core/tests/fixtures/*.xml` and
`conformance/vectors/*/*.xml` into `corpus/<target>/`, which is gitignored.

Seeds matter more than budget here. Starting from an empty corpus, libFuzzer
spends its run rediscovering that an ISO 20022 message opens with `<Document`,
and every input dies in the parser without a validator ever executing. Starting
from a real message, the first mutation is already inside the fields the rules
check.

Those real messages are the fixtures — and they are the project's source of
truth for what a well-formed message looks like. A committed corpus would be a
second copy of them, one that stops matching the first the day somebody edits a
fixture and not its duplicate. Generating it keeps one copy. (Same reasoning as
`site/scripts/sync-docs.mjs`, which generates the website's pages from the
Markdown rather than duplicating it.)

The script also refuses to copy any file containing a routing number that does
not start with `99`. The `99` block is assigned to no institution; a corpus is a
pile of payment messages, and a real bank's routing number inside one would end
up in CI artifacts and crash reports. See the table in
[`docs/handbook/README.md`](../docs/handbook/README.md).

## When a target crashes

`cargo fuzz` writes the input to `artifacts/<target>/` and exits non-zero. That
file is evidence, not source, and does not belong in this directory as a commit:

1. Reproduce it: `cargo +nightly fuzz run <target> artifacts/<target>/<file>`.
2. Open an issue with the input and the backtrace.
3. Fix the parser or validator, and add the input to `core/tests/` as an
   ordinary regression test — where it runs on stable, on every pull request,
   forever.

The nightly [`fuzz.yml`](../.github/workflows/fuzz.yml) workflow uploads the
crashing input as a run artifact for exactly this.
