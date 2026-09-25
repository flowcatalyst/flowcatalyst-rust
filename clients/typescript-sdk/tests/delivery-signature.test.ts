/**
 * Pins the delivery-signature contract shared with the platform: the Go
 * scheduled-job dispatcher and dispatch-process delivery sign HMAC-SHA256
 * over `timestamp + rawBody`, hex, with a millisecond-ISO8601 UTC timestamp
 * (the Go side pins the same bytes in its dispatcher tests; the Laravel SDK
 * in WebhookValidatorCompatTest).
 */
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { createHmac } from "node:crypto";

import {
	verifyDeliverySignature,
	WebhookSignatureError,
} from "../src/webhook/signature.js";

const SECRET = "sjdsp-signing-secret-1";

/** Sign exactly as the Go platform does. */
function goSign(timestamp: string, body: string, secret = SECRET): string {
	const mac = createHmac("sha256", secret);
	mac.update(timestamp);
	mac.update(body);
	return mac.digest("hex");
}

/** Millisecond-ISO8601 UTC, the Go `2006-01-02T15:04:05.000Z` format. */
function goTimestamp(msOffset = 0): string {
	return new Date(Date.now() + msOffset).toISOString();
}

const BODY =
	'{"jobId":"sjb_1","jobCode":"nightly-report","instanceId":"sji_1",' +
	'"firedAt":"2026-08-07T00:00:00Z","triggerKind":"CRON",' +
	'"tracksCompletion":false,"concurrent":false}';

function expectCode(fn: () => void, code: string) {
	assert.throws(fn, (e: unknown) => {
		assert.ok(e instanceof WebhookSignatureError, `expected WebhookSignatureError, got ${e}`);
		assert.equal(e.code, code);
		return true;
	});
}

describe("verifyDeliverySignature", () => {
	it("accepts the Go platform's signature format", () => {
		const ts = goTimestamp();
		verifyDeliverySignature({
			rawBody: BODY,
			signature: goSign(ts, BODY),
			timestamp: ts,
			secret: SECRET,
		});
	});

	it("accepts the raw body as bytes and multi-value headers", () => {
		const ts = goTimestamp();
		verifyDeliverySignature({
			rawBody: new TextEncoder().encode(BODY),
			signature: [goSign(ts, BODY)],
			timestamp: [ts],
			secret: SECRET,
		});
	});

	it("accepts a bare unix-seconds timestamp (legacy)", () => {
		const ts = String(Math.floor(Date.now() / 1000));
		verifyDeliverySignature({
			rawBody: BODY,
			signature: goSign(ts, BODY),
			timestamp: ts,
			secret: SECRET,
		});
	});

	it("rejects a tampered body", () => {
		const ts = goTimestamp();
		expectCode(
			() =>
				verifyDeliverySignature({
					rawBody: '{"tampered":true}',
					signature: goSign(ts, BODY),
					timestamp: ts,
					secret: SECRET,
				}),
			"invalid_signature",
		);
	});

	it("rejects a replay older than the tolerance", () => {
		const ts = goTimestamp(-10 * 60 * 1000);
		expectCode(
			() =>
				verifyDeliverySignature({
					rawBody: BODY,
					signature: goSign(ts, BODY),
					timestamp: ts,
					secret: SECRET,
				}),
			"timestamp_expired",
		);
	});

	it("rejects a far-future timestamp", () => {
		const ts = goTimestamp(10 * 60 * 1000);
		expectCode(
			() =>
				verifyDeliverySignature({
					rawBody: BODY,
					signature: goSign(ts, BODY),
					timestamp: ts,
					secret: SECRET,
				}),
			"timestamp_in_future",
		);
	});

	it("rejects the wrong secret", () => {
		const ts = goTimestamp();
		expectCode(
			() =>
				verifyDeliverySignature({
					rawBody: BODY,
					signature: goSign(ts, BODY, "a-different-secret"),
					timestamp: ts,
					secret: SECRET,
				}),
			"invalid_signature",
		);
	});

	it("fails closed on missing inputs", () => {
		const ts = goTimestamp();
		const sig = goSign(ts, BODY);
		expectCode(
			() => verifyDeliverySignature({ rawBody: BODY, signature: sig, timestamp: ts, secret: "" }),
			"missing_secret",
		);
		expectCode(
			() => verifyDeliverySignature({ rawBody: BODY, signature: undefined, timestamp: ts, secret: SECRET }),
			"missing_signature",
		);
		expectCode(
			() => verifyDeliverySignature({ rawBody: BODY, signature: sig, timestamp: null, secret: SECRET }),
			"missing_timestamp",
		);
		expectCode(
			() => verifyDeliverySignature({ rawBody: BODY, signature: sig, timestamp: "yesterday-ish", secret: SECRET }),
			"invalid_timestamp",
		);
	});
});

describe("verifyDeliverySignature bearer gate", () => {
	it("is AND-ed with the signature when configured", () => {
		const ts = goTimestamp();
		const sig = goSign(ts, BODY);
		// Both valid → passes.
		verifyDeliverySignature({
			rawBody: BODY,
			signature: sig,
			timestamp: ts,
			secret: SECRET,
			expectedBearerToken: "fc_tok",
			authorization: "Bearer fc_tok",
		});
		// Valid signature, wrong bearer → rejected (bearer is not optional once configured).
		expectCode(
			() =>
				verifyDeliverySignature({
					rawBody: BODY,
					signature: sig,
					timestamp: ts,
					secret: SECRET,
					expectedBearerToken: "fc_tok",
					authorization: "Bearer wrong",
				}),
			"invalid_bearer",
		);
		expectCode(
			() =>
				verifyDeliverySignature({
					rawBody: BODY,
					signature: sig,
					timestamp: ts,
					secret: SECRET,
					expectedBearerToken: "fc_tok",
					authorization: undefined,
				}),
			"missing_bearer",
		);
		// Valid bearer, broken signature → still rejected (bearer never substitutes).
		expectCode(
			() =>
				verifyDeliverySignature({
					rawBody: '{"tampered":true}',
					signature: sig,
					timestamp: ts,
					secret: SECRET,
					expectedBearerToken: "fc_tok",
					authorization: "Bearer fc_tok",
				}),
			"invalid_signature",
		);
	});

	it("stays signature-only when not configured", () => {
		const ts = goTimestamp();
		verifyDeliverySignature({
			rawBody: BODY,
			signature: goSign(ts, BODY),
			timestamp: ts,
			secret: SECRET,
			authorization: "Bearer anything",
		});
	});
});
