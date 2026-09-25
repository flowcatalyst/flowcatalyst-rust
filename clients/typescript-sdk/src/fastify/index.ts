/**
 * `@flowcatalyst/sdk/fastify` — drop-in OIDC + Bearer auth for Fastify apps.
 *
 * Quick start:
 *
 *   import Fastify from "fastify";
 *   import {
 *     flowcatalystAuth,
 *     defineRbac,
 *     generateSessionSecret,
 *   } from "@flowcatalyst/sdk/fastify";
 *
 *   const app = Fastify();
 *
 *   const rbac = defineRbac()
 *     .role("operant:admin").grants("operant:*")
 *     .role("operant:viewer").grants("operant:read")
 *     .build();
 *
 *   await app.register(flowcatalystAuth, {
 *     baseUrl: "https://platform.example.com",
 *     clientId: process.env.FC_CLIENT_ID!,
 *     clientSecret: process.env.FC_CLIENT_SECRET!,
 *     cookie: { secret: process.env.SESSION_SECRET! },  // 32B base64url; use generateSessionSecret()
 *     rbac,
 *   });
 *
 *   // App's post-auth hook (runs AFTER plugin populates request.principal):
 *   app.addHook("preHandler", async (req) => {
 *     if (!req.principal) return;
 *     const user = await db.user.upsert({ where: { fcId: req.principal.id }, ... });
 *     req.principal.sessionData.localUserId = user.id;
 *   });
 *
 *   // Web route — 302s to /auth/login if no session cookie.
 *   app.get("/dashboard", { preHandler: app.fc.requireSession() }, handler);
 *
 *   // API route — 401 JSON if no valid Bearer token.
 *   app.post("/api/orders", { preHandler: app.fc.requireBearer() }, handler);
 *
 *   // Either — redirects browsers, 401s machines.
 *   app.get("/api/me", { preHandler: app.fc.requireAuth() }, async (req) => {
 *     if (!req.principal!.hasPermissionTo(["operant:read"])) {
 *       throw app.httpErrors.forbidden();
 *     }
 *     return { id: req.principal!.id, roles: req.principal!.roles };
 *   });
 *
 *   // Logout (local-only; redirect to FC's platform-logout page if you
 *   // want to also terminate the OIDC session at the platform):
 *   app.post("/auth/logout", async (req, reply) => {
 *     await app.fc.logout(req, reply, {
 *       redirectTo: "https://platform.example.com/logout",
 *     });
 *   });
 */

export { flowcatalystAuth } from "./plugin.js";
export type {
	FlowcatalystAuthOptions,
	FlowcatalystAuthDecorator,
} from "./plugin.js";
export {
	defineRbac,
	type RbacBuilder,
	type RbacCatalogue,
} from "./rbac.js";
export type {
	Principal,
	PortalContext,
	PrincipalSnapshot,
	PrincipalType,
	PrincipalScope,
	AuthMechanism,
} from "./principal.js";
export {
	generateSessionSecret,
	createSessionCrypto,
	type SessionCrypto,
} from "./crypto.js";
export type {
	SessionStore,
	SessionPayload,
	SessionTokens,
} from "./session/types.js";
export {
	CookieSessionStore,
	type CookieAttrs,
	type CookieSessionStoreOptions,
} from "./session/cookie-store.js";
export {
	PgSessionStore,
	type PgSessionStoreOptions,
	CREATE_SESSION_TABLE_SQL,
	initSessionSchema,
} from "./session/pg-session-store.js";
export {
	RedisSessionStore,
	type RedisSessionStoreOptions,
} from "./session/redis-session-store.js";
