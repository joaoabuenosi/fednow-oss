package io.github.joaoabuenosi.fednow;

import java.util.List;

/** Base class for gateway-reported errors. Unchecked by design. */
public class GatewayException extends RuntimeException {

    public GatewayException(String message) {
        super(message);
    }

    public GatewayException(String message, Throwable cause) {
        super(message, cause);
    }

    /**
     * The payment fails the FedNow Release 1 profile (HTTP 422).
     * {@link #codes()} carries the gateway's stable rule identifiers — every
     * violation at once, not just the first.
     */
    public static final class ProfileViolation extends GatewayException {
        private final List<String> codes;

        public ProfileViolation(List<String> codes) {
            super("FedNow profile violation: " + String.join(", ", codes));
            this.codes = List.copyOf(codes);
        }

        public List<String> codes() {
            return codes;
        }
    }

    /** No payment exists under this idempotency key (HTTP 404). */
    public static final class UnknownPayment extends GatewayException {
        public UnknownPayment(String detail) {
            super(detail);
        }
    }

    /**
     * {@link GatewayClient#waitFinal} gave up before the payment reached a
     * final state. {@link #lastPayment()} is the last state observed.
     */
    public static final class WaitTimeout extends GatewayException {
        private final transient Payment lastPayment;

        public WaitTimeout(String message, Payment lastPayment) {
            super(message);
            this.lastPayment = lastPayment;
        }

        public Payment lastPayment() {
            return lastPayment;
        }
    }
}
