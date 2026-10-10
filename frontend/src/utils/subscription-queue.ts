/**
 * A subscription's queue is its dispatch priority. The server accepts
 * DEFAULT or HIGH_PRIORITY only (anything else is 400 INVALID_QUEUE), so the
 * forms offer exactly these two instead of free text.
 */

export const QUEUE_OPTIONS = [
	{ label: "Default", value: "DEFAULT" },
	{ label: "High priority", value: "HIGH_PRIORITY" },
] as const;

export type SubscriptionQueue = (typeof QUEUE_OPTIONS)[number]["value"];

/** The form value for a stored queue: unset (or unknown) is DEFAULT. */
export function queueFormValue(
	stored: string | null | undefined,
): SubscriptionQueue {
	const upper = (stored ?? "").trim().toUpperCase();
	return upper === "HIGH_PRIORITY" ? "HIGH_PRIORITY" : "DEFAULT";
}
