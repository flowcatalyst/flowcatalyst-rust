/**
 * Route guards installed on the Fastify instance as `app.fc.requireSession`,
 * `app.fc.requireBearer`, `app.fc.requireAuth`.
 *
 * Each returns a `preHandler` you attach per-route. The guards differ only
 * in what they accept and how they respond to missing/invalid credentials:
 *
 *   requireSession  — cookie only; redirects iff the request looks like a
 *                     browser navigation (Accept: text/html), else 401 JSON
 *   requireBearer   — Bearer token only, 401 JSON on miss
 *   requireAuth     — either; same content negotiation as requireSession
 */

import type { FastifyReply, FastifyRequest, preHandlerHookHandler } from "fastify";

export type GuardKind = "session" | "bearer" | "any";

export interface GuardContext {
	loginPath: string;
	returnToQueryParam: string;
}

export function makeGuard(
	kind: GuardKind,
	ctx: GuardContext,
): preHandlerHookHandler {
	return async function (req, reply) {
		if (req.principal) return; // already authenticated
		await deny(kind, req, reply, ctx);
	};
}

async function deny(
	kind: GuardKind,
	req: FastifyRequest,
	reply: FastifyReply,
	ctx: GuardContext,
): Promise<void> {
	// Redirect only real browser navigations; XHR/fetch callers (SPAs probing
	// a session endpoint) need the 401 — a silent 302 chain ends cross-origin
	// at the platform's authorize URL, which fetch cannot follow.
	const useRedirect = isHtmlNavigation(req) && kind !== "bearer";

	if (useRedirect) {
		const returnTo = encodeURIComponent(req.url);
		await reply
			.code(302)
			.redirect(`${ctx.loginPath}?${ctx.returnToQueryParam}=${returnTo}`);
		return;
	}
	await reply
		.code(401)
		.header("WWW-Authenticate", `Bearer realm="flowcatalyst"`)
		.send({ error: "unauthorized" });
}

function isHtmlNavigation(req: FastifyRequest): boolean {
	if (req.method !== "GET" && req.method !== "HEAD") return false;
	const accept = req.headers["accept"];
	return typeof accept === "string" && accept.includes("text/html");
}
