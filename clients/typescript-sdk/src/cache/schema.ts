/**
 * Migration helper for {@link PgCacheStore}.
 *
 * Run {@link initCacheSchema} once at startup (or fold the SQL into your
 * existing migration tool) before the first `PgCacheStore.get` /
 * `PgCacheStore.set`. Idempotent — `CREATE TABLE IF NOT EXISTS` and
 * `CREATE INDEX IF NOT EXISTS`.
 */

import type { PgQueryable } from "./types.js";

/**
 * SQL to create the cache table + supporting index. Default table name is
 * `fc_cache`; use {@link initCacheSchemaWithTable} to override.
 */
export const CREATE_CACHE_TABLE_SQL = `
CREATE TABLE IF NOT EXISTS {table} (
    key TEXT PRIMARY KEY,
    value BYTEA NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS {table}_expires_at_idx ON {table} (expires_at);
`;

/** Create the `fc_cache` table. Safe to run repeatedly. */
export async function initCacheSchema(client: PgQueryable): Promise<void> {
	return initCacheSchemaWithTable(client, "fc_cache");
}

/** Create the cache table with a custom name. */
export async function initCacheSchemaWithTable(
	client: PgQueryable,
	table: string,
): Promise<void> {
	const sql = CREATE_CACHE_TABLE_SQL.replace(/\{table\}/g, table);
	for (const stmt of sql
		.split(";")
		.map((s) => s.trim())
		.filter((s) => s.length > 0)) {
		await client.query(stmt);
	}
}
