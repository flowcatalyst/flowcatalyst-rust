import type { FilterOption } from "@/api/filter-options";

/**
 * The client choices matching a typed query (name or id, ignoring case),
 * at most `limit`. The choices are /bff/filter-options/clients: the active
 * clients the caller can access (every one for an anchor), so a
 * client-scoped user can bind a pool, connection or subscription to its own
 * client — the anchor-only client search left it no choice.
 */
export function matchClientOptions(
	options: FilterOption[],
	query: string,
	limit = 20,
): FilterOption[] {
	const q = query.trim().toLowerCase();
	const hits = q
		? options.filter(
				(o) =>
					o.label.toLowerCase().includes(q) ||
					o.value.toLowerCase().includes(q),
			)
		: options;
	return hits.slice(0, limit);
}
