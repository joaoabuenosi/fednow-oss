# Design: the real IBM MQ transport (gateway southbound)

*Status: design accepted, implementation not started · July 2026 ·
sources: FedNow Service Technical Overview and Planning Guide
(NONCONFIDENTIAL//EXTERNAL), Operating Procedures — public facts only.*

The gateway already speaks the *semantics* of the production connection
(fire-and-forget sends, advices pumped off a receive queue — `MqSimPort`).
This document plans the last mile: replacing HTTP-to-the-sim with a real
IBM MQ client connection, without moving anything above the `FedNowPort`
seam.

## Confirmed public facts about the production connection

From the Technical Overview and Planning Guide ("MQ Connectivity and
Queues", "Designing Client Applications", "Security and Controls"):

1. Participants connect with an **embedded IBM MQ client library** to a
   **cluster of FedNow-hosted queue managers**, over FedLine Direct or
   FedLine Advantage. The service appears as a **single connection target**;
   on component failure the connection drops and the client must
   **immediately reconnect** (surviving components route the new
   connection).
2. **Per-participant dedicated queues**, directional: **OUT queues**
   ("to FedNow", naming controlled by the service) and **IN queues**
   ("from FedNow", naming customizable by the participant).
3. **mTLS with FRS-issued client certificates**; the service additionally
   validates that entities referenced in a message are entitled to the
   queue it arrived on, and validates the message signature.
4. Client design expectations, verbatim intent:
   - long-running consumers with **persistent (pooled) connections**;
   - **multiple concurrent connections** for throughput, all monitored and
     reestablished by the participant;
   - **decouple draining from processing**: pull messages quickly off the
     FedNow queues into local storage/streaming, do business processing
     from there.
5. Correlation: an admi.002 message reject references the rejected message
   by the **MQ correlation id in the technical message header**
   (Implementation Guide) — transport-level identity matters and must be
   persisted.

Item 4's third bullet is exactly the architecture the gateway already has:
`poll_advice` drains, the event store absorbs, the state machine processes.
The design was chosen to make this adapter a thin shim.

## Decision: MQI via the IBM redistributable client, behind a feature flag

There is no production-grade pure-Rust MQI implementation. The realistic
options:

| Option | Verdict |
|---|---|
| FFI to the **IBM MQ redistributable client** (`libmqm`) | **Chosen.** The official client is freely downloadable and redistributable per IBM's terms; MQI is the API the Fed's docs assume. |
| IBM MQ via JMS (Java sidecar) | Rejected: a second runtime and process to operate 24x7; belongs in the future Java SDK instead. |
| AMQP/MQTT bridges | Rejected: not what FedNow exposes. |
| Pure-Rust MQI reimplementation | Rejected: enormous, unauditable risk for a payments path. |

Consequences:

- New adapter `MqPort` in the gateway, implementing `FedNowPort`
  (`submit`, `query`, `poll_advice`), compiled only with the cargo feature
  **`ibm-mq`** so the default build stays FFI-free and portable.
- The FFI layer lives in its own module with a narrow surface: connect
  (MQCONNX with TLS channel definition), put (MQPUT, syncpoint), get
  (MQGET with wait interval), disconnect. Nothing else of MQI is exposed.
- **The IBM client library is never vendored** — users install it and the
  build script locates it (`MQ_HOME`/pkg-config style), keeping the repo
  free of IBM-licensed bits.

## Mapping gateway concepts to MQ

| Gateway concept | MQ realization |
|---|---|
| `submit`/`query` (fire-and-forget) | MQPUT of the `FedNowIncoming` envelope to the participant's OUT queue, under syncpoint; commit = handoff confirmed → outbox `Published` |
| `poll_advice` | MQGET with wait interval on the IN queue(s); each message drained into the store before ack (get under syncpoint, commit after durable write) |
| `PortError::Transport` (ambiguous) | connection break between PUT and commit — exactly the ambiguity the reconciler resolves via pacs.028; **never resend** |
| Correlation id (admi.002) | persist MQMD MsgId/CorrelId alongside the outbox entry and inbound events |
| Reconnect discipline | drop → immediate reconnect loop with jittered backoff; connection state exposed as a health metric |
| Concurrency | N sender connections + M drainer connections, both configurable; drainers feed the same `pump`/store path |

Queue names, channel names, cipher suites and certificate provisioning are
participant-specific and arrive with onboarding (FedLine). They enter the
gateway as configuration, never as code:

```toml
[southbound.mq]
queue_manager = "..."       # from onboarding
channel = "..."
connection_name = "host(port)[,host(port)]"
out_queue = "..."           # "to FedNow" — naming controlled by the service
in_queues = ["..."]         # "from FedNow" — participant-customizable
key_repository = "/path/to/mtls/keystore"   # FRS-issued client certificate
```

## Testing strategy

1. **Contract tests stay on the sim** (already green): everything above
   `FedNowPort` is transport-agnostic and keeps running against
   `MqSimPort` in every CI run.
2. **Adapter integration tests** run against the free **IBM MQ Advanced
   for Developers container** (`icr.io/ibm-messaging/mq`) in an optional CI
   job gated on the `ibm-mq` feature: create a queue pair, PUT/GET
   envelopes, kill the connection mid-flight and assert the
   reconnect-and-reconcile arc. This validates MQI usage and the
   ambiguous-failure path without any Fed connectivity.
3. **mTLS**: the developer container supports TLS channels with
   self-signed material; the test provisions a throwaway keystore so the
   TLS code path is exercised in CI, not first exercised at onboarding.

## Phasing

- **Phase 1** — FFI foundation: `ibm-mq` feature, build-script discovery of
  the client library, connect/put/get/commit wrappers, developer-container
  smoke test in the optional CI job.
- **Phase 2** — `MqPort` adapter: `FedNowPort` implementation over the
  wrappers, correlation-id persistence, reconnect loop, config surface.
- **Phase 3** — operational hardening: concurrent senders/drainers,
  health/metrics (OpenTelemetry), chaos test (connection kill mid-PUT).
- Signing slots in **orthogonally** once issue #14 resolves: the envelope
  layer already preserves wire bytes, and the signature travels outside the
  XML (MQ property or technical header — the missing fact).

## Non-goals

- Operating an MQ *server* — participants talk to Fed-hosted queue
  managers; the sim remains the local stand-in.
- Supporting FedLine network provisioning — that is onboarding, not code.
