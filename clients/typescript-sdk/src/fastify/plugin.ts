/**
 * Fastify plugin that wires FlowCatalyst OIDC + Bearer authentication into
 * an app. See `./index.ts` for the README-grade usage example.
 *
 * Responsibilities:
 *  - Verify Bearer tokens (Authorization header) against FC's JWKS.
 *  - Run the OIDC authorization-code/PKCE flow for browser callers, storing
 *    an encrypted session via the configured {@link SessionStore}.
 *  - Build a {@link Principal} (identical shape regardless of mechanism)
 *    with role/permission helpers backed by the local {@link RbacCatalogue}.
 *  - Expose `app.fc.requireSession() / requireBearer() / requireAuth()`
 *    and a `request.principal` typed extension.
 *
 * Things the plugin deliberately does NOT do:
 *  - Custom per-app post-auth logic. Add a Fastify `preHandler` AFTER
 *    `app.register(flowcatalystAuth)` if you want to enrich the principal
 *    or perform custom checks. Example in the README.
 */

import type {
	FastifyInstance,
	FastifyPluginAsync,
	FastifyReply,
	FastifyRequest,
	preHandlerHookHandler,
} from "fastify";
import fp from "fastify-plugin";
import fastifyCookie from "@fastify/cookie";
import {
	buildAuthorizeUrl,
	exchangeCode,
	generateAuthCodeBag,
	refreshAccessToken,
} from "./oidc/flow.js";
import { createOidcClient, type OidcEndpoints } from "./oidc/discovery.js";
import { claimsToSnapshot, type FcAccessTokenClaims } from "./oidc/claims.js";
import { buildPrincipal, type Principal } from "./principal.js";
import type { RbacCatalogue } from "./rbac.js";
import {
	CookieSessionStore,
	type CookieAttrs,
} from "./session/cookie-store.js";
import type { SessionPayload, SessionStore } from "./session/types.js";
import { makeGuard, type GuardContext } from "./guards.js";

/**
 * Plugin options. The only required fields are `baseUrl`, `clientId`,
 * `clientSecret`, and `cookie.secret` — everything else has sensible
 * defaults documented inline below.
 */
export interface FlowcatalystAuthOptions {
	/** FlowCatalyst platform base URL, e.g. `https://platform.example.com`. */
	baseUrl: string;
	/** OAuth client id (confidential web client registered at FC). */
	clientId: string;
	/** OAuth client secret. */
	clientSecret: string;
	/** OIDC scopes requested at authorization. Defaults to `openid profile email`. */
	scope?: string;
	/** Expected `aud` claim. Defaults to `flowcatalyst` (FC's default audience). */
	expectedAudience?: string;
	/**
	 * Portal mode: this app is a PORTAL for a client's customers, so logins
	 * enter through FlowCatalyst's portal identity plane
	 * (`{baseUrl}/portal/authorize`) — a separate end-user population from
	 * platform users, no platform SSO reuse, and no refresh tokens. Requires
	 * the OAuth client's portal owner client to be set. The callback and
	 * token exchange are unchanged; the id_token `sub` is the portal
	 * identity id (`ptu_…`).
	 */
	portal?: boolean;
	/**
	 * Branded sign-in: the FlowCatalyst client identifier (URL-safe slug, e.g.
	 * `"acme"`) whose login branding the sign-in pages should wear — logo,
	 * colours, brand name and footer, as configured under Clients → Login
	 * Branding. A per-request `?client=` on the login route overrides this.
	 *
	 * Purely cosmetic: it does not affect who may sign in or what they may
	 * access, and an absent or unrecognised value falls back to the
	 * platform-wide theme. Ignored in {@link portal} mode, which has its own
	 * login surface.
	 */
	client?: string;
	/** Local RBAC catalogue (role → permissions). Omit to skip permission checks. */
	rbac?: RbacCatalogue;

	/** Cookie config — used by the default {@link CookieSessionStore}. */
	cookie: {
		name?: string;
		secret: string | readonly string[];
		path?: string;
		domain?: string;
		httpOnly?: boolean;
		secure?: boolean;
		sameSite?: "lax" | "strict" | "none";
		/** Max-age in seconds. Defaults to 8h. */
		maxAge?: number;
	};
	/**
	 * Override the session backend. Defaults to {@link CookieSessionStore}
	 * using the `cookie` config above. Pass {@link PgSessionStore} or
	 * {@link RedisSessionStore} for server-side storage.
	 */
	sessionStore?: SessionStore;

	/** Route paths (override only if they conflict with your routes). */
	routes?: {
		login?: string;
		callback?: string;
		logout?: string;
	};
	/** Query param name used to round-trip the post-login destination. */
	returnToQueryParam?: string;
	/** Public base URL for callbacks. Defaults to deriving from the incoming request. */
	publicBaseUrl?: string;
}

declare module "fastify" {
	interface FastifyRequest {
		principal?: Principal;
	}
	interface FastifyInstance {
		fc: FlowcatalystAuthDecorator;
	}
}

export interface FlowcatalystAuthDecorator {
	requireSession(): preHandlerHookHandler;
	requireBearer(): preHandlerHookHandler;
	requireAuth(): preHandlerHookHandler;
	/** Programmatic logout: clears the session and (optionally) 302s to a URL. */
	logout(
		req: FastifyRequest,
		reply: FastifyReply,
		opts?: { redirectTo?: string },
	): Promise<void>;
}

const DEFAULT_LOGIN = "/auth/login";
const DEFAULT_CALLBACK = "/auth/callback";
const DEFAULT_LOGOUT = "/auth/logout";
const DEFAULT_COOKIE_NAME = "fc_session";
const STATE_COOKIE_NAME = "fc_oauth_state";
const DEFAULT_RETURN_TO_PARAM = "returnTo";
const DEFAULT_SCOPE = "openid profile email";
const DEFAULT_MAX_AGE = 60 * 60 * 8; // 8h
const REFRESH_LEEWAY_MS = 60_000;

const flowcatalystAuthImpl: FastifyPluginAsync<FlowcatalystAuthOptions> =
	async (fastify, opts) => {
		await ensureCookiePlugin(fastify);

		const cookieAttrs = resolveCookieAttrs(opts.cookie);
		const sessionStore: SessionStore =
			opts.sessionStore ??
			new CookieSessionStore({
				cookieName: opts.cookie.name ?? DEFAULT_COOKIE_NAME,
				secret: opts.cookie.secret,
				cookieOptions: cookieAttrs,
			});
		// Wire any store-owned periodic maintenance (e.g. PgSessionStore's
		// expired-row reap) to this plugin instance's lifecycle.
		sessionStore.startReaper?.();
		fastify.addHook("onClose", async () => {
			sessionStore.close?.();
		});

		const routes = {
			login: opts.routes?.login ?? DEFAULT_LOGIN,
			callback: opts.routes?.callback ?? DEFAULT_CALLBACK,
			logout: opts.routes?.logout ?? DEFAULT_LOGOUT,
		};
		const returnToParam = opts.returnToQueryParam ?? DEFAULT_RETURN_TO_PARAM;
		const scope = opts.scope ?? DEFAULT_SCOPE;
		const oidc = createOidcClient({
			baseUrl: opts.baseUrl,
			...(opts.expectedAudience !== undefined
				? { expectedAudience: opts.expectedAudience }
				: {}),
			...(opts.portal
				? {
						authorizationEndpoint: `${opts.baseUrl.replace(/\/$/, "")}/portal/authorize`,
					}
				: {}),
		});

		// State cookie attrs — same security posture, scoped to /auth + short-lived.
		const stateCookieAttrs: CookieAttrs = {
			...cookieAttrs,
			path: routes.callback,
			maxAge: 600,
		};
		const stateCrypto = new CookieSessionStore({
			cookieName: STATE_COOKIE_NAME,
			secret: opts.cookie.secret,
			cookieOptions: stateCookieAttrs,
		});

		// ─── Request decoration & auth resolution ────────────────────────
		fastify.decorateRequest("principal", undefined);

		fastify.addHook("onRequest", async (req, reply) => {
			// First plane to authenticate the request wins. One app may register
			// flowcatalystAuth twice (a root-context management plane plus an
			// encapsulated portal plane); root-context hooks run on EVERY route,
			// so without this guard the later-running instance clobbers a
			// principal the route's own plane already resolved — e.g. an admin's
			// management session overwriting the ptu_ portal principal on portal
			// routes, which the app's membership gate then rejects as
			// no_portal_access even though the portal login just succeeded.
			if (req.principal) return;

			// Bearer wins if present — APIs explicitly identifying themselves should
			// never be silently downgraded to whatever session cookie the browser sent.
			const bearer = readBearer(req);
			if (bearer) {
				const principal = await verifyBearer({
					token: bearer,
					oidc,
					rbac: opts.rbac,
				});
				if (principal) {
					req.principal = principal;
				}
				return;
			}

			const session = await sessionStore.read<Record<string, unknown>>(req);
			if (!session) return;

			// Refresh access token if we're inside the leeway window.
			let working: SessionPayload<Record<string, unknown>> = session;
			if (
				session.tokens.refreshToken &&
				session.tokens.accessTokenExpiresAt - Date.now() < REFRESH_LEEWAY_MS
			) {
				const refreshed = await tryRefresh({
					session,
					oidc,
					clientId: opts.clientId,
					clientSecret: opts.clientSecret,
				});
				if (refreshed) {
					working = refreshed;
					await sessionStore.write(reply, refreshed);
				} else {
					await sessionStore.clear(req, reply);
					return;
				}
			}

			req.principal = buildPrincipal({
				snapshot: {
					...working.principal,
					sessionData: working.sessionData,
					mechanism: "session",
				},
				rbac: opts.rbac,
			});
		});

		// ─── Routes ─────────────────────────────────────────────────────
		fastify.get(routes.login, async (req, reply) => {
			const query = req.query as Record<string, string | undefined>;
			const returnTo = sanitizeReturnTo(query[returnToParam]);
			const bag = generateAuthCodeBag(returnTo);
			const endpoints = await oidc.endpoints();
			// Per-request ?client= wins over the configured default. Omitted in
			// portal mode: login branding covers the platform sign-in pages
			// only, never the portal identity plane.
			const client = opts.portal ? undefined : (query["client"] ?? opts.client);
			const url = await buildAuthorizeUrl({
				endpoints,
				clientId: opts.clientId,
				redirectUri: resolveCallbackUrl(req, opts.publicBaseUrl, routes.callback),
				scope,
				bag,
				...(client ? { client } : {}),
			});
			await stateCrypto.write(reply, bagToSession(bag));
			return reply.redirect(url);
		});

		fastify.get(routes.callback, async (req, reply) => {
			const query = req.query as Record<string, string | undefined>;
			const code = query["code"];
			const state = query["state"];
			const stateSession = await stateCrypto.read<{ bag: ReturnType<typeof generateAuthCodeBag> }>(
				req,
			);
			await stateCrypto.clear(req, reply);
			if (!code || !state || !stateSession || stateSession.principal.id !== "_oauth_state") {
				return reply.code(400).send({ error: "invalid_oauth_state" });
			}
			const bag = (stateSession.sessionData as { bag: ReturnType<typeof generateAuthCodeBag> }).bag;
			if (bag.state !== state) {
				return reply.code(400).send({ error: "invalid_oauth_state" });
			}

			const endpoints = await oidc.endpoints();
			const result = await exchangeCode({
				endpoints,
				clientId: opts.clientId,
				clientSecret: opts.clientSecret,
				redirectUri: resolveCallbackUrl(req, opts.publicBaseUrl, routes.callback),
				code,
				codeVerifier: bag.codeVerifier,
			});

			const sessionMaxAgeMs = (opts.cookie.maxAge ?? DEFAULT_MAX_AGE) * 1000;
			const session: SessionPayload = {
				principal: claimsToSnapshot(result.claims, "session"),
				tokens: {
					accessToken: result.accessToken,
					accessTokenExpiresAt: result.accessTokenExpiresAt,
					...(result.refreshToken ? { refreshToken: result.refreshToken } : {}),
				},
				sessionData: {},
				expiresAt: Date.now() + sessionMaxAgeMs,
			};
			await sessionStore.write(reply, session);
			return reply.redirect(bag.returnTo || "/");
		});

		const logout: FlowcatalystAuthDecorator["logout"] = async (
			req,
			reply,
			logoutOpts,
		) => {
			await sessionStore.clear(req, reply);
			if (logoutOpts?.redirectTo) {
				await reply.redirect(logoutOpts.redirectTo);
			}
		};

		fastify.post(routes.logout, async (req, reply) => {
			const body = (req.body as { redirectTo?: string } | undefined) ?? {};
			await logout(req, reply, body.redirectTo ? { redirectTo: body.redirectTo } : {});
			if (!reply.sent) await reply.code(204).send();
		});

		// ─── Decorator ──────────────────────────────────────────────────
		const guardCtx: GuardContext = {
			loginPath: routes.login,
			returnToQueryParam: returnToParam,
		};
		const decorator: FlowcatalystAuthDecorator = {
			requireSession: () => makeGuard("session", guardCtx),
			requireBearer: () => makeGuard("bearer", guardCtx),
			requireAuth: () => makeGuard("any", guardCtx),
			logout,
		};
		fastify.decorate("fc", decorator);
	};

/**
 * Wrapped with `fastify-plugin` so the `fc` decorator, `request.principal`,
 * and the registered `/auth/*` routes escape the encapsulation boundary
 * and are visible to the parent scope.
 */
export const flowcatalystAuth = fp(flowcatalystAuthImpl, {
	fastify: "5.x",
	name: "@flowcatalyst/sdk/fastify",
});

// ───────────────────────── helpers ─────────────────────────

async function ensureCookiePlugin(fastify: FastifyInstance): Promise<void> {
	// `@fastify/cookie` is idempotent-friendly: registering twice throws.
	// Apps that already have it registered should be detected via the decorator.
	if (
		fastify.hasReplyDecorator("setCookie") &&
		fastify.hasRequestDecorator("cookies")
	) {
		return;
	}
	await fastify.register(fastifyCookie);
}

function readBearer(req: FastifyRequest): string | null {
	const raw = req.headers["authorization"];
	if (typeof raw !== "string") return null;
	const m = /^Bearer\s+(.+)$/i.exec(raw.trim());
	return m ? m[1]! : null;
}

async function verifyBearer(opts: {
	token: string;
	oidc: { endpoints(): Promise<OidcEndpoints> };
	rbac: RbacCatalogue | undefined;
}): Promise<Principal | null> {
	try {
		const endpoints = await opts.oidc.endpoints();
		const claims = (await endpoints.verify(opts.token)) as FcAccessTokenClaims;
		return buildPrincipal({
			snapshot: {
				...claimsToSnapshot(claims, "bearer"),
				sessionData: {},
			},
			rbac: opts.rbac,
		});
	} catch {
		return null;
	}
}

async function tryRefresh(opts: {
	session: SessionPayload;
	oidc: { endpoints(): Promise<OidcEndpoints> };
	clientId: string;
	clientSecret: string;
}): Promise<SessionPayload | null> {
	if (!opts.session.tokens.refreshToken) return null;
	try {
		const endpoints = await opts.oidc.endpoints();
		const result = await refreshAccessToken({
			endpoints,
			clientId: opts.clientId,
			clientSecret: opts.clientSecret,
			refreshToken: opts.session.tokens.refreshToken,
		});
		return {
			principal: claimsToSnapshot(result.claims, "session"),
			tokens: {
				accessToken: result.accessToken,
				accessTokenExpiresAt: result.accessTokenExpiresAt,
				...(result.refreshToken
					? { refreshToken: result.refreshToken }
					: opts.session.tokens.refreshToken
						? { refreshToken: opts.session.tokens.refreshToken }
						: {}),
			},
			sessionData: opts.session.sessionData,
			expiresAt: opts.session.expiresAt,
		};
	} catch {
		return null;
	}
}

function resolveCookieAttrs(c: FlowcatalystAuthOptions["cookie"]): CookieAttrs {
	return {
		path: c.path ?? "/",
		...(c.domain ? { domain: c.domain } : {}),
		httpOnly: c.httpOnly ?? true,
		secure: c.secure ?? true,
		sameSite: c.sameSite ?? "lax",
		maxAge: c.maxAge ?? DEFAULT_MAX_AGE,
	};
}

function resolveCallbackUrl(
	req: FastifyRequest,
	publicBaseUrl: string | undefined,
	callbackPath: string,
): string {
	if (publicBaseUrl) {
		return `${publicBaseUrl.replace(/\/$/, "")}${callbackPath}`;
	}
	const proto =
		(req.headers["x-forwarded-proto"] as string | undefined) ?? req.protocol;
	const host =
		(req.headers["x-forwarded-host"] as string | undefined) ??
		req.headers["host"];
	return `${proto}://${host}${callbackPath}`;
}

function sanitizeReturnTo(raw: string | undefined): string {
	if (!raw) return "/";
	try {
		const decoded = decodeURIComponent(raw);
		// Only allow same-origin paths (start with `/`, not `//` which is protocol-relative).
		if (decoded.startsWith("/") && !decoded.startsWith("//")) {
			return decoded;
		}
	} catch {
		// fall through
	}
	return "/";
}

/**
 * State stash uses CookieSessionStore by abusing the `principal.id` field
 * as a discriminator. Cheap and avoids a second crypto helper.
 */
function bagToSession(
	bag: ReturnType<typeof generateAuthCodeBag>,
): SessionPayload<{ bag: ReturnType<typeof generateAuthCodeBag> }> {
	return {
		principal: {
			id: "_oauth_state",
			type: "USER",
			scope: "client",
			name: "",
			clients: [],
			clientIds: [],
			clientCodes: [],
			roles: [],
			applications: [],
			applicationCodes: [],
			allApplications: false,
		},
		tokens: { accessToken: "", accessTokenExpiresAt: 0 },
		sessionData: { bag },
		expiresAt: Date.now() + 600_000,
	};
}
