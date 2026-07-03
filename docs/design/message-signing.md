# Design: FedNow message signing in fednow-core

*Status: operational model confirmed; wire format pending (Technical
Specifications) · July 2026*

Every message exchanged with the FedNow Service must be cryptographically signed,
and the service validates both the signature and the binding between the sending
entity and the key ([Technical Overview and Planning Guide](https://explore.fednow.org/resources/technical-overview-guide.pdf),
"Message Signing"). This document records what is publicly confirmed about the
signing scheme, what is not, and how we sequence the work so `fednow-core` makes
progress without betting on unconfirmed details.

## Confirmed by public Federal Reserve documentation

1. **All messages are signed** — both participant-to-service and service-to-participant.
2. **Asymmetric key pairs, participant-generated.** Participants create the pairs
   with "specifications defined by the FedNow Service" using standard tooling
   (OpenSSL, Java, any KMS). This is a registered-key model, not a Fed-issued
   certificate chain: first-time public-key registration happens through the FedNow
   interface, later rotations via key-exchange messages over MQ (the
   `FedNowKeyExchange` message set exists for this).
3. **Private-key confidentiality is an Operating Circular 8 obligation.**
4. **Every FedNow business message carries a Business Application Header**
   (head.001.001.02). The BAH schema's `Sgntr` element (`SignatureEnvelope`,
   a lax `xs:any`) is where ISO 20022 signatures travel by construction.
5. mTLS on the transport and the message signature are **independent layers**;
   one does not replace the other.
6. **Signing is point-to-point, not end-to-end** — each signature covers one
   leg between a participant (or its service provider) and the service; the
   receiving participant verifies the *service's* signature, not the sender's
   ([Readiness Guide: Information Security](https://explore.fednow.org/resources/readiness-guide-information-security.pdf),
   "Message Signing and Key Pairs").
7. **The signer may be the FI, its service provider, or the service** — a
   gateway signing on behalf of a participant is an arrangement the service
   design explicitly anticipates (same source). The signing tools and
   instructions (including key creation/management details) are distributed
   **during onboarding** — confirming there is no self-service public channel
   for the wire format.

## Operational model (FedNow Service Operating Procedures, §8 "Message Signing")

Confirmed from the public Operating Procedures (June 2024 edition,
frbservices.org):

- **Everything is signed except the Participant Broadcast Ping (admi.004)** —
  and the service does not validate the signature of a ping. Same requirements
  in the test and production environments (with distinct key pairs per
  environment).
- **Key lifecycle:** the *first* key pair (or a recovery when all keys expired)
  is established via the FedNow interface with phone validation by the Support
  Center; subsequent pairs are added **via MQ** (the `FedNowKeyExchange`
  message set), which requires an explicit expiration date, capped at **364
  days**. Revocation via interface or MQ (`Revoke/Compromise` action).
  Owners may hold many active keys; multiple active keys with staggered
  expirations is the recommended practice. Service providers may share one key
  across FIs or use per-FI (target RTN) keys.
- **Key identity:** a key has *key*, *key name* and *key id* — and for FedNow
  Service keys, **key name equals key id**. Receivers must check that the key
  id of an inbound message matches an active key in their list *before*
  validating the signature, and validate before processing.
- **Service key distribution:** initial list via the FedNow interface; updates
  via interface or MQ; a `FNKY` FedNow Broadcast (admi.004) announces each new
  service key. The service signs with its **oldest active key** and rolls to
  the next as expiration nears.
- **Failure handling:** unsigned / expired-key / unknown-key messages are
  rejected with admi.002 and an error code; correlation with the rejected
  message uses the **MQ correlation id in the technical message header**
  (Implementation Guide) — further evidence the signature travels in the MQ
  layer, outside the ISO payload. Participants receiving bad signatures from
  the service must reject and call the Support Center (the service initially
  does not process inbound admi.002).

Gateway design implications (for later): the key store must track (key id,
public key, expiration, environment) for both own keys and service keys; key
rotation is an MQ flow the gateway must speak; the verifier needs a "key id →
active key" lookup that fails closed.

## Not yet confirmed: the exact signature wire format

The normative format (algorithms, what exactly is digested, envelope shape) lives
in the **FedNow Service Technical Specifications**. That document is *not* in the
MyStandards usage-guideline collection (verified July 2026) and is not in the
public FedNow Explorer resource library either: per the Information Security
Readiness Guide, signing tools and instructions are distributed **during
participant onboarding** (FedLine / frbservices.org, access-controlled). Tracked
as issue #14; realistic acquisition paths are a design partner with legitimate
access reporting interoperability facts, or contact through the FedNow Community
channels. Its content must not be pasted into this repo regardless of how it is
obtained.

Two candidate shapes exist in the wild:

**(a) ISO/SWIFT-style XMLDSig** — `ds:Signature` inside `AppHdr/Sgntr`;
exclusive C14N; `rsa-sha256`; SHA-256 digests; three references (the `KeyInfo` by
Id, the `AppHdr` itself with the enveloped-signature transform, and the `Document`
as a URI-less reference). This is the profile used by CBPR+/SWIFT tooling and
Mastercard's open-source ISO 20022 signer, and it is what the `SignatureEnvelope`
element was designed to carry. *Evidence: strong for the ISO ecosystem in general;
indirect for FedNow specifically.*

**(b) Detached JWS (RFC 7515/7797)** — RS256, `b64=false`, key id in the protected
header. Community projects (e.g. `open-fednow`) implement this, but over an
HTTP/JSON reinterpretation of FedNow, not the ISO 20022 XML-over-MQ channel — so
this is weak evidence for the wire format.

**Update 1 (schema evidence):** the vendored `head.001.001.02.xsd` defines
`SignatureEnvelope` as `xs:any namespace="http://www.w3.org/2000/09/xmldsig#"
processContents="lax"` — in the *base* ISO schema, `Sgntr` content is
namespace-locked to W3C XMLDSig.

**Update 2 (FedNow usage guideline + envelope evidence, July 2026 — CONFIRMED):**
the FedNow Service Release 1 BAH usage guideline (verified against the
MyStandards schema export) *removes* the `Sgntr` element entirely (alongside
`CharSet`, `PssblDplct` and `Prty`), and the FedNow MQ technical envelope
(`FedNowIncoming`/`FedNowOutgoing`: a typed wrapper of `AppHdr` + `Document`,
plus an open `FedNowTechnicalHeader`) has **no per-message signature element**
either — its signature-related members exist only for key-exchange operations
(`FedNowKeyID` up to 300 alphanumeric chars; algorithm expressed as strings like
`RSA-2048`). Consequence: the signature almost certainly travels **outside the
XML business message** — as an MQ message property or in the technical header —
which flips the default assumption from (a) XMLDSig to **(b) a detached
JWS-style signature over the wire bytes**. The exact transport slot, protected
header and signing input remain to be confirmed in the Technical Specifications
document (not in the per-message guidelines).

**Invariants that hold under either profile** (safe to build against now):
RSA ≥ 2048 with SHA-256; the signature is detached; the signer is identified by
a `FedNowKeyID` registered with the service; signing must operate on the exact
wire bytes — never on re-serialized models.

**Licensing note:** MyStandards usage-guideline content and the Fed's envelope
schemas are access-controlled/confidential material. This repo records only the
minimal interoperability facts needed to implement, never reproduces them, and
vendors only base ISO 20022 schemas.

## Sequencing decision

1. **Now — BAH support (not blocked).** Model + parse + validate head.001.001.02
   in `fednow-core`: `Fr`/`To` (routing numbers), `BizMsgIdr`, `MsgDefIdr`,
   `CreDt`, and `Sgntr` preserved as raw XML for round-tripping. Every later piece
   (simulator, gateway) needs the BAH regardless of the signing profile.
2. **Done (July 2026):** MyStandards account created and the Release 1
   usage-guideline collection fully mined (all Release 1 profiles calibrated).
   The Technical Specifications are **not** distributed there — see issue #14
   for the open acquisition paths. What we still need from it: signature
   transport slot (MQ property vs technical header), protected header /
   signing-input definition, and how the key id is expressed on the wire.
2b. **Roadmap note:** MyStandards also lists a planned **"FedNow Service 2026
   Enhanced Messages Release" (Q4 2026)** — track it; message versions may move
   beyond the 2019 set this crate currently targets.

3. **Then — implement the confirmed profile** in a `sign` module:
   - If XMLDSig: implement **exclusive C14N in pure Rust** over the quick-xml event
     stream, scoped to the subset ISO 20022 messages actually use (no DTDs, no
     processing instructions, controlled namespaces). No `libxmlsec1` system
     dependency: it would hurt the multi-platform build and auditability, and a
     scoped C14N is tractable and independently valuable (verification of inbound
     pacs.002 advices needs it too).
   - If JWS: `rsa` + `sha2` + base64url, considerably simpler; C14N is avoided
     entirely.
   - Either way: signing and verification land together, with test vectors checked
     into `core/tests/fixtures/` and — once the simulator exists — round-trip tests
     through `fednow-sim`.

## Non-goals

- No support for Fed-restricted schema content in this repo (licensing).
- No HSM/KMS integration in `fednow-core` itself — the signing API takes a signer
  trait so the gateway can plug HSM-backed keys later without touching the core.
