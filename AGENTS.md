# AGENTS.md — permanent context for AI agents

Product and repository context for coding agents working on FedNow OSS. The
development pipeline is operated by the **esteira** plugin (marketplace
`joca-plugins`, enabled in `.claude/settings.json`); this file holds only what
is specific to this repository. Session notes never go here (`CLAUDE.md` is
gitignored on purpose — see PR #55).

## What this is

Open-source (Apache-2.0) tooling that lowers the cost of building **send**
capability on the FedNow® Service, for community banks, credit unions and
service providers in the US. Cargo workspace (`core/`, `simulator/`,
`gateway/`, `conformance/`) plus SDKs in `sdk/python` and `sdk/java`. Current
milestone and roadmap live in `README.md`; requirements in
`docs/requisitos.md`; design docs in `docs/design/`; the FedNow Integration
Handbook in `docs/handbook/`.

| Crate | Directory | Role |
|---|---|---|
| `fednow-core` | `core/` | ISO 20022 library: parsing, validation (XSD facets + FedNow profile rules), builders, MQ technical envelope |
| `fednow-sim` | `simulator/` | Local FedNow simulator (HTTP + MQ modes) for CTP preparation |
| `fednow-gateway` | `gateway/` | Send middleware: event-sourced state machine on SQLite, idempotency-keyed REST API, outbox, pacs.028 reconciler |
| `fednow-conformance` | `conformance/` | Language-agnostic vector corpus + validator CLI + live scenario runner |

## Pipeline

```
/esteira:plan → implement → /esteira:review → /esteira:qa → /esteira:security → /esteira:release → /esteira:docs
                                 gate            gate            gate               gate
```

## Commands (what CI runs — `.github/workflows/ci.yml`)

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
xmllint --noout --schema core/schemas/<msg>.xsd core/tests/fixtures/<msg>_valid*.xml   # when fixtures or schemas change
FEDNOW_GW_URL=http://localhost:8090 python -m pytest sdk/python -q                     # needs sim + gateway running (see ci.yml)
FEDNOW_GW_URL=http://localhost:8090 mvn -q -B -f sdk/java/pom.xml test
```

`audit.yml` runs `cargo audit` on every lockfile change and daily. Dependabot
opens weekly PRs (Mondays): cargo minor/patch are grouped, majors come alone;
triage with `/esteira:deps`.

## Rules

- **Language**: code, comments, commits, docs and PRs in English. Discussion
  with the maintainer may be in pt-BR.
- **No real data, ever**: no credentials, certificates, real routing numbers,
  institution names or Fed endpoints. Fixtures use fictitious identifiers with
  valid checksums. Nothing copied from access-restricted specifications
  (see `core/schemas/README.md` for how XSDs are handled).
- **ISO 20022**: required fields per FedNow profile, correct namespaces; every
  new fixture validates against the vendored XSD and gets a vector in
  `conformance/vectors`.
- **Rust**: `unsafe` only with a `// SAFETY:` comment; no swallowed errors
  (`let _ =`, bare `unwrap()`/`expect()` outside tests); no `println!` in
  library code; `clippy -D warnings` must pass.
- **Timeout reconciliation invariants** (`docs/handbook/02-timeout-reconciliation.md`):
  `ACK_PENDING` past the timeout is resolved via pacs.028 — never a blind resend.
- **Public API of `fednow-core`**: a breaking change needs a `CHANGELOG.md`
  entry and a version bump coherent with SemVer (pre-1.0: minor).
- **Docs follow behaviour**: visible change → README / QUICKSTART / handbook /
  CHANGELOG in the same PR.

## Release

Version in `Cargo.toml` (`[workspace.package] version`). Sequence: PR with
green CI → merge into `main` → tag `vX.Y.Z` → `release.yml` builds binaries,
SBOM and checksums. `cargo publish` only with explicit maintainer confirmation
(irreversible for that version).

## Git

`main` is protected: pull request + green `fmt + clippy + test` required.
Short imperative subject (≤ 72 chars), body explaining *why*; reference issues
with `#N`. Never force-push `main`.
