package io.github.joaoabuenosi.fednow;

import java.util.Objects;

/**
 * A payment submission. Build with {@link #builder()}; all fields without a
 * default are required and checked at {@code build()} time.
 *
 * <p>Amounts are <b>integer cents</b> — there is deliberately no way to pass
 * a floating-point amount.
 */
public final class SubmitPaymentRequest {

    final String reference;
    final long amountCents;
    final String debtorName;
    final String debtorAccount;
    final String creditorName;
    final String creditorAccount;
    final String creditorAgentRoutingNumber;
    final String categoryPurpose;
    final String endToEndIdentification; // nullable
    final String uetr; // nullable

    private SubmitPaymentRequest(Builder b) {
        this.reference = require(b.reference, "reference");
        if (b.amountCents <= 0) {
            throw new IllegalArgumentException("amountCents must be positive (integer cents)");
        }
        this.amountCents = b.amountCents;
        this.debtorName = require(b.debtorName, "debtorName");
        this.debtorAccount = require(b.debtorAccount, "debtorAccount");
        this.creditorName = require(b.creditorName, "creditorName");
        this.creditorAccount = require(b.creditorAccount, "creditorAccount");
        this.creditorAgentRoutingNumber =
                require(b.creditorAgentRoutingNumber, "creditorAgentRoutingNumber");
        this.categoryPurpose = b.categoryPurpose;
        this.endToEndIdentification = b.endToEndIdentification;
        this.uetr = b.uetr;
    }

    private static String require(String value, String name) {
        if (value == null || value.isBlank()) {
            throw new IllegalArgumentException(name + " is required");
        }
        return value;
    }

    public static Builder builder() {
        return new Builder();
    }

    public static final class Builder {
        private String reference;
        private long amountCents;
        private String debtorName;
        private String debtorAccount;
        private String creditorName;
        private String creditorAccount;
        private String creditorAgentRoutingNumber;
        private String categoryPurpose = "CONS";
        private String endToEndIdentification;
        private String uetr;

        public Builder reference(String v) {
            this.reference = v;
            return this;
        }

        /** Integer cents — never a float. */
        public Builder amountCents(long v) {
            this.amountCents = v;
            return this;
        }

        public Builder debtorName(String v) {
            this.debtorName = v;
            return this;
        }

        public Builder debtorAccount(String v) {
            this.debtorAccount = v;
            return this;
        }

        public Builder creditorName(String v) {
            this.creditorName = v;
            return this;
        }

        public Builder creditorAccount(String v) {
            this.creditorAccount = v;
            return this;
        }

        public Builder creditorAgentRoutingNumber(String v) {
            this.creditorAgentRoutingNumber = v;
            return this;
        }

        /** {@code CONS} (default) or {@code BIZZ}. */
        public Builder categoryPurpose(String v) {
            this.categoryPurpose = Objects.requireNonNull(v);
            return this;
        }

        public Builder endToEndIdentification(String v) {
            this.endToEndIdentification = v;
            return this;
        }

        public Builder uetr(String v) {
            this.uetr = v;
            return this;
        }

        public SubmitPaymentRequest build() {
            return new SubmitPaymentRequest(this);
        }
    }
}
