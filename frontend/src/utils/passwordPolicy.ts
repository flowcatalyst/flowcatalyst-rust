// Client-side mirror of the server password policy
// (internal/platform/auth/passwordpolicy) for instant feedback. The server
// remains authoritative — it additionally checks a 10k common-password list;
// here we only carry a small top-slice of it.

const MIN_LENGTH = 8;
const MAX_LENGTH = 128;

// Top slice of the server's embedded SecLists 10k list — enough to catch the
// classics without shipping the whole list to the browser.
const COMMON_PASSWORDS = new Set([
	"password",
	"password1",
	"passw0rd",
	"12345678",
	"123456789",
	"1234567890",
	"qwerty123",
	"qwertyuiop",
	"iloveyou",
	"sunshine",
	"princess",
	"baseball",
	"football",
	"superman",
	"trustno1",
	"welcome1",
	"letmein1",
	"whatever",
	"jennifer",
	"michelle",
	"computer",
	"11111111",
	"aa123456",
	"abc12345",
]);

function reverse(s: string): string {
	return [...s].reverse().join("");
}

export interface PasswordIdentity {
	email?: string | null;
	name?: string | null;
}

/**
 * Returns a user-facing error message when the password violates the policy,
 * or null when it is acceptable. Mirrors the server's rules: length bounds,
 * no single repeated character, not derived from the user's email/name
 * (forwards or reversed), not a well-known password or the product name.
 */
export function passwordPolicyError(
	password: string,
	identity: PasswordIdentity = {},
): string | null {
	if (password.length < MIN_LENGTH) {
		return `Password must be at least ${MIN_LENGTH} characters`;
	}
	if (password.length > MAX_LENGTH) {
		return `Password must be at most ${MAX_LENGTH} characters`;
	}
	const norm = password.trim().toLowerCase();
	if (new Set(norm).size === 1) {
		return "Password cannot be a single repeated character";
	}

	const reversed = reverse(norm);
	const candidates: string[] = [];
	const email = identity.email?.trim().toLowerCase() ?? "";
	if (email) {
		candidates.push(email);
		const at = email.indexOf("@");
		if (at > 0) candidates.push(email.slice(0, at));
	}
	for (const tok of (identity.name ?? "").toLowerCase().split(/\s+/)) {
		if (tok) candidates.push(tok);
	}
	for (const c of candidates) {
		const hit =
			c.length >= 4
				? norm.includes(c) || reversed.includes(c)
				: norm === c || reversed === c;
		if (hit) {
			return "Password cannot contain your email address or name";
		}
	}

	if (COMMON_PASSWORDS.has(norm)) {
		return "That password is on the list of most commonly used passwords — choose something less guessable";
	}
	if (norm.includes("flowcatalyst")) {
		return "Password cannot be based on the product name";
	}
	return null;
}
