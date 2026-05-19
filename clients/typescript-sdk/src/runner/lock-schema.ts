/**
 * Migration helper for {@link PgLockProvider}.
 *
 * Run {@link initLockSchema} once at startup (or fold the SQL into your
 * existing migration tool) before the first `PgLockProvider.acquire`.
 * Idempotent — `CREATE TABLE IF NOT EXISTS` and `CREATE INDEX IF NOT EXISTS`.
 */

import type { PgQueryable } from "../cache/types.js";

/**
 * SQL to create the lock table + supporting index. Default table name is
 * `fc_locks`; use {@link initLockSchemaWithTable} to override.
 */
export const CREATE_LOCK_TABLE_SQL = `
CREATE TABLE IF NOT EXISTS {table} (
    key TEXT PRIMARY KEY,
    holder TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS {table}_expires_at_idx ON {table} (expires_at);
`;

/** Create the `fc_locks` table. Safe to run repeatedly. */
export async function initLockSchema(client: PgQueryable): Promise<void> {
	return initLockSchemaWithTable(client, "fc_locks");
}

/** Create the lock table with a custom name. */
export async function initLockSchemaWithTable(
	client: PgQueryable,
	table: string,
): Promise<void> {
	const sql = CREATE_LOCK_TABLE_SQL.replace(/\{table\}/g, table);
	for (const stmt of sql
		.split(";")
		.map((s) => s.trim())
		.filter((s) => s.length > 0)) {
		await client.query(stmt);
	}
}
