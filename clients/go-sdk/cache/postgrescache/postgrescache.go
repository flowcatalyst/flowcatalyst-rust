// Package postgrescache is a pgx-backed cache that satisfies
// cache.Cache. Opt-in: importing this package pulls in pgx/v5; the
// root cache package has no driver dependency.
//
// Stores values as BYTEA in a table (default name: fc_cache). Reads
// filter on expires_at > NOW() so an expired row is invisible even
// before it's reaped; writes upsert on the primary key so callers can
// refresh the TTL by writing again.
//
// Call InitSchema once at startup (or fold the SQL into your own
// migrations) before the first Get / Set.
package postgrescache

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/cache"
)

// DefaultTable is the cache table name when New is used.
const DefaultTable = "fc_cache"

// Cache is a Postgres-backed implementation of cache.Cache.
type Cache struct {
	pool  *pgxpool.Pool
	table string
}

// New builds a cache against pool using DefaultTable.
func New(pool *pgxpool.Pool) *Cache {
	return &Cache{pool: pool, table: DefaultTable}
}

// WithTable overrides the table name (for multi-tenant deployments
// that need separate cache tables per service).
func WithTable(pool *pgxpool.Pool, table string) *Cache {
	return &Cache{pool: pool, table: table}
}

// GetBytes — see cache.Cache.GetBytes.
func (c *Cache) GetBytes(ctx context.Context, key string) ([]byte, bool, error) {
	sql := fmt.Sprintf("SELECT value FROM %s WHERE key = $1 AND expires_at > NOW()", c.table)
	var value []byte
	err := c.pool.QueryRow(ctx, sql, key).Scan(&value)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, false, nil
	}
	if err != nil {
		return nil, false, fmt.Errorf("%w: %s", cache.ErrBackend, err)
	}
	return value, true, nil
}

// SetBytes — see cache.Cache.SetBytes.
func (c *Cache) SetBytes(ctx context.Context, key string, value []byte, ttl time.Duration) error {
	if err := cache.EnsurePositiveTTL(ttl); err != nil {
		return err
	}
	sql := fmt.Sprintf(
		"INSERT INTO %s (key, value, expires_at) VALUES ($1, $2, $3) "+
			"ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, expires_at = EXCLUDED.expires_at",
		c.table,
	)
	expiresAt := time.Now().Add(ttl)
	if _, err := c.pool.Exec(ctx, sql, key, value, expiresAt); err != nil {
		return fmt.Errorf("%w: %s", cache.ErrBackend, err)
	}
	return nil
}

// Delete — see cache.Cache.Delete.
func (c *Cache) Delete(ctx context.Context, key string) error {
	sql := fmt.Sprintf("DELETE FROM %s WHERE key = $1", c.table)
	if _, err := c.pool.Exec(ctx, sql, key); err != nil {
		return fmt.Errorf("%w: %s", cache.ErrBackend, err)
	}
	return nil
}

// ReapExpired removes rows whose TTL has elapsed and returns the
// number removed. Cheap thanks to the index on expires_at.
func (c *Cache) ReapExpired(ctx context.Context) (int64, error) {
	sql := fmt.Sprintf("DELETE FROM %s WHERE expires_at <= NOW()", c.table)
	tag, err := c.pool.Exec(ctx, sql)
	if err != nil {
		return 0, fmt.Errorf("%w: %s", cache.ErrBackend, err)
	}
	return tag.RowsAffected(), nil
}

// CreateTableSQL is the schema for the cache table — substitute {table}
// for the actual name. Indexed on expires_at to make reaping cheap.
const CreateTableSQL = `
CREATE TABLE IF NOT EXISTS {table} (
    key TEXT PRIMARY KEY,
    value BYTEA NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS {table}_expires_at_idx ON {table} (expires_at);
`

// InitSchema creates DefaultTable. Safe to run repeatedly.
func InitSchema(ctx context.Context, pool *pgxpool.Pool) error {
	return InitSchemaWithTable(ctx, pool, DefaultTable)
}

// InitSchemaWithTable creates the cache table with a custom name.
func InitSchemaWithTable(ctx context.Context, pool *pgxpool.Pool, table string) error {
	sql := strings.ReplaceAll(CreateTableSQL, "{table}", table)
	for _, stmt := range strings.Split(sql, ";") {
		trimmed := strings.TrimSpace(stmt)
		if trimmed == "" {
			continue
		}
		if _, err := pool.Exec(ctx, trimmed); err != nil {
			return fmt.Errorf("init cache schema: %w", err)
		}
	}
	return nil
}
