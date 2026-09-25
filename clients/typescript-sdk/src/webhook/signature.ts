/**
 * Verification for FlowCatalyst-signed deliveries: scheduled-job firings and
 * dispatch webhooks both carry
 *
 *   X-FlowCatalyst-Signature: hex(HMAC-SHA256(timestamp + rawBody, secret))
 *   X-FlowCatalyst-Timestamp: 2026-08-07T09:32:12.123Z   (ms-precision ISO8601)
 *
 * signed with your application service account's signing secret. Byte-format
 * matches the platform's router/dispatcher signers and the Laravel SDK's
 * WebhookValidator: 5-minute replay tolerance, 60-second future grace,
 * constant-time compare.
 *
 * The verifier is framework-agnostic — hand it the raw body and the two
 * header values from whatever HTTP framework hosts your runner. The body MUST
 * be the raw bytes as received (sign-then-parse; a re-serialised JSON body
 * will not verify).
 *
 *   // e.g. Fastify with a raw-body content-type parser
 *   verifyDeliverySignature({
 *     rawBody,
 *     signature: req.headers["x-flowcatalyst-signature"],
 *     timestamp: req.headers["x-flowcatalyst-timestamp"],
 *     secret: process.env.FLOWCATALYST_SIGNING_SECRET!,
 *   }); // throws WebhookSignatureError on any failure
 *   const result = await runner.process(JSON.parse(rawBody));
 */

import { createHmac, timingSafeEqual } from "node:crypto";

export type WebhookSignatureErrorCode =
	| "missing_secret"
	| "missing_signature"
	| "missing_timestamp"
	| "invalid_timestamp"
	| "timestamp_expired"
	| "timestamp_in_future"
	| "invalid_signature"
	| "missing_bearer"
	| "invalid_bearer";

/** Verification failure; `code` says exactly what to fix. */
export class WebhookSignatureError extends Error {
	readonly code: WebhookSignatureErrorCode;

	constructor(code: WebhookSignatureErrorCode, message: string) {
		super(message);
		this.name = "WebhookSignatureError";
		this.code = code;
	}
}

export interface VerifyDeliverySignatureParams {
	/** Raw request body, exactly as received. */
	rawBody: string | Uint8Array;
	/** Value of the X-FlowCatalyst-Signature header. */
	signature: string | string[] | null | undefined;
	/** Value of the X-FlowCatalyst-Timestamp header. */
	timestamp: string | string[] | null | undefined;
	/** Your application service account's signing secret. */
	secret: string;
	/** Max age in seconds before a delivery is considered a replay. Default 300. */
	toleranceSeconds?: number;
	/**
	 * Optional second gate, AND-ed with the signature (never a substitute for
	 * it): when set, the delivery's Authorization header must be exactly
	 * `Bearer <expectedBearerToken>` — your service account's webhook auth
	 * token, sent by the platform alongside the signature.
	 */
	expectedBearerToken?: string;
	/** Value of the Authorization header (required when expectedBearerToken is set). */
	authorization?: string | string[] | null | undefined;
}

/**
 * Verify a signed delivery; throws {@link WebhookSignatureError} on any
 * failure (fail-closed — an empty secret is an error, not a bypass).
 */
export function verifyDeliverySignature(params: VerifyDeliverySignatureParams): void {
	const { rawBody, secret } = params;
	const tolerance = params.toleranceSeconds ?? 300;

	if (!secret) {
		throw new WebhookSignatureError(
			"missing_secret",
			"Webhook signing secret is not configured.",
		);
	}
	const signature = single(params.signature);
	if (!signature) {
		throw new WebhookSignatureError(
			"missing_signature",
			"Missing X-FlowCatalyst-Signature header.",
		);
	}
	const timestamp = single(params.timestamp);
	if (!timestamp) {
		throw new WebhookSignatureError(
			"missing_timestamp",
			"Missing X-FlowCatalyst-Timestamp header.",
		);
	}

	const webhookSeconds = parseTimestamp(timestamp);
	if (webhookSeconds === null) {
		throw new WebhookSignatureError(
			"invalid_timestamp",
			`Unparseable X-FlowCatalyst-Timestamp '${timestamp}'.`,
		);
	}
	const nowSeconds = Math.floor(Date.now() / 1000);
	if (webhookSeconds < nowSeconds - tolerance) {
		throw new WebhookSignatureError(
			"timestamp_expired",
			`Delivery timestamp older than ${tolerance}s — replay rejected.`,
		);
	}
	if (webhookSeconds > nowSeconds + 60) {
		throw new WebhookSignatureError(
			"timestamp_in_future",
			"Delivery timestamp is in the future — check clock sync.",
		);
	}

	// HMAC over the raw timestamp header string + raw body bytes.
	const mac = createHmac("sha256", secret);
	mac.update(timestamp);
	mac.update(typeof rawBody === "string" ? Buffer.from(rawBody) : rawBody);
	const expected = mac.digest("hex");

	const a = Buffer.from(expected, "utf8");
	const b = Buffer.from(signature.toLowerCase(), "utf8");
	if (a.length !== b.length || !timingSafeEqual(a, b)) {
		throw new WebhookSignatureError("invalid_signature", "Signature mismatch.");
	}

	// Layered bearer gate (opt-in). Runs AFTER the signature so the strong
	// check is always exercised; both must pass when configured.
	if (params.expectedBearerToken) {
		const auth = single(params.authorization);
		const prefix = "Bearer ";
		if (!auth || !auth.startsWith(prefix)) {
			throw new WebhookSignatureError(
				"missing_bearer",
				"Missing Authorization bearer token.",
			);
		}
		const presented = Buffer.from(auth.slice(prefix.length), "utf8");
		const want = Buffer.from(params.expectedBearerToken, "utf8");
		if (presented.length !== want.length || !timingSafeEqual(presented, want)) {
			throw new WebhookSignatureError("invalid_bearer", "Invalid bearer token.");
		}
	}
}

/** First value of a possibly-multi header. */
function single(v: string | string[] | null | undefined): string | undefined {
	if (Array.isArray(v)) return v[0];
	return v ?? undefined;
}

/**
 * The platform emits millisecond-ISO8601 UTC; a bare Unix-seconds integer is
 * accepted for backward compatibility. The HMAC always covers the raw header
 * string — parsing only feeds the replay-window check.
 */
function parseTimestamp(timestamp: string): number | null {
	if (/^\d+$/.test(timestamp)) return Number(timestamp);
	const ms = Date.parse(timestamp);
	if (Number.isNaN(ms)) return null;
	return Math.floor(ms / 1000);
}
