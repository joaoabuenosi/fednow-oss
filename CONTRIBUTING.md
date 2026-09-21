# Contributing

Thanks for your interest! The project is in early development — the fastest way to help
is to pick an open issue or open a discussion before writing code.

## Ground rules

- **Language:** code, comments, commits and docs are in English.
- **Branches:** `main` is protected; all changes land via pull request with green CI.
- **Pipeline:** the repo ships a Claude Code plugin setup (`.claude/settings.json`, plugin `esteira`) and `AGENTS.md` with the rules agents follow; the PR template lists the gates.
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

## Rehearsing a release

A tag push is irreversible: by the time a release step fails, the tag exists and a
half-finished release is already public. That is exactly how v0.3.0 shipped with zero
assets. So rehearse first.

From the **Actions** tab, pick the **Release** workflow, click **Run workflow**, leave
the ref on `main` (or the release-prep branch) and leave **`dry_run` checked** — it is
checked by default. The run executes the same steps a tag push would: the tag/version
guard (against a synthetic `v<workspace version>` tag built from `Cargo.toml`), the test
suite, the release build, both SBOMs, the fail-fast checks on each SBOM, and the
checksums. It then uploads the whole asset set as the `release-assets` artifact and
prints the checksums to the run summary.

It does **not** sign and does **not** create a release. Signing and publishing live in a
separate `publish` job that a dry run skips, so the dry-run path is never granted
`id-token: write` or `contents: write`.

Only once that run is green should the tag be pushed:

```sh
git tag -a vX.Y.Z <merge-sha> -m "vX.Y.Z" && git push origin vX.Y.Z
```

Unchecking `dry_run` is the recovery path, not the normal one: it publishes a tag that
has *already* been pushed, and the run must be started from that tag's ref (the guard
refuses a branch, because cosign binds the signing certificate to the ref).

## Getting started

```sh
cargo test --workspace
```

The current milestone and the roadmap live in the README. Design context for each
component is documented in `docs/`.
