# Contributing

Thanks for your interest! The project is in early development — the fastest way to help
is to pick an open issue or open a discussion before writing code.

## Ground rules

- **Language:** code, comments, commits and docs are in English.
- **Branches:** `main` is protected; all changes land via pull request with green CI.
- **CI must pass:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo test --workspace` run on every push and PR.
- **No credentials, certificates or institution-specific data** in the repo — ever.
  This includes test fixtures: use fake (but checksum-valid) routing numbers and
  fictitious names.
- **Licensing:** contributions are accepted under Apache-2.0. Do not paste content from
  specifications or portals that restrict redistribution (see `core/schemas/README.md`
  for how we handle XSDs).

## Commit style

Short imperative subject line (max ~72 chars), body explaining *why* when it isn't
obvious. Reference issues with `#N`.

## Getting started

```sh
cargo test --workspace
```

The current milestone and the roadmap live in the README. Design context for each
component is documented in `docs/`.
