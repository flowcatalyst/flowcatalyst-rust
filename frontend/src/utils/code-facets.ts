/**
 * Facets derived from "application:subdomain:aggregate:event" codes.
 *
 * The dispatch-job filter-options endpoint returns the distinct codes, not
 * their segments; the application, subdomain and aggregate filters take
 * their options from those codes, each level narrowed by the values chosen
 * above it (as the server-rendered /ui page does: internal/ui/pages
 * firehose.go codeLevels).
 */

export interface CodeLevels {
	applications: string[];
	subdomains: string[];
	aggregates: string[];
	codes: string[];
}

export function codeLevels(
	codes: string[],
	applications: string[] = [],
	subdomains: string[] = [],
	aggregates: string[] = [],
): CodeLevels {
	const chosen = [applications, subdomains, aggregates];
	const under = (seg: string[], level: number) => {
		for (let i = 0; i < level; i++) {
			const pick = chosen[i] ?? [];
			if (pick.length > 0 && (seg.length <= i || !pick.includes(seg[i] ?? "")))
				return false;
		}
		return true;
	};
	const levels: Set<string>[] = [new Set(), new Set(), new Set(), new Set()];
	for (const code of codes) {
		const seg = code.split(":");
		for (let level = 0; level < 3; level++) {
			const value = seg[level];
			if (value !== undefined && under(seg, level)) levels[level]?.add(value);
		}
		if (under(seg, 3)) levels[3]?.add(code);
	}
	const sorted = (s: Set<string> | undefined) => [...(s ?? [])].sort();
	return {
		applications: sorted(levels[0]),
		subdomains: sorted(levels[1]),
		aggregates: sorted(levels[2]),
		codes: sorted(levels[3]),
	};
}
