// @vitest-environment jsdom
/**
 * A password sign-in follows Go's contract: `/auth/login` either completes
 * (session set, navigation), or hands back a pending two-factor step — a
 * challenge (`mfa_required`) or a forced enrolment (`enrollment_required`) —
 * with no session. `/auth/2fa/verify` completes the pending sign-in.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";

const router = vi.hoisted(() => ({ replace: vi.fn() }));
vi.mock("@/router", () => ({ default: router }));

const okBody = {
	status: "ok",
	principalId: "prn_1",
	name: "Ada",
	email: "ada@example.com",
	roles: ["platform:admin"],
	permissions: ["platform:*:*:*"],
	clientId: null,
	ssoManaged: false,
};

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
	router.replace.mockReset();
	vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
	vi.unstubAllGlobals();
});

describe("login", () => {
	it("returns the challenge and sets no user on mfa_required", async () => {
		fetchMock.mockResolvedValueOnce(
			jsonResponse(200, {
				status: "mfa_required",
				mfaToken: "mfa-tok",
				methods: ["TOTP", "EMAIL_PIN"],
				rememberDeviceAllowed: true,
			}),
		);
		const { login } = await import("@/api/auth");
		const { useAuthStore } = await import("@/stores/auth");

		const result = await login({ email: "ada@example.com", password: "pw" });

		expect(result).toEqual({
			status: "mfa_required",
			mfaToken: "mfa-tok",
			methods: ["TOTP", "EMAIL_PIN"],
			rememberDeviceAllowed: true,
		});
		expect(useAuthStore().user).toBeNull();
		expect(router.replace).not.toHaveBeenCalled();
	});

	it("returns the enrolment step on enrollment_required", async () => {
		fetchMock.mockResolvedValueOnce(
			jsonResponse(200, {
				status: "enrollment_required",
				enrollToken: "enr-tok",
				allowedMethods: ["TOTP"],
			}),
		);
		const { login } = await import("@/api/auth");

		const result = await login({ email: "ada@example.com", password: "pw" });

		expect(result).toEqual({
			status: "enrollment_required",
			enrollToken: "enr-tok",
			allowedMethods: ["TOTP"],
		});
	});

	it("signs in and navigates on ok", async () => {
		fetchMock.mockResolvedValueOnce(jsonResponse(200, okBody));
		const { login } = await import("@/api/auth");
		const { useAuthStore } = await import("@/stores/auth");

		const result = await login({ email: "ada@example.com", password: "pw" });

		expect(result).toEqual({ status: "ok" });
		expect(useAuthStore().user?.id).toBe("prn_1");
		expect(router.replace).toHaveBeenCalledWith("/dashboard");
	});

	it("shows the server's message on a rejected password", async () => {
		fetchMock.mockResolvedValueOnce(
			jsonResponse(401, {
				code: "INVALID_CREDENTIALS",
				error: "INVALID_CREDENTIALS",
				message: "Invalid email or password",
			}),
		);
		const { login } = await import("@/api/auth");
		const { useAuthStore } = await import("@/stores/auth");

		await expect(
			login({ email: "ada@example.com", password: "bad" }),
		).rejects.toThrow("Invalid email or password");
		expect(useAuthStore().error).toBe("Invalid email or password");
	});
});

describe("verifyTwoFactor", () => {
	it("posts the code and completes the sign-in", async () => {
		fetchMock.mockResolvedValueOnce(jsonResponse(200, okBody));
		const { verifyTwoFactor } = await import("@/api/twofactor");
		const { useAuthStore } = await import("@/stores/auth");

		await verifyTwoFactor({
			mfaToken: "mfa-tok",
			method: "RECOVERY_CODE",
			code: "ABCDE-12345",
			rememberDevice: true,
		});

		const [url, init] = fetchMock.mock.calls[0]!;
		expect(url).toBe("/auth/2fa/verify");
		expect(JSON.parse(init.body)).toEqual({
			mfaToken: "mfa-tok",
			method: "RECOVERY_CODE",
			code: "ABCDE-12345",
			rememberDevice: true,
		});
		expect(useAuthStore().user?.email).toBe("ada@example.com");
		expect(router.replace).toHaveBeenCalledWith("/dashboard");
	});

	it("throws the server's message on a wrong code", async () => {
		fetchMock.mockResolvedValueOnce(
			jsonResponse(401, {
				code: "UNAUTHENTICATED",
				message: "Invalid or expired code",
			}),
		);
		const { verifyTwoFactor } = await import("@/api/twofactor");

		await expect(
			verifyTwoFactor({ mfaToken: "t", method: "TOTP", code: "000000" }),
		).rejects.toThrow("Invalid or expired code");
		expect(router.replace).not.toHaveBeenCalled();
	});
});

describe("enrolment confirm", () => {
	it("sets the session but defers navigation so the codes can be shown", async () => {
		fetchMock.mockResolvedValueOnce(
			jsonResponse(200, { ...okBody, recoveryCodes: ["AAAAA-11111"] }),
		);
		const { enrollTotpConfirm } = await import("@/api/twofactor");
		const { useAuthStore } = await import("@/stores/auth");

		const codes = await enrollTotpConfirm("enr-tok", "123456");

		expect(codes).toEqual(["AAAAA-11111"]);
		expect(useAuthStore().user?.id).toBe("prn_1");
		expect(router.replace).not.toHaveBeenCalled();
	});
});
