/**
 * Cache primitive.
 *
 * Pluggable key-value cache with required TTL. See {@link CacheStore} for
 * the interface and {@link MemoryCacheStore} / {@link PgCacheStore} /
 * {@link RedisCacheStore} for the three shipped backends.
 */

export { CacheError, type CacheStore } from "./types.js";
export { MemoryCacheStore } from "./memory-cache-store.js";
export { PgCacheStore } from "./pg-cache-store.js";
export {
	RedisCacheStore,
	type RedisCommandable,
	type RedisCacheStoreOptions,
} from "./redis-cache-store.js";
export {
	CREATE_CACHE_TABLE_SQL,
	initCacheSchema,
	initCacheSchemaWithTable,
} from "./schema.js";
