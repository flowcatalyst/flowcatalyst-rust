// Stays hand-rolled: auth surface — chi-mounted outside huma, not in the
// OpenAPI spec. Revisit if these routes converge on huma.
// Self-service password change. Endpoints live under /auth/* (NOT /api), like
// the other session-gated auth calls. The change-password endpoint may answer
// MFA_REQUIRED when the user has 2FA enrolled, so this client surfaces the
// response code rather than collapsing everything into a thrown Error.

const AUTH = "/auth";

export interface ChangePasswordResult {
	ok: boolean;
	mfaRequired?: boolean;
	/** Confirmed factor types, e.g. ["TOTP", "EMAIL_PIN"]. */
	methods?: string[];
	errorCode?: string;
	message?: string;
}

export async function changePassword(input: {
	currentPassword: string;
	newPassword: string;
	code?: string;
}): Promise<ChangePasswordResult> {
	const res = await fetch(`${AUTH}/change-password`, {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify(input),
		credentials: "include",
	});
	const data: {
		code?: string;
		methods?: string[];
		message?: string;
	} = await res.json().catch(() => ({}));
	if (res.ok) {
		return { ok: true, message: data.message };
	}
	return {
		ok: false,
		mfaRequired: data.code === "MFA_REQUIRED",
		methods: data.methods,
		errorCode: data.code,
		message: data.message || "Could not change your password.",
	};
}

export async function sendChangePasswordEmailCode(): Promise<{ message: string }> {
	const res = await fetch(`${AUTH}/change-password/send-email-code`, {
		method: "POST",
		credentials: "include",
	});
	const data: { message?: string } = await res.json().catch(() => ({}));
	if (!res.ok) {
		throw new Error(data.message || "Could not send a code.");
	}
	return { message: data.message ?? "A code has been sent to your email." };
}
