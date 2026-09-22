package io.github.joaoabuenosi.fednow;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;

/**
 * Client for one fednow-gateway instance.
 *
 * <pre>{@code
 * var gw = new GatewayClient("http://localhost:8090", System.getenv("FEDNOW_GW_API_KEY"));
 * var payment = gw.submit("order-2026-0001", SubmitPaymentRequest.builder()
 *         .reference("ORDER0001")
 *         .amountCents(125_000)
 *         .debtorName("Jane Example").debtorAccount("123456789012")
 *         .creditorName("John Example").creditorAccount("987654321000")
 *         .creditorAgentRoutingNumber("992000008")
 *         .build());
 * // ACK_PENDING: the answer is asynchronous (MQ semantics).
 * var settled = gw.waitFinal("order-2026-0001");
 * }</pre>
 *
 * <p>The client mirrors the gateway's operating rules instead of hiding
 * them: the idempotency key is a required argument of {@link #submit} (and
 * resubmitting a key is always safe — nothing touches the wire twice), and
 * {@link #waitFinal} knows that {@code TIMEOUT_UNRESOLVED} is not final —
 * the gateway's reconciler is resolving it via pacs.028, never a resend.
 *
 * <p>Every call except {@link #healthy} needs an API key, sent as
 * {@code Authorization: Bearer <key>}: without a valid one the gateway answers
 * 401 ({@link GatewayException.Unauthorized}), and a read-only key on a write
 * gets 403 ({@link GatewayException.Forbidden}).
 */
public final class GatewayClient {

    private static final ObjectMapper JSON = new ObjectMapper();

    private final String baseUrl;
    private final HttpClient http;
    private final Duration requestTimeout;
    /** Never logged and never part of an exception message. May be null. */
    private final String apiKey;

    /** A client that sends no API key — only {@link #healthy} will work. */
    public GatewayClient(String baseUrl) {
        this(baseUrl, Duration.ofSeconds(10), null);
    }

    /** A client that sends no API key — only {@link #healthy} will work. */
    public GatewayClient(String baseUrl, Duration requestTimeout) {
        this(baseUrl, requestTimeout, null);
    }

    /** A client authenticating with {@code apiKey} (10s request timeout). */
    public GatewayClient(String baseUrl, String apiKey) {
        this(baseUrl, Duration.ofSeconds(10), apiKey);
    }

    /**
     * @param apiKey one of the gateway's {@code FEDNOW_GW_API_KEYS} (or a
     *     read-only key for read-only use); {@code null} sends none
     */
    public GatewayClient(String baseUrl, Duration requestTimeout, String apiKey) {
        this.apiKey = apiKey;
        this.baseUrl = baseUrl.endsWith("/")
                ? baseUrl.substring(0, baseUrl.length() - 1)
                : baseUrl;
        this.requestTimeout = requestTimeout;
        this.http = HttpClient.newBuilder().connectTimeout(requestTimeout).build();
    }

    /**
     * Submit a payment, idempotently: resubmitting the same key returns the
     * payment as it stands without sending anything again.
     *
     * @throws GatewayException.ProfileViolation when the payment fails
     *     FedNow Release 1 validation (nothing reached the wire)
     */
    public Payment submit(String idempotencyKey, SubmitPaymentRequest request) {
        ObjectNode body = JSON.createObjectNode();
        body.put("reference", request.reference);
        body.put("amount_cents", request.amountCents);
        body.put("debtor_name", request.debtorName);
        body.put("debtor_account", request.debtorAccount);
        body.put("creditor_name", request.creditorName);
        body.put("creditor_account", request.creditorAccount);
        body.put("creditor_agent_routing_number", request.creditorAgentRoutingNumber);
        body.put("category_purpose", request.categoryPurpose);
        if (request.endToEndIdentification != null) {
            body.put("end_to_end_identification", request.endToEndIdentification);
        }
        if (request.uetr != null) {
            body.put("uetr", request.uetr);
        }
        HttpRequest req = newRequest("/payments")
                .header("Idempotency-Key", idempotencyKey)
                .header("content-type", "application/json")
                .POST(HttpRequest.BodyPublishers.ofString(body.toString()))
                .build();
        return Payment.fromJson(send(req));
    }

    /** Current state of a payment. */
    public Payment get(String idempotencyKey) {
        HttpRequest req = newRequest("/payments/" + idempotencyKey).GET().build();
        return Payment.fromJson(send(req));
    }

    /**
     * Drive one reconciliation pass now (the gateway also does this on its
     * background sweeper — calling it is never required).
     */
    public Payment reconcile(String idempotencyKey) {
        HttpRequest req = newRequest("/payments/" + idempotencyKey + "/reconcile")
                .POST(HttpRequest.BodyPublishers.noBody())
                .build();
        return Payment.fromJson(send(req));
    }

    public boolean healthy() {
        try {
            // The liveness probe is public: no credential sent.
            HttpRequest req = HttpRequest.newBuilder(URI.create(baseUrl + "/healthz"))
                    .timeout(requestTimeout).GET().build();
            return http.send(req, HttpResponse.BodyHandlers.ofString()).statusCode() == 200;
        } catch (IOException e) {
            return false;
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return false;
        }
    }

    /** {@link #waitFinal(String, Duration, Duration)} with 120s / 1s. */
    public Payment waitFinal(String idempotencyKey) {
        return waitFinal(idempotencyKey, Duration.ofSeconds(120), Duration.ofSeconds(1));
    }

    /**
     * Poll until the payment reaches a final state ({@link Payment#FINAL_STATES}).
     *
     * <p>{@code TIMEOUT_UNRESOLVED} is <em>not</em> final: the gateway's
     * reconciler is resolving it via pacs.028, so this keeps waiting.
     *
     * @throws GatewayException.WaitTimeout when {@code timeout} elapses
     *     first; carries the last-seen payment
     */
    public Payment waitFinal(String idempotencyKey, Duration timeout, Duration pollInterval) {
        long deadline = System.nanoTime() + timeout.toNanos();
        Payment payment = get(idempotencyKey);
        while (!payment.isFinal()) {
            if (System.nanoTime() >= deadline) {
                throw new GatewayException.WaitTimeout(
                        "payment '" + idempotencyKey + "' still " + payment.state()
                                + " after " + timeout.toSeconds() + "s",
                        payment);
            }
            try {
                Thread.sleep(pollInterval.toMillis());
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                throw new GatewayException("interrupted while waiting", e);
            }
            payment = get(idempotencyKey);
        }
        return payment;
    }

    // -- Plumbing ----------------------------------------------------------

    private HttpRequest.Builder newRequest(String path) {
        HttpRequest.Builder builder =
                HttpRequest.newBuilder(URI.create(baseUrl + path)).timeout(requestTimeout);
        if (apiKey != null) {
            builder.header("Authorization", "Bearer " + apiKey);
        }
        return builder;
    }

    private JsonNode send(HttpRequest request) {
        HttpResponse<String> response;
        try {
            response = http.send(request, HttpResponse.BodyHandlers.ofString());
        } catch (IOException e) {
            throw new GatewayException("transport failure: " + e.getMessage(), e);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new GatewayException("interrupted", e);
        }
        int status = response.statusCode();
        String body = response.body();
        if (status == 401) {
            throw new GatewayException.Unauthorized(body);
        }
        if (status == 403) {
            throw new GatewayException.Forbidden(body);
        }
        if (status == 404) {
            throw new GatewayException.UnknownPayment(
                    body.isBlank() ? "unknown payment" : body);
        }
        if (status == 422) {
            throw new GatewayException.ProfileViolation(parseCodes(body));
        }
        if (status >= 400) {
            throw new GatewayException("HTTP " + status + ": " + body);
        }
        try {
            return JSON.readTree(body);
        } catch (IOException e) {
            throw new GatewayException("unparseable response: " + e.getMessage(), e);
        }
    }

    private static List<String> parseCodes(String body) {
        List<String> codes = new ArrayList<>();
        try {
            JsonNode node = JSON.readTree(body).path("codes");
            if (node.isArray()) {
                node.forEach(c -> codes.add(c.asText()));
            }
        } catch (IOException ignored) {
            // fall through with whatever we collected
        }
        return codes;
    }
}
