package io.github.joaoabuenosi.fednow;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.List;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

/** Unit tests against a JDK-builtin stub server — no gateway required. */
class GatewayClientTest {

    private static final String PAYMENT_JSON =
            """
            {"idempotency_key":"k1","state":"%s",
             "message_identification":"20260703021040078QS0001",
             "end_to_end_identification":"QS0001","uetr":null,
             "queries_sent":0,"rejection_reason":null,"events":%d}
            """;

    private HttpServer server;
    private GatewayClient client;
    private final AtomicInteger gets = new AtomicInteger();
    private volatile String lastIdempotencyKey;

    @BeforeEach
    void start() throws IOException {
        server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/payments", this::handle);
        server.createContext("/healthz", ex -> respond(ex, 200, "ok"));
        server.start();
        client = new GatewayClient(
                "http://127.0.0.1:" + server.getAddress().getPort(), Duration.ofSeconds(5));
    }

    @AfterEach
    void stop() {
        server.stop(0);
    }

    private void handle(HttpExchange ex) throws IOException {
        String path = ex.getRequestURI().getPath();
        String body = new String(ex.getRequestBody().readAllBytes(), StandardCharsets.UTF_8);
        if ("POST".equals(ex.getRequestMethod()) && path.equals("/payments")) {
            lastIdempotencyKey = ex.getRequestHeaders().getFirst("Idempotency-Key");
            if (lastIdempotencyKey == null) {
                respond(ex, 400, "the Idempotency-Key header is mandatory");
            } else if (body.contains("\"WRONG\"")) {
                respond(ex, 422,
                        "{\"error\":\"fednow_profile_violation\","
                                + "\"codes\":[\"fednow.ctgypurp.known\"]}");
            } else {
                respond(ex, 200, PAYMENT_JSON.formatted("ACK_PENDING", 4));
            }
        } else if ("GET".equals(ex.getRequestMethod()) && path.equals("/payments/k1")) {
            // First poll pending, then settled — exercises waitFinal.
            boolean settled = gets.incrementAndGet() >= 2;
            respond(ex, 200, PAYMENT_JSON.formatted(settled ? "SETTLED" : "ACK_PENDING",
                    settled ? 5 : 4));
        } else {
            respond(ex, 404, "unknown payment");
        }
    }

    private static void respond(HttpExchange ex, int status, String body) throws IOException {
        byte[] bytes = body.getBytes(StandardCharsets.UTF_8);
        ex.getResponseHeaders().set("content-type", "application/json");
        ex.sendResponseHeaders(status, bytes.length);
        try (OutputStream os = ex.getResponseBody()) {
            os.write(bytes);
        }
        ex.close();
    }

    private SubmitPaymentRequest.Builder validRequest() {
        return SubmitPaymentRequest.builder()
                .reference("QS0001")
                .amountCents(125_000)
                .debtorName("Jane")
                .debtorAccount("123456789012")
                .creditorName("John")
                .creditorAccount("987654321000")
                .creditorAgentRoutingNumber("091000019");
    }

    @Test
    void submitParsesPaymentAndSendsKeyHeader() {
        Payment p = client.submit("k1", validRequest().build());
        assertEquals("ACK_PENDING", p.state());
        assertFalse(p.isFinal());
        assertEquals("k1", lastIdempotencyKey);
    }

    @Test
    void profileViolationCarriesCodes() {
        var exc = assertThrows(GatewayException.ProfileViolation.class,
                () -> client.submit("k1", validRequest().categoryPurpose("WRONG").build()));
        assertEquals(List.of("fednow.ctgypurp.known"), exc.codes());
    }

    @Test
    void unknownPaymentThrows() {
        assertThrows(GatewayException.UnknownPayment.class, () -> client.get("nope"));
    }

    @Test
    void waitFinalPollsThroughPending() {
        Payment p = client.waitFinal("k1", Duration.ofSeconds(10), Duration.ofMillis(10));
        assertEquals("SETTLED", p.state());
        assertTrue(p.isFinal());
    }

    @Test
    void waitTimeoutCarriesLastPayment() throws IOException {
        // A stub that always answers pending.
        HttpServer pending = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        pending.createContext("/payments",
                ex -> respond(ex, 200, PAYMENT_JSON.formatted("ACK_PENDING", 4)));
        pending.start();
        try {
            var stuck = new GatewayClient(
                    "http://127.0.0.1:" + pending.getAddress().getPort());
            var exc = assertThrows(GatewayException.WaitTimeout.class,
                    () -> stuck.waitFinal("k1", Duration.ofMillis(50), Duration.ofMillis(10)));
            assertEquals("ACK_PENDING", exc.lastPayment().state());
        } finally {
            pending.stop(0);
        }
    }

    @Test
    void builderRejectsMissingFieldsAndBadAmounts() {
        assertThrows(IllegalArgumentException.class,
                () -> SubmitPaymentRequest.builder().build());
        assertThrows(IllegalArgumentException.class,
                () -> validRequest().amountCents(0).build());
    }

    @Test
    void healthyProbes() {
        assertTrue(client.healthy());
        assertFalse(new GatewayClient("http://127.0.0.1:1").healthy());
    }
}
