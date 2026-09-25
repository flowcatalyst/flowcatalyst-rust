/**
 * Mock OIDC issuer for end-to-end Fastify plugin tests.
 *
 * Spins up a Node http server that speaks just enough of the OIDC dance
 * the plugin actually exercises:
 *
 *   GET /.well-known/openid-configuration  → discovery doc
 *   GET /.well-known/jwks.json             → JWKS for the issuer key
 *   GET /oauth/authorize                   → 302 to redirect_uri with `code`
 *   POST /oauth/token                      → signed access/refresh/id tokens
 *
 * Claim envelopes match FlowCatalyst's `AccessTokenClaims` and
 * `IDTokenClaims` (see `internal/platform/auth/authservice`) so the plugin's
 * claim extractor is exercised against the real shape — not a synthetic
 * stand-in. By default no `id_token` is returned, which mirrors a
 * `client_credentials`-shaped exchange; `signIdToken` + `setNextTokens`
 * opt a test into the interactive shape.
 */

import { createServer, type Server } from "node:http";
import { AddressInfo } from "node:net";
import {
	generateKeyPair,
	SignJWT,
	exportJWK,
	type KeyLike,
	type JWK,
} from "jose";

export interface IssuedTokens {
	accessToken: string;
	refreshToken: string;
	expiresIn: number;
	idToken?: string;
}

export interface MockIssuer {
	baseUrl: string;
	issuer: string;
	stop(): Promise<void>;
	/** Tokens that the next `/oauth/token` call will return. */
	setNextTokens(tokens: IssuedTokens): void;
	/** Convenience: build a signed access token matching FC's claim shape. */
	signAccessToken(claims: Partial<TokenClaims> & { sub: string; name: string }): Promise<string>;
	/**
	 * Build a signed ID token matching FC's `IDTokenClaims`. `aud` defaults to
	 * `clientId` — the relying party — not the platform API audience.
	 */
	signIdToken(claims: Partial<IdTokenClaims> & { sub: string }): Promise<string>;
	/** Last code accepted at /oauth/token (for assertions). */
	lastSeenCode(): string | null;
	/** Last refresh_token grant body (for assertions). */
	lastRefreshGrant(): URLSearchParams | null;
	/** Next /authorize call will issue this code instead of a random one. */
	setNextCode(code: string): void;
}

interface TokenClaims {
	sub: string;
	iss: string;
	aud: string | string[];
	exp: number;
	iat: number;
	nbf: number;
	jti: string;
	type: "USER" | "SERVICE";
	scope: "ANCHOR" | "PARTNER" | "CLIENT";
	name: string;
	email?: string;
	clients: string[];
	roles: string[];
	applications: string[];
}

interface IdTokenClaims {
	sub: string;
	iss: string;
	aud: string;
	exp: number;
	iat: number;
	type: "USER" | "SERVICE";
	tier: "ANCHOR" | "PARTNER" | "CLIENT";
	name?: string;
	email?: string;
	clients: string[];
	roles: string[];
	applications: string[];
}

export async function startMockIssuer(): Promise<MockIssuer> {
	const { publicKey, privateKey } = await generateKeyPair("RS256", {
		modulusLength: 2048,
		extractable: true,
	});
	const kid = "test-key-1";
	const jwk: JWK = { ...(await exportJWK(publicKey)), kid, alg: "RS256", use: "sig" };

	let issuedTokens: IssuedTokens | null = null;
	let lastCode: string | null = null;
	let lastRefreshGrant: URLSearchParams | null = null;
	let nextCode: string | null = null;

	let baseUrl = "";
	let issuer = "";

	const sign = async (claims: Partial<TokenClaims> & { sub: string; name: string }) => {
		const now = Math.floor(Date.now() / 1000);
		const full: TokenClaims = {
			iss: issuer,
			aud: "flowcatalyst",
			exp: now + 600,
			iat: now,
			nbf: now,
			jti: crypto.randomUUID(),
			type: "USER",
			scope: "CLIENT",
			clients: ["clt_test"],
			roles: ["billing-admin"],
			applications: ["billing"],
			...claims,
		};
		return await new SignJWT(full as unknown as Record<string, unknown>)
			.setProtectedHeader({ alg: "RS256", kid })
			.sign(privateKey as KeyLike);
	};

	const signId = async (claims: Partial<IdTokenClaims> & { sub: string }) => {
		const now = Math.floor(Date.now() / 1000);
		const full: IdTokenClaims = {
			iss: issuer,
			aud: "test-client",
			exp: now + 600,
			iat: now,
			type: "USER",
			tier: "CLIENT",
			clients: [],
			roles: [],
			applications: [],
			...claims,
		};
		return await new SignJWT(full as unknown as Record<string, unknown>)
			.setProtectedHeader({ alg: "RS256", kid })
			.sign(privateKey as KeyLike);
	};

	const server: Server = createServer(async (req, res) => {
		try {
			const url = new URL(req.url ?? "/", baseUrl || "http://localhost");
			if (req.method === "GET" && url.pathname === "/.well-known/openid-configuration") {
				return json(res, {
					issuer,
					authorization_endpoint: `${baseUrl}/oauth/authorize`,
					token_endpoint: `${baseUrl}/oauth/token`,
					jwks_uri: `${baseUrl}/.well-known/jwks.json`,
					end_session_endpoint: `${baseUrl}/oauth/logout`,
					response_types_supported: ["code"],
					grant_types_supported: ["authorization_code", "refresh_token", "client_credentials"],
					id_token_signing_alg_values_supported: ["RS256"],
				});
			}
			if (req.method === "GET" && url.pathname === "/.well-known/jwks.json") {
				return json(res, { keys: [jwk] });
			}
			if (req.method === "GET" && url.pathname === "/oauth/authorize") {
				const redirectUri = url.searchParams.get("redirect_uri");
				const state = url.searchParams.get("state");
				if (!redirectUri || !state) {
					res.writeHead(400);
					return res.end("missing redirect_uri or state");
				}
				const code = nextCode ?? `code_${crypto.randomUUID()}`;
				nextCode = null;
				const redirect = new URL(redirectUri);
				redirect.searchParams.set("code", code);
				redirect.searchParams.set("state", state);
				res.writeHead(302, { Location: redirect.toString() });
				return res.end();
			}
			if (req.method === "POST" && url.pathname === "/oauth/token") {
				const body = await readBody(req);
				const params = new URLSearchParams(body);
				const grant = params.get("grant_type");
				if (grant === "authorization_code") {
					lastCode = params.get("code");
				} else if (grant === "refresh_token") {
					lastRefreshGrant = params;
				}
				if (!issuedTokens) {
					issuedTokens = {
						accessToken: await sign({ sub: "prn_default", name: "Default" }),
						refreshToken: "rt_default",
						expiresIn: 600,
					};
				}
				return json(res, {
					access_token: issuedTokens.accessToken,
					refresh_token: issuedTokens.refreshToken,
					token_type: "Bearer",
					expires_in: issuedTokens.expiresIn,
					...(issuedTokens.idToken ? { id_token: issuedTokens.idToken } : {}),
				});
			}
			res.writeHead(404);
			res.end();
		} catch (err) {
			res.writeHead(500);
			res.end(String(err));
		}
	});

	await new Promise<void>((resolve) =>
		server.listen(0, "127.0.0.1", () => resolve()),
	);
	const addr = server.address() as AddressInfo;
	baseUrl = `http://127.0.0.1:${addr.port}`;
	issuer = baseUrl;

	return {
		baseUrl,
		issuer,
		async stop() {
			await new Promise<void>((resolve, reject) =>
				server.close((e) => (e ? reject(e) : resolve())),
			);
		},
		setNextTokens(t) {
			issuedTokens = t;
		},
		signAccessToken: sign,
		signIdToken: signId,
		lastSeenCode: () => lastCode,
		lastRefreshGrant: () => lastRefreshGrant,
		setNextCode(c) {
			nextCode = c;
		},
	};
}

function json(res: import("node:http").ServerResponse, body: unknown): void {
	const buf = Buffer.from(JSON.stringify(body));
	res.writeHead(200, {
		"Content-Type": "application/json",
		"Content-Length": buf.byteLength.toString(),
	});
	res.end(buf);
}

async function readBody(req: import("node:http").IncomingMessage): Promise<string> {
	const chunks: Buffer[] = [];
	for await (const c of req) {
		chunks.push(c as Buffer);
	}
	return Buffer.concat(chunks).toString("utf-8");
}
