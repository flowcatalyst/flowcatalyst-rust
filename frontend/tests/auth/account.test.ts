// @vitest-environment jsdom
/**
 * Change password asks for a second-factor code the way Go's does: a first
 * submit without one is answered MFA_REQUIRED with the user's methods, which
 * the dialog reads to reveal its code step.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

function jsonResponse(status: number, body: unknown): Response {
	return new Response(JSON.stringify(body), {
		status,
		headers: { "Content-Type": "application/json" },
	});
}

const fetchMock = vi.fn();

beforeEach(() => {
	fetchMock.mockReset();
	vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
	vi.unstubAllGlobals();
});

describe("changePassword", () => {
	it("reports MFA_REQUIRED with the methods to ask for", async () => {
		fetchMock.mockResolvedValueOnce(
			jsonResponse(400, {
				code: "MFA_REQUIRED",
				message: "Enter a code from your second factor to change your password.",
				methods: ["TOTP", "EMAIL_PIN"],
			}),
		);
		const { changePassword } = await import("@/api/account");

		const result = await changePassword({
			currentPassword: "old",
			newPassword: "New#Pass123",
		});

		expect(fetchMock.mock.calls[0]![0]).toBe("/auth/change-password");
		expect(result).toEqual({
			ok: false,
			mfaRequired: true,
			methods: ["TOTP", "EMAIL_PIN"],
			errorCode: "MFA_REQUIRED",
			message: "Enter a code from your second factor to change your password.",
		});
	});

	it("reports a wrong current password without asking for a code", async () => {
		fetchMock.mockResolvedValueOnce(
			jsonResponse(401, {
				code: "INVALID_CURRENT_PASSWORD",
				message: "Your current password is incorrect.",
			}),
		);
		const { changePassword } = await import("@/api/account");

		const result = await changePassword({
			currentPassword: "wrong",
			newPassword: "New#Pass123",
		});

		expect(result.ok).toBe(false);
		expect(result.mfaRequired).toBe(false);
		expect(result.message).toBe("Your current password is incorrect.");
	});

	it("succeeds with the code", async () => {
		fetchMock.mockResolvedValueOnce(
			jsonResponse(200, { message: "Your password has been changed." }),
		);
		const { changePassword } = await import("@/api/account");

		const result = await changePassword({
			currentPassword: "old",
			newPassword: "New#Pass123",
			code: "123456",
		});

		expect(JSON.parse(fetchMock.mock.calls[0]![1].body)).toEqual({
			currentPassword: "old",
			newPassword: "New#Pass123",
			code: "123456",
		});
		expect(result).toEqual({
			ok: true,
			message: "Your password has been changed.",
		});
	});
});
