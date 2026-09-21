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

`ci.yml` also has two gated jobs that run only when a diff reaches them: **`site`**
(build + trademark/privacy checks) and **`docker`**, which runs the quickstart —
`docker compose build`, `up -d`, then both services must answer `/healthz` on
their published ports — whenever `gateway/**`, `simulator/**`, `core/**`,
`Cargo.toml`/`Cargo.lock`, `docker-compose.yml` or `ci.yml` changes. That job is
the only thing that proves a binary built in the Dockerfile's build stage can
actually start in its runtime stage; `cargo test` runs against the runner's
libraries, not the image's. It is why **both Dockerfile stages must stay on the
same Debian release** — a glibc skew builds cleanly and dies at startup.

`audit.yml` runs `cargo audit` on every lockfile change and daily. `codeql.yml`
runs CodeQL on every PR and push to `main` over Rust, Python, Java and the
workflow files. `fuzz.yml` runs the `cargo-fuzz` targets nightly; to run one
locally (nightly toolchain, see [`fuzz/README.md`](fuzz/README.md)):

```sh
./fuzz/seed-corpus.sh
cargo +nightly fuzz run pacs008_parse_validate -- -max_total_time=120
```

Dependabot opens weekly PRs (Mondays) for cargo, GitHub Actions, maven
(`sdk/java`), npm (`site`), docker (both Dockerfiles) and pip
(`.github/requirements`): cargo minor/patch are grouped, majors come alone;
triage with `/esteira:deps`.

Pinned by hash and therefore only updated by something that updates them: the
Dockerfile base images (by digest), `.github/requirements/pytest.txt` (by
artifact hash) and every action `uses:` (by commit SHA). Both Dockerfile stages
must stay on the same Debian release.

## Rules

- **Language**: code, comments, commits, docs and PRs in English. Discussion
  with the maintainer may be in pt-BR.
- **No real data, ever**: no credentials, certificates, real routing numbers,
  institution names or Fed endpoints. Nothing copied from access-restricted
  specifications (see `core/schemas/README.md` for how XSDs are handled).
  **Routing numbers must start with `99`** — a block the ABA assigns to nobody
  (real ones live in `01`–`12`, `21`–`32`, `61`–`72`, `80`), so an example can
  never name a real institution. Use `991000009` (sender), `992000008`
  (creditor agent) or `993000007` (service application); all three pass the
  check digit because `fednow.aba.checksum` enforces it. For a negative test,
  alter the last digit (`991000008`, `992000007`). The table in
  `docs/handbook/README.md` is the reference, and `site/scripts/check-build.sh`
  fails the site build if a known-real number reappears.
- **Maintainer's personal data**: never write a personal detail about the
  maintainer — name, biography, employer, location, dates, job title, past
  roles — that the maintainer has not provided **verbatim**. Do not infer one
  from a GitHub handle, a commit email, a package namespace or anything else,
  and do not embellish a detail that was provided. If a sentence needs a fact
  that is missing, leave `TODO(maintainer): <what is needed>` for them to fill
  in; shipping a plausible guess is worse than shipping a gap. This is not
  hypothetical: a surname was invented in `site/src/content/docs/about.md` and
  published. The facts currently cleared for publication are exactly:

  | Fact | Value |
  |---|---|
  | Name | João Bueno |
  | GitHub | `@joaoabuenosi` |
  | Contact | `joaobuenosi@gmail.com` (already published in `SECURITY.md`) |
  | Background | payments engineer who ran Pix point-of-sale middleware in production |

  Anything beyond that table needs the maintainer's own words first. The same
  rule governs the site's frontmatter note in `about.md`; keep the two in step.
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
- **The site derives, never duplicates**: a fact the repository already states
  (version, release status, SBOM formats, release asset names, workflow
  cadences) is parsed at build time by `site/scripts/project-facts.mjs`, not
  typed into a page; the two verification commands come out of `SECURITY.md` the
  same way, via `sync-docs.mjs`. Before adding a fact to a hand-authored page,
  check whether a repository file owns it; if it does, derive it. **No page
  names a release asset itself** — signature bundles were renamed from
  `.cosign.bundle` to `.sigstore.json` without touching a page, and that is the
  property to preserve. The `site` job in `ci.yml` builds and checks the site on
  any PR that touches a file the site reads, so a change that would falsify it
  fails in review rather than after deploy. Rewording a fact in `SECURITY.md`
  is *meant* to break the build: `npm run check` runs
  `scripts/selftest-project-facts.sh`, which rewords each fact in a throwaway
  copy and requires the extractor to refuse it with that fact's own message. A
  new derived fact needs a case there; `node scripts/derive-facts.mjs` prints
  what a checkout currently derives.

## Release

Version in `Cargo.toml` (`[workspace.package] version`). Sequence: PR with
green CI → merge into `main` → tag `vX.Y.Z` → `release.yml` builds binaries,
SBOM and checksums. `cargo publish` only with explicit maintainer confirmation
(irreversible for that version).

## Git

`main` is protected: pull request + green `fmt + clippy + test` required.
Short imperative subject (≤ 72 chars), body explaining *why*; reference issues
with `#N`. Never force-push `main`.
