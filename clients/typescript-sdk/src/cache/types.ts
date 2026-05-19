/**
 * Cache primitive
 *
 * Pluggable key-value cache. TTL is **required** on every write —
 * open-ended caches silently leak memory in long-running services.
 *
 * Backends:
 *   - {@link MemoryCacheStore} — process-local, default for tests/dev.
 *   - {@link PgCacheStore} — Postgres-backed (any node-postgres-compatible
 *     pool or client). Ships {@link initCacheSchema} for the migration.
 *   - {@link RedisCacheStore} — ioredis-compatible client. Expiry enforced
 *     by Redis itself.
 *
 * Mirrors the Rust SDK's `Cache` trait so apps written in either language
 * follow the same shape.
 */

/**
 * Errors thrown by a {@link CacheStore} implementation. Backends should
 * throw `CacheError` (or a subclass) rather than leaking driver-specific
 * errors. Use {@link CacheError.invalidTtl} for zero / negative TTLs.
 */
export class CacheError extends Error {
	readonly underlyingCause?: unknown;

	constructor(message: string, cause?: unknown) {
		super(message);
		this.name = "CacheError";
		this.underlyingCause = cause;
	}

	static invalidTtl(): CacheError {
		return new CacheError("cache TTL must be greater than zero");
	}

	static backend(message: string, cause?: unknown): CacheError {
		return new CacheError(`cache backend error: ${message}`, cause);
	}

	static deserialize(message: string, cause?: unknown): CacheError {
		return new CacheError(`cache value deserialization failed: ${message}`, cause);
	}

	static serialize(message: string, cause?: unknown): CacheError {
		return new CacheError(`cache value serialization failed: ${message}`, cause);
	}
}

/**
 * Pluggable cache contract.
 *
 * `set` requires a positive `ttlMs`. Backends should throw
 * {@link CacheError.invalidTtl} if `ttlMs <= 0`.
 *
 * `get` returns `null` for both misses AND expired entries — callers can't
 * distinguish, which is the right semantics for a cache.
 *
 * `getOrSet` is read-through: returns the cached value on hit, else calls
 * `supplier`, caches the result, and returns it. It's NOT atomic across
 * replicas; if you need exactly-once supplier execution, layer a
 * `LockProvider` around the call.
 */
export interface CacheStore {
	get<T>(key: string): Promise<T | null>;
	set<T>(key: string, value: T, ttlMs: number): Promise<void>;
	delete(key: string): Promise<void>;
	getOrSet<T>(
		key: string,
		ttlMs: number,
		supplier: () => Promise<T>,
	): Promise<T>;
}

/** Internal helper used by every backend to validate TTL the same way. */
export function ensurePositiveTtl(ttlMs: number): void {
	if (!Number.isFinite(ttlMs) || ttlMs <= 0) {
		throw CacheError.invalidTtl();
	}
}

/**
 * Minimal `pg`-compatible query interface. `pg.Pool`, `pg.PoolClient`, and
 * Drizzle's underlying client all satisfy this shape — no explicit `pg`
 * dependency required.
 *
 * Defined here (not pulled from the outbox module) so the cache + lock
 * primitives can be shipped independently of the outbox driver changes.
 */
export interface PgQueryable {
	query(
		text: string,
		params?: ReadonlyArray<unknown>,
	): Promise<{ rows?: unknown[]; rowCount?: number | null } | unknown>;
}
