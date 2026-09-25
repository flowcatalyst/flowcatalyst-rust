package io.flowcatalyst.sdk.tsid;

import java.util.concurrent.ThreadLocalRandom;

/**
 * Lightweight TSID (Time-Sorted ID) generator, byte-for-byte compatible with
 * the TypeScript SDK: 13-character Crockford Base32 strings from a 64-bit
 * value composed of 42 bits of timestamp (custom epoch 2020-01-01T00:00:00Z)
 * + 22 bits of randomness.
 */
public final class Tsid {

    private static final String CROCKFORD_ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    private static final int TSID_LENGTH = 13;
    private static final int RANDOM_BITS = 22;
    private static final int RANDOM_MASK = (1 << RANDOM_BITS) - 1;
    private static final long CUSTOM_EPOCH = 1577836800000L;

    /**
     * Packed (millis, sequence) state for collision-free generation: the
     * sequence is randomly seeded each new millisecond and incremented within
     * it (borrowing into the next millisecond on overflow) — same scheme as
     * the platform's Go generator, same wire layout as the TypeScript SDK.
     */
    private static final java.util.concurrent.atomic.AtomicLong STATE = new java.util.concurrent.atomic.AtomicLong(0);

    private Tsid() {}

    /** Generate a new TSID as a 13-character Crockford Base32 string. */
    public static String generate() {
        while (true) {
            long previous = STATE.get();
            long previousMs = previous >>> RANDOM_BITS;
            long nowMs = System.currentTimeMillis() - CUSTOM_EPOCH;

            long next;
            if (nowMs > previousMs) {
                next = (nowMs << RANDOM_BITS)
                        | ThreadLocalRandom.current().nextInt(RANDOM_MASK + 1);
            } else {
                // Same (or clock-rewound) millisecond: increment; on sequence
                // wrap, borrow into the next millisecond.
                next = previous + 1;
            }
            if (STATE.compareAndSet(previous, next)) {
                return encodeCrockford(next);
            }
        }
    }

    /**
     * Generate a BRANDED (typed) TSID: {@code {prefix}_{raw}} — matching the
     * FlowCatalyst platform convention (e.g. {@code aud_…}, {@code prn_…}).
     * Use a short lowercase prefix for your own entities, e.g.
     * {@code generateWithPrefix("cmt")} → {@code cmt_6F7JC2A6JFR7N}.
     *
     * @throws IllegalArgumentException if the prefix is empty or contains an underscore
     */
    public static String generateWithPrefix(String prefix) {
        if (prefix == null || prefix.isEmpty() || prefix.contains("_")) {
            throw new IllegalArgumentException(
                    "TSID prefix must be non-empty and contain no underscore.");
        }
        return prefix + "_" + generate();
    }

    /** Validate that a string is a valid (unbranded) TSID format. */
    public static boolean isValid(String tsid) {
        if (tsid == null || tsid.length() != TSID_LENGTH) return false;
        String upper = tsid.toUpperCase(java.util.Locale.ROOT);
        for (int i = 0; i < upper.length(); i++) {
            if (CROCKFORD_ALPHABET.indexOf(upper.charAt(i)) < 0) return false;
        }
        return true;
    }

    private static String encodeCrockford(long value) {
        char[] chars = new char[TSID_LENGTH];
        long remaining = value;
        for (int i = TSID_LENGTH - 1; i >= 0; i--) {
            chars[i] = CROCKFORD_ALPHABET.charAt((int) (remaining & 31L));
            remaining >>>= 5;
        }
        return new String(chars);
    }
}
