package io.github.joaoabuenosi.fednow;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.time.Duration;
import java.util.UUID;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.condition.EnabledIfEnvironmentVariable;

/**
 * Integration: the SDK against a live fednow-gateway (+ fednow-sim).
 * Enabled only when FEDNOW_GW_URL is set — CI launches the stack in MQ mode
 * with fast reconcile timings; see .github/workflows/ci.yml.
 */
@EnabledIfEnvironmentVariable(named = "FEDNOW_GW_URL", matches = ".+")
class IntegrationTest {

    private GatewayClient client;

    @BeforeEach
    void connect() {
        client = new GatewayClient(System.getenv("FEDNOW_GW_URL"));
        assertTrue(client.healthy(), "no gateway answering");
    }

    private Payment submit(String key, long amountCents) {
        String ref = "JVM" + UUID.randomUUID().toString()
                .replace("-", "").substring(0, 12).toUpperCase();
        return client.submit(key, SubmitPaymentRequest.builder()
                .reference(ref)
                .amountCents(amountCents)
                .debtorName("Jane Example")
                .debtorAccount("123456789012")
                .creditorName("John Example")
                .creditorAccount("987654321000")
                .creditorAgentRoutingNumber("091000019")
                .build());
    }

    @Test
    void settleAndIdempotentReplay() {
        String key = "jvm-it-" + UUID.randomUUID();
        Payment first = submit(key, 125_000);
        Payment settled = client.waitFinal(key, Duration.ofSeconds(30), Duration.ofMillis(500));
        assertEquals("SETTLED", settled.state());

        Payment replay = client.submit(key, SubmitPaymentRequest.builder()
                .reference(first.endToEndIdentification())
                .amountCents(125_000)
                .debtorName("Jane Example")
                .debtorAccount("123456789012")
                .creditorName("John Example")
                .creditorAccount("987654321000")
                .creditorAgentRoutingNumber("091000019")
                .build());
        assertEquals("SETTLED", replay.state());
        assertEquals(settled.events(), replay.events());
    }

    @Test
    void rejectionCarriesIsoReason() {
        String key = "jvm-it-" + UUID.randomUUID();
        submit(key, 125_011); // .11 → receiving bank rejects
        Payment fin = client.waitFinal(key, Duration.ofSeconds(30), Duration.ofMillis(500));
        assertEquals("REJECTED", fin.state());
        assertEquals("AC04", fin.rejectionReason());
    }

    @Test
    void timeoutArcResolvesWithoutResend() {
        String key = "jvm-it-" + UUID.randomUUID();
        submit(key, 125_033); // .33 → no advice until pacs.028 asks
        Payment fin = client.waitFinal(key, Duration.ofSeconds(60), Duration.ofMillis(500));
        assertEquals("SETTLED", fin.state());
        assertTrue(fin.queriesSent() >= 1, "resolved by status request, not resend");
    }

    @Test
    void profileViolationNeverReachesTheWire() {
        String key = "jvm-it-" + UUID.randomUUID();
        var exc = assertThrows(GatewayException.ProfileViolation.class,
                () -> client.submit(key, SubmitPaymentRequest.builder()
                        .reference("JVMBAD0001")
                        .amountCents(125_000)
                        .categoryPurpose("WRONG")
                        .debtorName("Jane Example")
                        .debtorAccount("123456789012")
                        .creditorName("John Example")
                        .creditorAccount("987654321000")
                        .creditorAgentRoutingNumber("091000019")
                        .build()));
        assertTrue(exc.codes().contains("fednow.ctgypurp.known"));
    }
}
