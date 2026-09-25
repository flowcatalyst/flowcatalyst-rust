// @vitest-environment jsdom
/**
 * The password flows' wire shapes follow Go: a factor-gated reset sends the
 * authenticator code as `factorCode`, confirm answers the next step
 * (`enrollment_required`, `redirectUri`, `sessionEstablished`), and the
 * create-your-password request carries the OAuth round-trip to resume.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";

vi.mock("@/router", () => ({ default: { replace: vi.fn() } }));

function jsonResponse(status: number, body: unknown): Response {
	return new Response(JSON.stringify(body), {
		status,
		headers: { "Content-Type": "application/json" },
	});
}

const fetchMock = vi.fn();

beforeEach(() => {
	setActivePinia(createPinia());
	fetchMock.mockReset();
	vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
	vi.unstubAllGlobals();
});

function sentBody(call = 0): unknown {
	return JSON.parse(fetchMock.mock.calls[call]![1].body);
}

describe("confirmPasswordReset", () => {
	it("sends the factor code only when one is given", async () => {
		fetchMock.mockImplementation(async () =>
			jsonResponse(200, { status: "ok", message: "Password reset successfully." }),
		);
		const { confirmPasswordReset } = await import("@/api/auth");

		await confirmPasswordReset("tok", "Secret#123");
		await confirmPasswordReset("tok", "Secret#123", "654321");

		expect(fetchMock.mock.calls[0]![0]).toBe("/auth/password-reset/confirm");
		expect(sentBody(0)).toEqual({ token: "tok", password: "Secret#123" });
		expect(sentBody(1)).toEqual({
			token: "tok",
			password: "Secret#123",
			factorCode: "654321",
		});
	});

	it("returns the enrolment step the domain requires", async () => {
		fetchMock.mockResolvedValueOnce(
			jsonResponse(200, {
				status: "enrollment_required",
				message: "Password set. Set up two-factor authentication to finish.",
				enrollToken: "enr",
				allowedMethods: ["TOTP", "EMAIL_PIN"],
			}),
		);
		const { confirmPasswordReset } = await import("@/api/auth");

		const result = await confirmPasswordReset("tok", "Secret#123");

		expect(result.status).toBe("enrollment_required");
		expect(result.enrollToken).toBe("enr");
		expect(result.allowedMethods).toEqual(["TOTP", "EMAIL_PIN"]);
	});

	it("throws the server's message on a wrong authenticator code", async () => {
		fetchMock.mockResolvedValueOnce(
			jsonResponse(400, {
				code: "INVALID_FACTOR",
				error: "INVALID_FACTOR",
				message: "Invalid authenticator code.",
			}),
		);
		const { confirmPasswordReset } = await import("@/api/auth");

		await expect(
			confirmPasswordReset("tok", "Secret#123", "000000"),
		).rejects.toThrow("Invalid authenticator code.");
	});
});

describe("validateResetToken", () => {
	it("reports a factor-gated token", async () => {
		fetchMock.mockResolvedValueOnce(
			jsonResponse(200, { valid: true, reason: null, requiresFactor: true }),
		);
		const { validateResetToken } = await import("@/api/auth");

		const result = await validateResetToken("a b");

		expect(fetchMock.mock.calls[0]![0]).toBe(
			"/auth/password-reset/validate?token=a%20b",
		);
		expect(result.requiresFactor).toBe(true);
	});
});

describe("requestPasswordSetup", () => {
	it("posts the email and the OAuth round-trip to resume", async () => {
		fetchMock.mockResolvedValueOnce(jsonResponse(200, { message: "sent" }));
		const { requestPasswordSetup } = await import("@/api/auth");

		await requestPasswordSetup("ada@example.com", "/oauth/authorize?client_id=x");

		expect(fetchMock.mock.calls[0]![0]).toBe("/auth/password-setup/request");
		expect(sentBody()).toEqual({
			email: "ada@example.com",
			redirectUri: "/oauth/authorize?client_id=x",
		});
	});
});
