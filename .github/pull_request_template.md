## What changes

<!-- one line per crate/area touched: core / simulator / gateway / conformance / sdk / docs / ci -->

## Pipeline (esteira)

- [ ] `/esteira:review` — verdict: <!-- APROVADO / APROVADO COM RESSALVAS -->
- [ ] `/esteira:qa` — TUDO OK (`cargo fmt --all --check` · `cargo clippy --workspace --all-targets -- -D warnings` · `cargo test --workspace`)
- [ ] `/esteira:security` — SEM ACHADOS / NÃO BLOQUEANTES (no credentials, certificates, real routing numbers or institution names)
- [ ] New or changed XML fixtures validate against `core/schemas/*.xsd` and have a vector in `conformance/vectors`
- [ ] Public API of `fednow-core` changed? Entry under `## [Unreleased]` in `CHANGELOG.md` (pre-1.0: breaking = minor bump)
- [ ] Behaviour visible to users changed? README / QUICKSTART / handbook updated (`/esteira:docs`)

## How to test

<!-- commands, or the QUICKSTART step that exercises the change -->
