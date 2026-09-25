/**
 * Every code the platform facets on — event `type`s and dispatch-job `code`s —
 * must be a fully qualified `application:subdomain:aggregate:action` string.
 * The platform projects the first three segments out as application/subdomain/
 * aggregate facets; a bare one-word code both renders as `name:::` in the UI
 * and facets under the WRONG application (segment 1), and it denies the
 * delivery pipeline the application linkage it uses to resolve signing
 * credentials. The SDK therefore refuses to emit unqualified codes.
 */
export function assertQualifiedCode(value: string, field: string): void {
	const segments = value.split(":");
	if (segments.length !== 4 || segments.some((s) => s.trim() === "")) {
		throw new Error(
			`${field} '${value}' must be a fully qualified code with four non-empty ` +
				`colon-separated segments: application:subdomain:aggregate:action ` +
				`(e.g. 'fulfil-go:fulfilment:part:create-pick')`,
		);
	}
}
