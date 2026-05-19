/**
 * Postgres-backed cache.
 *
 * Duck-typed against any node-postgres-compatible client via the
 * {@link PgQueryable} shape — works with `pg.Pool`, `pg.PoolClient`, and
 * Drizzle's underlying client. No `pg` peer dep required.
 *
 * Stores values as JSON (TEXT) in a `fc_cache` table. Reads filter on
 * `expires_at > NOW()` so an expired row is invisible even before it's
 * reaped; writes upsert on the primary key so callers can refresh the TTL
 * by writing again. Run {@link initCacheSchema} once at startup (or fold
 * the SQL into your migration tool).
 *
 * Stale rows are reaped lazily by {@link PgCacheStore.reapExpired}; call it
 * from a periodic task if you write keys that are rarely read back.
 */

import {
	CacheError,
	type CacheStore,
	ensurePositiveTtl,
	type PgQueryable,
} from "./types.js";

interface PgQueryResult<R> {
	rows?: R[];
	rowCount?: number | null;
}

export class PgCacheStore implements CacheStore {
	private readonly executor: PgQueryable;
	private readonly table: string;

	constructor(executor: PgQueryable, table = "fc_cache") {
		this.executor = executor;
		this.table = table;
	}

	async get<T>(key: string): Promise<T | null> {
		try {
			const result = (await this.executor.query(
				`SELECT value FROM ${this.table} WHERE key = $1 AND expires_at > NOW()`,
				[key],
			)) as PgQueryResult<{ value: Buffer | string }>;
			const row = result.rows?.[0];
			if (!row) return null;
			const raw = row.value;
			const text = Buffer.isBuffer(raw) ? raw.toString("utf-8") : raw;
			try {
				return JSON.parse(text) as T;
			} catch (e) {
				throw CacheError.deserialize(
					e instanceof Error ? e.message : String(e),
					e,
				);
			}
		} catch (e) {
			if (e instanceof CacheError) throw e;
			throw CacheError.backend(e instanceof Error ? e.message : String(e), e);
		}
	}

	async set<T>(key: string, value: T, ttlMs: number): Promise<void> {
		ensurePositiveTtl(ttlMs);
		let payload: Buffer;
		try {
			payload = Buffer.from(JSON.stringify(value), "utf-8");
		} catch (e) {
			throw CacheError.serialize(
				e instanceof Error ? e.message : String(e),
				e,
			);
		}
		const expiresAt = new Date(Date.now() + ttlMs);
		try {
			await this.executor.query(
				`INSERT INTO ${this.table} (key, value, expires_at) VALUES ($1, $2, $3)
				 ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, expires_at = EXCLUDED.expires_at`,
				[key, payload, expiresAt],
			);
		} catch (e) {
			throw CacheError.backend(e instanceof Error ? e.message : String(e), e);
		}
	}

	async delete(key: string): Promise<void> {
		try {
			await this.executor.query(`DELETE FROM ${this.table} WHERE key = $1`, [
				key,
			]);
		} catch (e) {
			throw CacheError.backend(e instanceof Error ? e.message : String(e), e);
		}
	}

	async getOrSet<T>(
		key: string,
		ttlMs: number,
		supplier: () => Promise<T>,
	): Promise<T> {
		const hit = await this.get<T>(key);
		if (hit !== null) return hit;
		const value = await supplier();
		await this.set(key, value, ttlMs);
		return value;
	}

	/** Delete rows whose TTL has elapsed. Returns the number of rows removed. */
	async reapExpired(): Promise<number> {
		try {
			const result = (await this.executor.query(
				`DELETE FROM ${this.table} WHERE expires_at <= NOW()`,
			)) as PgQueryResult<unknown>;
			return result.rowCount ?? 0;
		} catch (e) {
			throw CacheError.backend(e instanceof Error ? e.message : String(e), e);
		}
	}
}
