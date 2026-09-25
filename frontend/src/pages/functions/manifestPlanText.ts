// Renders a PromotePlanResponse as the `+`/`~`/`-`/`!` lines Java's
// `fcdev fn validate` prints (ValidateCommand.java#printText), so the SPA and
// the CLI read the same way. Ported verbatim from Java's SPA; kept in
// lockstep by hand, there is no shared source.
import type { PromotePlanResponse } from "@/api/functions";

export function renderPlanLines(plan: PromotePlanResponse): string[] {
	const lines: string[] = [];

	if (plan.httpOnly) {
		lines.push("! named alias — no wiring change");
	} else {
		if (plan.pool) lines.push(...poolLines(plan.pool));
		for (const s of plan.subscriptions ?? []) lines.push(...subscriptionLines(s));
		for (const s of plan.schedules ?? []) lines.push(...scheduleLines(s));
		if (plan.publicRoutes) lines.push(...publicRouteLines(plan.publicRoutes));
	}

	for (const c of plan.conflicts) {
		lines.push(`! conflict: ${c.code}: ${c.message}`);
	}

	if (plan.settingsMissing.length > 0) {
		lines.push(`! settings missing: ${plan.settingsMissing.join(", ")}`);
	}

	if (!plan.httpOnly && lines.length === 0) {
		lines.push("no changes");
	}

	return lines;
}

function poolLines(pool: { action: string; changedFields: string[] }): string[] {
	if (pool.action === "unchanged") return [];
	if (pool.action === "create") return ["+ pool (create)"];
	return [`~ pool (update: ${pool.changedFields.join(", ")})`];
}

function subscriptionLines(s: {
	action: string;
	eventType: string;
	changedFields: string[];
}): string[] {
	switch (s.action) {
		case "create":
			return [`+ subscription ${s.eventType} (create)`];
		case "update":
			return [`~ subscription ${s.eventType} (update: ${s.changedFields.join(", ")})`];
		case "delete":
			return [`- subscription ${s.eventType} (delete)`];
		default:
			return [];
	}
}

function scheduleLines(s: { action: string; cron: string; changedFields: string[] }): string[] {
	switch (s.action) {
		case "create":
			return [`+ schedule "${s.cron}" (create)`];
		case "update":
			return [`~ schedule "${s.cron}" (update: ${s.changedFields.join(", ")})`];
		case "delete":
			return [`- schedule "${s.cron}" (delete)`];
		default:
			return [];
	}
}

function publicRouteLines(routes: {
	action: string;
	added: { hostname: string; pathPrefix: string }[];
	removed: { hostname: string; pathPrefix: string }[];
}): string[] {
	if (routes.action !== "replace") return [];
	const lines: string[] = [];
	for (const r of routes.added) lines.push(`+ route ${r.hostname}${r.pathPrefix}`);
	for (const r of routes.removed) lines.push(`- route ${r.hostname}${r.pathPrefix}`);
	return lines;
}
