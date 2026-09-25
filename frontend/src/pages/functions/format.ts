// Pure display helpers shared by the function pages.

export function formatDate(value?: string | null): string {
	if (!value) return "—";
	return new Date(value).toLocaleString();
}

/** `sha256:0123abcd…89ef01`: the algorithm, the first 8 and last 6 hex. */
export function shortDigest(digest: string): string {
	const [algo, hex] = digest.split(":");
	if (!hex || hex.length <= 14) return digest;
	return `${algo}:${hex.slice(0, 8)}…${hex.slice(-6)}`;
}

/** How long ago a heartbeat was, relative to `now`. */
export function heartbeatAge(value?: string | null, now: number = Date.now()): string {
	if (!value) return "—";
	const ms = now - new Date(value).getTime();
	if (ms < 0) return "just now";
	const seconds = Math.floor(ms / 1000);
	if (seconds < 60) return `${seconds}s ago`;
	const minutes = Math.floor(seconds / 60);
	if (minutes < 60) return `${minutes}m ago`;
	return `${Math.floor(minutes / 60)}h ago`;
}

export function functionStatusSeverity(status: string): "success" | "secondary" {
	return status === "ACTIVE" ? "success" : "secondary";
}

export function versionStateSeverity(state: string): "success" | "info" | "secondary" {
	if (state === "READY") return "success";
	if (state === "PUBLISHED") return "info";
	return "secondary";
}

export function loadedStateSeverity(state: string): "success" | "info" | "danger" {
	if (state === "LOADED") return "success";
	if (state === "FAILED") return "danger";
	return "info";
}

/** A DNS label, as the platform's `DnsLabel`: 1-63 of a-z, 0-9 and `-`, not starting or ending with `-`. */
export const DNS_LABEL_PATTERN = /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$/;

/** The `{ message, location }` entries of an error envelope's `details.errors`. */
export function detailErrors(
	details: Record<string, unknown> | undefined,
): Array<{ location?: string; message: string }> {
	const errs = details?.["errors"];
	if (!Array.isArray(errs)) return [];
	return errs
		.filter(
			(e): e is { message: string; location?: string } =>
				typeof e === "object" && e !== null && typeof (e as { message?: unknown }).message === "string",
		)
		.map((e) => ({ location: e.location, message: e.message }));
}
