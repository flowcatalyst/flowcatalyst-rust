/**
 * Redis-backed cache.
 *
 * Duck-typed against ioredis. Works directly with `new Redis()` from
 * `ioredis`; with node-redis (`redis` package, v4+) wrap your client to
 * match the {@link RedisCommandable} shape — the four methods used here
 * (`set`, `get`, `del`, `eval`) all have an ioredis-equivalent invocation.
 *
 * Uses `SET key value PX millis` for atomic writes with TTL and `GET` for
 * reads. TTL is enforced by Redis itself, so there's no separate reaper to
 * run — expired keys disappear automatically.
 */

import { CacheError, type CacheStore, ensurePositiveTtl } from "./types.js";

/**
 * Minimal ioredis-compatible command surface. ioredis users can pass their
 * Redis client directly; node-redis users can wrap their client to match.
 */
export interface RedisCommandable {
	set(
		key: string,
		value: string,
		...args: (string | number)[]
	): Promise<string | null>;
	get(key: string): Promise<string | null>;
	del(...keys: string[]): Promise<number>;
}

export interface RedisCacheStoreOptions {
	/** Key prefix prepended as `${prefix}:${key}`. Defaults to no prefix. */
	prefix?: string;
}

export class RedisCacheStore implements CacheStore {
	private readonly client: RedisCommandable;
	private readonly prefix: string;

	constructor(client: RedisCommandable, options: RedisCacheStoreOptions = {}) {
		this.client = client;
		this.prefix = options.prefix ?? "";
	}

	private makeKey(key: string): string {
		return this.prefix.length === 0 ? key : `${this.prefix}:${key}`;
	}

	async get<T>(key: string): Promise<T | null> {
		const fullKey = this.makeKey(key);
		let raw: string | null;
		try {
			raw = await this.client.get(fullKey);
		} catch (e) {
			throw CacheError.backend(e instanceof Error ? e.message : String(e), e);
		}
		if (raw === null) return null;
		try {
			return JSON.parse(raw) as T;
		} catch (e) {
			throw CacheError.deserialize(
				e instanceof Error ? e.message : String(e),
				e,
			);
		}
	}

	async set<T>(key: string, value: T, ttlMs: number): Promise<void> {
		ensurePositiveTtl(ttlMs);
		const fullKey = this.makeKey(key);
		let payload: string;
		try {
			payload = JSON.stringify(value);
		} catch (e) {
			throw CacheError.serialize(
				e instanceof Error ? e.message : String(e),
				e,
			);
		}
		try {
			await this.client.set(fullKey, payload, "PX", ttlMs);
		} catch (e) {
			throw CacheError.backend(e instanceof Error ? e.message : String(e), e);
		}
	}

	async delete(key: string): Promise<void> {
		const fullKey = this.makeKey(key);
		try {
			await this.client.del(fullKey);
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
}
