# fednow-gateway-client (Java)

Java 17+ client for the [fednow-gateway](../../gateway/) REST API. One
runtime dependency (Jackson); HTTP via the JDK's `java.net.http`.

```java
import io.github.joaoabuenosi.fednow.*;

var gw = new GatewayClient("http://localhost:8090", System.getenv("FEDNOW_GW_API_KEY"));

var payment = gw.submit("order-2026-0001",          // idempotency key — mandatory
    SubmitPaymentRequest.builder()
        .reference("ORDER0001")
        .amountCents(125_000)                        // integer cents, never floats
        .debtorName("Jane Example").debtorAccount("123456789012")
        .creditorName("John Example").creditorAccount("987654321000")
        .creditorAgentRoutingNumber("992000008")
        .build());
System.out.println(payment.state());                 // ACK_PENDING — the answer is async

var settled = gw.waitFinal("order-2026-0001");
System.out.println(settled.state());                 // SETTLED (or REJECTED + rejectionReason)
```

The client mirrors the gateway's operating rules instead of hiding them:

- **Idempotency first.** The key is a required argument of `submit`; calling
  it again with the same key returns the payment as it stands — safe inside
  any retry loop.
- **`waitFinal` understands the timeout case.** `TIMEOUT_UNRESOLVED` is not
  final: the gateway's reconciler is resolving it with a pacs.028 status
  request (never a resend), so the client keeps polling through it.
  `HELD`, `REFUSED` and `CANCELLED`, which only a gateway with its optional
  [pre-send risk check](../../gateway/README.md#pre-send-risk-check) produces,
  return at once: nothing was sent. `HELD` is not terminal (an operator key
  can release or cancel it), but it waits for a person, not for an advice;
  call `waitFinal` again after a release. Releasing is an operator action, so the
  SDKs, which hold a submitting key, deliberately have no `release` call.
- **Authenticated.** Pass the API key (one of the gateway's
  `FEDNOW_GW_API_KEYS`, or a read-only key for monitoring) to the
  constructor; it is sent as `Authorization: Bearer <key>` on every call
  except `healthy()`. A missing or unknown key throws
  `GatewayException.Unauthorized` (401), a read-only key on a write throws
  `GatewayException.Forbidden` (403). The key is never included in an
  exception message.
- **Profile violations are exceptions with rule codes.**
  `GatewayException.ProfileViolation.codes()` carries the gateway's stable
  identifiers (`fednow.ctgypurp.known`, `fednow.aba.checksum`, …) — every
  violation at once, before anything reaches the wire.

## Build / test

```sh
mvn -f sdk/java/pom.xml test          # unit tests (JDK stub server, no gateway)

# integration against the live stack (see QUICKSTART.md at the repo root);
# FEDNOW_GW_API_KEY must be a key the gateway was started with:
FEDNOW_GW_URL=http://localhost:8090 FEDNOW_GW_API_KEY=... mvn -f sdk/java/pom.xml test
```

The integration tests run in CI against a real gateway↔simulator pair in
MQ mode on every commit. Not yet published to Maven Central — build from
source for now.
