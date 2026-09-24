/**
 * Audit logs must never store passwords or secrets. `operationData` reaches
 * the outbox as the whole command; this walks it and masks anything that
 * looks like a credential before it is serialised into the audit row, so a
 * leak (a command that carries a plaintext token or secret field the way
 * `serviceaccount` Create/Update once did) never reaches `aud_logs`.
 *
 * A key is secret when, lower-cased with `_` and `-` removed, it **ends
 * with** `password`, `passwordhash`, `secret`, `secretref`, `passphrase` or
 * `token`, or **equals** `apikey`, `privatekey`, `authorization` or
 * `cookie`. A secret key's value becomes the string `"***"` — whatever its
 * type — except `null` and booleans, which are kept as-is. `maskedFields`
 * additionally masks top-level field names the caller declares even though
 * the name rule would keep them (e.g. a config value whose secrecy depends
 * on a sibling field). Objects and arrays are walked; everything else is
 * untouched. Pure — never mutates `data`.
 *
 * `data` is first taken through a JSON round trip, so the rule sees exactly
 * the document that will be serialised: a nested `Date` stays its ISO
 * string (not an empty object), `toJSON` is honoured, and methods and
 * `undefined` fields are dropped as `JSON.stringify` drops them.
 */
export function redactAuditData(
	data: Record<string, unknown>,
	maskedFields: readonly string[] = [],
): Record<string, unknown> {
	const document: unknown = JSON.parse(JSON.stringify(data));
	if (document === null || typeof document !== "object" || Array.isArray(document)) {
		return data;
	}
	return redactObject(document as Record<string, unknown>, new Set(maskedFields));
}

const SECRET_SUFFIXES = [
	"password",
	"passwordhash",
	"secret",
	"secretref",
	"passphrase",
	"token",
];

const SECRET_EXACT = new Set(["apikey", "privatekey", "authorization", "cookie"]);

function normalizeKey(key: string): string {
	return key.toLowerCase().replace(/[_-]/g, "");
}

function isSecretKey(key: string): boolean {
	const normalized = normalizeKey(key);
	return (
		SECRET_EXACT.has(normalized) ||
		SECRET_SUFFIXES.some((suffix) => normalized.endsWith(suffix))
	);
}

function maskValue(value: unknown): unknown {
	if (value === null || typeof value === "boolean") {
		return value;
	}
	return "***";
}

function redactValue(value: unknown): unknown {
	if (Array.isArray(value)) {
		return value.map((item) => redactValue(item));
	}
	if (value !== null && typeof value === "object") {
		return redactObject(value as Record<string, unknown>, EMPTY_MASKED);
	}
	return value;
}

const EMPTY_MASKED: ReadonlySet<string> = new Set();

function redactObject(
	obj: Record<string, unknown>,
	topLevelMasked: ReadonlySet<string>,
): Record<string, unknown> {
	const result: Record<string, unknown> = {};
	for (const [key, value] of Object.entries(obj)) {
		if (topLevelMasked.has(key) || isSecretKey(key)) {
			result[key] = maskValue(value);
		} else {
			result[key] = redactValue(value);
		}
	}
	return result;
}

/**
 * A command that declares top-level fields its audit row must mask even
 * though the name rule would keep them (e.g. a config `value` that is only
 * secret when a sibling `valueType` says so). The outbox unit of work reads
 * the declaration when the command it audits implements this; methods are
 * not serialised, so it never reaches the audit row itself.
 */
export interface AuditMasked {
	auditMaskedFields(): readonly string[];
}

/** The command's declared masked fields (see `AuditMasked`), or none. */
export function auditMaskedFieldsOf(command: unknown): readonly string[] {
	if (command !== null && typeof command === "object") {
		const declare = (command as Partial<AuditMasked>).auditMaskedFields;
		if (typeof declare === "function") {
			const fields = declare.call(command);
			if (Array.isArray(fields)) {
				return fields.filter((f): f is string => typeof f === "string");
			}
		}
	}
	return [];
}
