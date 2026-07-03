# fednow-conformance

Prove an implementation speaks FedNow Service Release 1 — in either direction.

## Three tools

**1. The vector corpus** (`vectors/`) — language-agnostic test vectors: real
ISO 20022 messages (valid and deliberately broken) with expected verdicts in
[`vectors/manifest.toml`](vectors/manifest.toml). Any implementation, in any
language, can consume the XML and assert the `valid` field; implementations
adopting fednow-core's rule taxonomy also assert the exact `codes`
(e.g. `fednow.aba.checksum`). Run it against fednow-core itself:

```sh
cargo run -p fednow-conformance -- vectors
```

**2. The validator** — judge any message file or directory against the FedNow
Release 1 profiles (message type detected from the namespace):

```sh
cargo run -p fednow-conformance -- validate path/to/messages/
```

**3. The scenario runner** — drive a live FedNow-Service-side endpoint through
the official Customer Testing Program credit-transfer scenarios (Readiness
Portal Guide, chapter 3) and report pass/fail:

```sh
cargo run -p fednow-sim &          # or any implementation of the same contract
cargo run -p fednow-conformance -- scenarios http://localhost:8080
# PASS cct-1-happy-path
# PASS cct-2-service-reject
# PASS cct-3-participant-reject
# PASS cct-4-accept-without-posting
# PASS cct-5-timeout-then-query
# PASS cct-6-query-unknown-is-not-found
```

`fednow-sim` passes all scenarios in CI — the suite keeps the simulator honest
and gives anyone building against it a certification target before the real
Customer Testing Program.

## Claiming conformance

An implementation may claim *"message-conformant with fednow-core vX"* when it
agrees with every vector's `valid` verdict, and *"scenario-conformant"* when
the scenario runner passes against it. Both checks are deterministic and run
in one command.
