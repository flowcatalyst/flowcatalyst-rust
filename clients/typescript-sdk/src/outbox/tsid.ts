/**
 * Lightweight TSID (Time-Sorted ID) generator.
 *
 * Generates 13-character Crockford Base32 strings from a 64-bit value
 * composed of 42 bits of timestamp + 22 bits of randomness.
 */

const CROCKFORD_ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const TSID_LENGTH = 13;
const RANDOM_BITS = 22;
const RANDOM_MASK = (1 << RANDOM_BITS) - 1;

// Custom epoch: 2020-01-01T00:00:00Z
const CUSTOM_EPOCH = 1577836800000;

/**
 * Generate a new TSID as a 13-character Crockford Base32 string.
 */
export function generate(): string {
	const timestamp = Date.now() - CUSTOM_EPOCH;
	const random = Math.floor(Math.random() * (RANDOM_MASK + 1));

	// Combine: timestamp in upper 42 bits, random in lower 22 bits
	// Use BigInt for 64-bit arithmetic
	const value = (BigInt(timestamp) << BigInt(RANDOM_BITS)) | BigInt(random);

	return encodeCrockford(value);
}

/**
 * Generate a BRANDED (typed) TSID: `${prefix}_${raw}` — matching the
 * FlowCatalyst platform convention (e.g. `aud_…`, `prn_…`). Use a short
 * lowercase prefix for your own entities, e.g. `generateWithPrefix("cmt")`
 * → `cmt_6F7JC2A6JFR7N`.
 *
 * @throws if the prefix is empty or contains an underscore.
 */
export function generateWithPrefix(prefix: string): string {
	if (prefix.length === 0 || prefix.includes("_")) {
		throw new Error("TSID prefix must be non-empty and contain no underscore.");
	}
	return `${prefix}_${generate()}`;
}

/**
 * Validate that a string is a valid TSID format.
 */
export function isValid(tsid: string): boolean {
	if (tsid.length !== TSID_LENGTH) return false;
	const upper = tsid.toUpperCase();
	for (let i = 0; i < upper.length; i++) {
		if (!CROCKFORD_ALPHABET.includes(upper[i]!)) return false;
	}
	return true;
}

function encodeCrockford(value: bigint): string {
	const chars: string[] = Array.from({ length: TSID_LENGTH });
	let remaining = value;

	for (let i = TSID_LENGTH - 1; i >= 0; i--) {
		chars[i] = CROCKFORD_ALPHABET[Number(remaining & 31n)]!;
		remaining >>= 5n;
	}

	return chars.join("");
}
