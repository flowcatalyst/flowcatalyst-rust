/**
 * Process-local in-memory cache.
 *
 * Suitable for tests, single-Node-process dev servers, and anywhere durable
 * cross-process state isn't needed. Expired entries are reaped lazily on
 * read; call {@link MemoryCacheStore.reapExpired} from a periodic task if
 * you write keys that are rarely read back.
 */

import { type CacheStore, ensurePositiveTtl } from "./types.js";

interface Entry {
	value: unknown;
	expiresAt: number;
}

export class MemoryCacheStore implements CacheStore {
	private readonly entries: Map<string, Entry> = new Map();

	async get<T>(key: string): Promise<T | null> {
		const entry = this.entries.get(key);
		if (!entry) return null;
		if (entry.expiresAt <= Date.now()) {
			this.entries.delete(key);
			return null;
		}
		return entry.value as T;
	}

	async set<T>(key: string, value: T, ttlMs: number): Promise<void> {
		ensurePositiveTtl(ttlMs);
		this.entries.set(key, { value, expiresAt: Date.now() + ttlMs });
	}

	async delete(key: string): Promise<void> {
		this.entries.delete(key);
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

	/** Drop entries whose TTL has elapsed. Returns count removed. */
	reapExpired(): number {
		const now = Date.now();
		let removed = 0;
		for (const [key, entry] of this.entries) {
			if (entry.expiresAt <= now) {
				this.entries.delete(key);
				removed++;
			}
		}
		return removed;
	}

	/** @internal — exposed for tests; do not use in production code. */
	size(): number {
		return this.entries.size;
	}
}
