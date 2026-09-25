/**
 * Live example: a Fastify app authenticated by FlowCatalyst.
 *
 * Run against a local fcdev:
 *
 *   1. Start fcdev (e.g. `cargo run -p fcdev` from the workspace root).
 *   2. Register a confidential web client at FlowCatalyst with redirect URI
 *      `http://localhost:4000/auth/callback`. Copy the client id + secret.
 *   3. Generate a session secret:
 *        node -e "import('@flowcatalyst/sdk/fastify').then(m => console.log(m.generateSessionSecret()))"
 *   4. Export the env vars below and run `pnpm start`.
 *
 *      FC_BASE_URL=http://localhost:8080
 *      FC_CLIENT_ID=clt_xxx
 *      FC_CLIENT_SECRET=xxx
 *      SESSION_SECRET=<32B base64url>
 *
 *   5. Browse to http://localhost:4000/dashboard — you'll be redirected to
 *      FlowCatalyst to log in, then bounced back with a session cookie.
 *
 *   For Bearer-token testing, mint a client_credentials token from FC and
 *   curl http://localhost:4000/api/me with `Authorization: Bearer <token>`.
 */

import Fastify from "fastify";
import {
	flowcatalystAuth,
	defineRbac,
} from "@flowcatalyst/sdk/fastify";

const required = (name: string): string => {
	const v = process.env[name];
	if (!v) {
		throw new Error(`missing required env var ${name}`);
	}
	return v;
};

const FC_BASE_URL = required("FC_BASE_URL");
const FC_CLIENT_ID = required("FC_CLIENT_ID");
const FC_CLIENT_SECRET = required("FC_CLIENT_SECRET");
const SESSION_SECRET = required("SESSION_SECRET");
const PORT = Number(process.env.PORT ?? 4000);

const rbac = defineRbac()
	.role("operant:admin").grants("operant:*")
	.role("operant:viewer").grants("operant:read")
	.role("billing-admin").grants("invoice:create", "invoice:read", "invoice:void")
	.role("billing-viewer").grants("invoice:read")
	.role("support").grants("ticket:*")
	.build();

const app = Fastify({ logger: { level: "info" } });

await app.register(flowcatalystAuth, {
	baseUrl: FC_BASE_URL,
	clientId: FC_CLIENT_ID,
	clientSecret: FC_CLIENT_SECRET,
	cookie: {
		secret: SESSION_SECRET,
		secure: process.env.NODE_ENV === "production",
		sameSite: "lax",
	},
	rbac,
});

// Example: app's own post-auth enrichment hook. Runs AFTER the plugin has
// populated `request.principal`. Use this to upsert a local DB record, log
// who's hitting the app, or enforce app-specific access rules.
app.addHook("preHandler", async (req) => {
	if (!req.principal) return;
	req.log.info(
		{ pid: req.principal.id, mechanism: req.principal.mechanism, roles: req.principal.roles },
		"authenticated request",
	);
});

// ─── Public routes ─────────────────────────────────────────────
app.get("/", async () => ({
	hello: "world",
	hint: "Try /dashboard (browser) or /api/me with a Bearer token.",
}));

// ─── Browser routes (redirect on miss) ─────────────────────────
app.get("/dashboard", { preHandler: app.fc.requireSession() }, async (req) => ({
	greeting: `Hi ${req.principal!.name}`,
	scope: req.principal!.scope,
	clients: req.principal!.clients,
	roles: req.principal!.roles,
	applications: req.principal!.applications,
	canReadInvoices: req.principal!.hasPermissionTo(["invoice:read"]),
	canVoidInvoices: req.principal!.hasPermissionTo(["invoice:void"]),
}));

// ─── API routes (Bearer-only, 401 on miss) ─────────────────────
app.get("/api/me", { preHandler: app.fc.requireBearer() }, async (req) => ({
	id: req.principal!.id,
	type: req.principal!.type,
	scope: req.principal!.scope,
	clients: req.principal!.clients,
	roles: req.principal!.roles,
}));

app.post("/api/orders", { preHandler: app.fc.requireBearer() }, async (req, reply) => {
	if (!req.principal!.hasPermissionTo(["invoice:create"])) {
		return reply.code(403).send({ error: "forbidden", required: "invoice:create" });
	}
	return { ok: true };
});

// ─── Either (redirect browsers, 401 machines) ──────────────────
app.get("/whoami", { preHandler: app.fc.requireAuth() }, async (req) => ({
	id: req.principal!.id,
	via: req.principal!.mechanism,
}));

// ─── Logout: clear cookie + send the user to FC's platform-logout page ─
app.post("/auth/logout-and-redirect", async (req, reply) => {
	await app.fc.logout(req, reply, {
		redirectTo: `${FC_BASE_URL}/logout`,
	});
});

await app.listen({ port: PORT, host: "0.0.0.0" });
app.log.info(`fastify-app listening on http://localhost:${PORT}`);
