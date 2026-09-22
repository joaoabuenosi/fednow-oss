package io.github.joaoabuenosi.fednow;

import com.fasterxml.jackson.databind.JsonNode;
import java.util.Set;

/**
 * The gateway's view of one payment.
 *
 * <p>{@code SETTLED}, {@code REJECTED}, {@code HELD} and {@code REFUSED} are
 * the final states. {@code HELD} and {@code REFUSED} only occur when the
 * gateway runs a pre-send risk check; nothing was sent, and this gateway
 * version has no route that sends it later.
 * {@code TIMEOUT_UNRESOLVED} is a work item the gateway's reconciler resolves
 * via pacs.028 — {@link GatewayClient#waitFinal} keeps waiting through it.
 */
public record Payment(
        String idempotencyKey,
        String state,
        String messageIdentification,
        String endToEndIdentification,
        String uetr,
        int queriesSent,
        String rejectionReason,
        int events) {

    /** States no advice can change anymore. */
    public static final Set<String> FINAL_STATES =
            Set.of("SETTLED", "REJECTED", "HELD", "REFUSED");

    public boolean isFinal() {
        return FINAL_STATES.contains(state);
    }

    static Payment fromJson(JsonNode node) {
        return new Payment(
                node.path("idempotency_key").asText(),
                node.path("state").asText(),
                node.path("message_identification").asText(),
                node.path("end_to_end_identification").asText(),
                textOrNull(node, "uetr"),
                node.path("queries_sent").asInt(0),
                textOrNull(node, "rejection_reason"),
                node.path("events").asInt(0));
    }

    private static String textOrNull(JsonNode node, String field) {
        JsonNode value = node.path(field);
        return value.isMissingNode() || value.isNull() ? null : value.asText();
    }
}
