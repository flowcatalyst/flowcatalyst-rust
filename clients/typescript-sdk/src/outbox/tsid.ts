/**
 * Lightweight TSID (Time-Sorted ID) generator.
 *
 * Generates 13-character Crockford Base32 strings from a 64-bit value:
 * 42 bits of timestamp, then 22 low bits split as Go's `pkg/fcsdk/tsid`
 * does — a 12-bit sequence above 10 random bits. The sequence starts at a
 * random value each millisecond and increments within it, so ids from one
 * process are strictly increasing: outbox rows written in one transaction
 * (one `created_at`) keep their creation order when sorted by id, which the
 * outbox processor relies on within a message group.
 */

const CROCKFORD_ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const TSID_LENGTH = 13;
const LOW_BITS = 22;
const SEQUENCE_MAX = 0xfff;
const RANDOM_MASK = 0x3ff;

// Custom epoch: 2020-01-01T00:00:00Z
const CUSTOM_EPOCH = 1577836800000;

let lastMs = -1;
let lastSeq = 0;

function randomBelow(n: number): number {
	return Math.floor(Math.random() * n);
}

/** The next (millisecond, sequence) pair (Go `nextMsSeq`). */
function nextMsSeq(): [number, number] {
	const now = Date.now() - CUSTOM_EPOCH;
	if (now > lastMs) {
		lastMs = now;
		lastSeq = randomBelow(SEQUENCE_MAX + 1);
	} else if (lastSeq < SEQUENCE_MAX) {
		lastSeq += 1;
	} else {
		lastMs += 1;
		lastSeq = randomBelow(SEQUENCE_MAX + 1);
	}
	return [lastMs, lastSeq];
}

/**
 * Generate a new TSID as a 13-character Crockford Base32 string.
 */
export function generate(): string {
	const [ms, seq] = nextMsSeq();
	const random = randomBelow(RANDOM_MASK + 1);
	// Use BigInt for 64-bit arithmetic
	const value =
		(BigInt(ms) << BigInt(LOW_BITS)) | (BigInt(seq) << 10n) | BigInt(random);
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
