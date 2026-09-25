/**
 * PgSessionStore behaviour tests (T2).
 *
 * There is no real-Postgres test harness in this repo (no other pg-backed
 * store — PgCacheStore, PgLockProvider, the outbox driver — has one either),
 * and standing one up (testcontainers, docker-compose) is not cheap for two
 * behaviours. `PgQueryable` is deliberately a one-method duck-typed
 * interface (`query(text, params)`), so instead this fakes just that
 * interface with an in-memory table and asserts against its actual rows —
 * the observable effect ("the row is gone from the live table"), not that a
 * particular method was called. The fake pattern-matches on the exact SQL
 * `PgSessionStore` issues; that coupling is inherent to testing SQL behaviour
 * without a real database.
 */

import { strict as assert } from "node:assert";
import { after, describe, it } from "node:test";
import Fastify, { type FastifyInstance, type FastifyReply, type FastifyRequest } from "fastify";
import {
	PgSessionStore,
	type PgSessionStoreOptions,
} from "../../src/fastify/session/pg-session-store.js";
import type { SessionPayload } from "../../src/fastify/session/types.js";
import type { CookieAttrs } from "../../src/fastify/session/cookie-store.js";
import { flowcatalystAuth } from "../../src/fastify/plugin.js";
import { generateSessionSecret } from "../../src/fastify/crypto.js";

const cookieOptions: CookieAttrs = {
	path: "/",
	httpOnly: true,
	secure: true,
	sameSite: "lax",
	maxAge: 3600,
};

interface FakeRow {
	payload: string;
	expiresAt: Date;
}

/** In-memory stand-in for a `pg.Pool` that understands only the four query
 * shapes `PgSessionStore` issues. Exposes `rows` so tests can assert on the
 * table's actual contents. */
class FakePgExecutor {
	readonly rows = new Map<string, FakeRow>();

	async query(
		text: string,
		params: ReadonlyArray<unknown> = [],
	): Promise<{ rows?: unknown[]; rowCount?: number | null }> {
		const now = Date.now();

		if (text.includes("SELECT payload")) {
			const sid = params[0] as string;
			const row = this.rows.get(sid);
			if (!row || row.expiresAt.getTime() <= now) return { rows: [] };
			return { rows: [{ payload: row.payload }] };
		}

		if (text.includes("INSERT INTO")) {
			const [sid, payload, previousSid, expiresAt] = params as [
				string,
				string,
				string | null,
				Date,
			];
			if (previousSid && previousSid !== sid) {
				this.rows.delete(previousSid);
			}
			this.rows.set(sid, { payload, expiresAt });
			return { rowCount: 1 };
		}

		if (text.includes("WHERE sid = $1")) {
			const sid = params[0] as string;
			const existed = this.rows.delete(sid);
			return { rowCount: existed ? 1 : 0 };
		}

		if (text.includes("expires_at <= NOW()")) {
			let removed = 0;
			for (const [sid, row] of this.rows) {
				if (row.expiresAt.getTime() <= now) {
					this.rows.delete(sid);
					removed++;
				}
			}
			return { rowCount: removed };
		}

		throw new Error(`FakePgExecutor: unrecognised query: ${text}`);
	}
}

function session(over: Partial<SessionPayload> = {}): SessionPayload {
	return {
		principal: {
			id: "prn_x",
			type: "USER",
			scope: "client",
			name: "Tester",
			clients: ["clt_a:acme"],
			clientIds: ["clt_a"],
			clientCodes: ["acme"],
			roles: ["r"],
			applications: ["app"],
			applicationCodes: ["code"],
			allApplications: false,
		},
		tokens: {
			accessToken: "at",
			accessTokenExpiresAt: Date.now() + 5 * 60_000,
		},
		sessionData: {},
		expiresAt: Date.now() + 60 * 60_000,
		...over,
	};
}

/** A FastifyReply stand-in whose `.request.cookies` can be pre-seeded (as a
 * real request carrying an existing session cookie would be) and whose
 * `setCookie` capture lets the test read the minted sid back out. */
function fakeReply(existingCookies: Record<string, string> = {}): FastifyReply & {
	captured?: { name: string; value: string };
} {
	const req = { cookies: existingCookies } as unknown as FastifyRequest;
	const r: Record<string, unknown> = {
		request: req,
		captured: undefined,
		setCookie(name: string, value: string) {
			r.captured = { name, value };
			return r;
		},
		clearCookie() {
			return r;
		},
	};
	return r as FastifyReply & { captured?: { name: string; value: string } };
}

function newStore(opts: Partial<PgSessionStoreOptions> & { executor: FakePgExecutor }) {
	return new PgSessionStore({
		cookieName: "fc_session",
		cookieOptions,
		...opts,
	});
}

describe("PgSessionStore.write (T2a: rotation must not orphan the superseded row)", () => {
	it("write() twice for the same session leaves exactly one row — the superseded sid is deleted", async () => {
		const executor = new FakePgExecutor();
		const store = newStore({ executor });

		// First write: a fresh login, no existing cookie on the request.
		const reply1 = fakeReply();
		await store.write(reply1, session());
		const sid1 = reply1.captured!.value;
		assert.equal(executor.rows.size, 1, "first write leaves one row");
		assert.ok(executor.rows.has(sid1));

		// Second write: a mid-session refresh — the request now carries sid1,
		// exactly as plugin.ts's onRequest hook does when it refreshes.
		const reply2 = fakeReply({ fc_session: sid1 });
		await store.write(reply2, session());
		const sid2 = reply2.captured!.value;

		assert.notEqual(sid2, sid1, "write() mints a new sid every call");
		assert.equal(
			executor.rows.size,
			1,
			"exactly one row must survive — the superseded sid's row must be gone",
		);
		assert.ok(executor.rows.has(sid2), "the new sid's row is present");
		assert.ok(!executor.rows.has(sid1), "the old sid's row must be deleted, not orphaned");
	});

	it("write() with no previous cookie (fresh login) never deletes anything else", async () => {
		const executor = new FakePgExecutor();
		const store = newStore({ executor });

		const replyA = fakeReply();
		await store.write(replyA, session());
		const replyB = fakeReply(); // a second, unrelated fresh login
		await store.write(replyB, session());

		assert.equal(executor.rows.size, 2, "two unrelated sessions must both survive");
	});
});

describe("PgSessionStore.reapExpired (T2b: selective sweep)", () => {
	it("removes only rows whose expires_at has passed", async () => {
		const executor = new FakePgExecutor();
		const store = newStore({ executor });

		const expiredReply = fakeReply();
		await store.write(expiredReply, session({ expiresAt: Date.now() - 1000 }));
		const liveReply = fakeReply();
		await store.write(liveReply, session({ expiresAt: Date.now() + 60 * 60_000 }));

		assert.equal(executor.rows.size, 2, "sanity: both rows written");

		const removed = await store.reapExpired();

		assert.equal(removed, 1, "reapExpired reports exactly the one row it removed");
		assert.equal(executor.rows.size, 1, "exactly one row remains");
		assert.ok(
			executor.rows.has(liveReply.captured!.value),
			"the live row must survive the sweep",
		);
		assert.ok(
			!executor.rows.has(expiredReply.captured!.value),
			"the expired row must be gone",
		);
	});
});

describe("flowcatalystAuth plugin wires the reaper (T2b: wiring, disable-able)", () => {
	let app: FastifyInstance | undefined;

	after(async () => {
		await app?.close();
	});

	async function registerWith(executor: FakePgExecutor, reapIntervalMs: number | false) {
		const store = newStore({ executor, reapIntervalMs });
		const fastify = Fastify({ logger: false });
		await fastify.register(flowcatalystAuth, {
			baseUrl: "http://fc.invalid",
			clientId: "clt_test",
			clientSecret: "secret",
			cookie: { secret: generateSessionSecret(), secure: false, sameSite: "lax" },
			sessionStore: store,
		});
		await fastify.ready();
		return { fastify, store };
	}

	it("a registered plugin actually sweeps expired rows over time, with no manual reapExpired() call", async () => {
		const executor = new FakePgExecutor();
		const { fastify } = await registerWith(executor, 15);
		app = fastify;

		// Seed an already-expired row directly through the store (black-box —
		// no reliance on internals beyond the PgQueryable contract write()
		// already uses).
		const reply = fakeReply();
		const seededStore = newStore({ executor, reapIntervalMs: false }); // just for the write helper
		await seededStore.write(reply, session({ expiresAt: Date.now() - 1000 }));
		assert.equal(executor.rows.size, 1, "sanity: the expired row was written");

		// Wait past several reap ticks — long enough that this only stays
		// green because the plugin's wiring actually started the interval.
		await new Promise((resolve) => setTimeout(resolve, 200));

		assert.equal(
			executor.rows.size,
			0,
			"the plugin-wired periodic reap must have swept the expired row on its own",
		);
	});

	it("reapIntervalMs: false opts the wired reaper out entirely", async () => {
		const executor = new FakePgExecutor();
		const { fastify } = await registerWith(executor, false);
		app = fastify;

		const reply = fakeReply();
		const seededStore = newStore({ executor, reapIntervalMs: false });
		await seededStore.write(reply, session({ expiresAt: Date.now() - 1000 }));

		await new Promise((resolve) => setTimeout(resolve, 200));

		assert.equal(
			executor.rows.size,
			1,
			"an opted-out store must never sweep, even though the plugin still wires it",
		);
	});

	it("close() stops the sweep so it does not run after the app shuts down", async () => {
		const executor = new FakePgExecutor();
		const { fastify, store } = await registerWith(executor, 15);
		await fastify.close(); // triggers the plugin's onClose -> store.close()

		const reply = fakeReply();
		await store.write(reply, session({ expiresAt: Date.now() - 1000 }));
		await new Promise((resolve) => setTimeout(resolve, 100));

		assert.equal(
			executor.rows.size,
			1,
			"no sweep should run once the store has been closed",
		);
	});
});
