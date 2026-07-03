# fednow-gateway-client (Java)

Java 17+ client for the [fednow-gateway](../../gateway/) REST API. One
runtime dependency (Jackson); HTTP via the JDK's `java.net.http`.

```java
import io.github.joaoabuenosi.fednow.*;

var gw = new GatewayClient("http://localhost:8090");

var payment = gw.submit("order-2026-0001",          // idempotency key — mandatory
    SubmitPaymentRequest.builder()
        .reference("ORDER0001")
        .amountCents(125_000)                        // integer cents, never floats
        .debtorName("Jane Example").debtorAccount("123456789012")
        .creditorName("John Example").creditorAccount("987654321000")
        .creditorAgentRoutingNumber("091000019")
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
- **Profile violations are exceptions with rule codes.**
  `GatewayException.ProfileViolation.codes()` carries the gateway's stable
  identifiers (`fednow.ctgypurp.known`, `fednow.aba.checksum`, …) — every
  violation at once, before anything reaches the wire.

## Build / test

```sh
mvn -f sdk/java/pom.xml test          # unit tests (JDK stub server, no gateway)

# integration against the live stack (see QUICKSTART.md at the repo root):
FEDNOW_GW_URL=http://localhost:8090 mvn -f sdk/java/pom.xml test
```

The integration tests run in CI against a real gateway↔simulator pair in
MQ mode on every commit. Not yet published to Maven Central — build from
source for now.
