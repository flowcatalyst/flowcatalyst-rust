package io.flowcatalyst.sdk.outbox;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.flowcatalyst.sdk.tsid.Tsid;
import java.util.HashSet;
import java.util.Set;
import org.junit.jupiter.api.Test;

class TsidTest {

    private static final String ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    private static final long CUSTOM_EPOCH = 1577836800000L;

    @Test
    void generatesThirteenCharCrockford() {
        String tsid = Tsid.generate();
        assertEquals(13, tsid.length());
        assertTrue(Tsid.isValid(tsid), tsid);
    }

    @Test
    void embedsCurrentTimestampInUpper42Bits() {
        long before = System.currentTimeMillis();
        String tsid = Tsid.generate();
        long after = System.currentTimeMillis();

        long value = 0;
        for (char c : tsid.toCharArray()) {
            value = (value << 5) | ALPHABET.indexOf(c);
        }
        // Small slack after: sequence borrowing can nudge the embedded
        // millisecond ahead under bursts.
        long timestamp = (value >>> 22) + CUSTOM_EPOCH;
        assertTrue(timestamp >= before && timestamp <= after + 50,
                "decoded " + timestamp + " outside [" + before + ", " + after + "]");
    }

    @Test
    void sortsByGenerationTime() throws Exception {
        String first = Tsid.generate();
        Thread.sleep(2);
        String second = Tsid.generate();
        assertTrue(first.compareTo(second) < 0);
    }

    @Test
    void uniqueAcrossManyGenerations() {
        Set<String> seen = new HashSet<>();
        for (int i = 0; i < 10_000; i++) {
            assertTrue(seen.add(Tsid.generate()), "duplicate TSID at iteration " + i);
        }
    }

    @Test
    void brandedPrefixesFollowPlatformConvention() {
        String branded = Tsid.generateWithPrefix("cmt");
        assertTrue(branded.startsWith("cmt_"));
        assertTrue(Tsid.isValid(branded.substring(4)));
        assertThrows(IllegalArgumentException.class, () -> Tsid.generateWithPrefix(""));
        assertThrows(IllegalArgumentException.class, () -> Tsid.generateWithPrefix("a_b"));
    }

    @Test
    void rejectsInvalidFormats() {
        assertFalse(Tsid.isValid(null));
        assertFalse(Tsid.isValid("short"));
        assertFalse(Tsid.isValid("UUUUUUUUUUUUU")); // U not in Crockford alphabet
        assertTrue(Tsid.isValid("0123456789abc")); // case-insensitive
    }
}
