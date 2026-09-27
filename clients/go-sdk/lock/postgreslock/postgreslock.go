// Package postgreslock is a table-based distributed lock that
// satisfies lock.Provider. Opt-in: importing this package pulls in
// pgx/v5; the root lock package has no driver dependency.
//
// pg_try_advisory_lock would be faster, but advisory locks have no
// TTL — a crashed holder keeps the lock until its session ends. With
// table-based locks the TTL is explicit and enforced by the upsert's
// WHERE clause: another holder can reclaim an expired row.
//
// Acquire is a single
// INSERT … ON CONFLICT … DO UPDATE … WHERE … RETURNING
// statement — atomic in Postgres, so no race window between checking
// and taking.
package postgreslock

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/lock"
)

// DefaultTable is the lock table name when New is used.
const DefaultTable = "fc_locks"

// Provider is a Postgres-backed implementation of lock.Provider.
//
// Each Acquire mints a unique holder token (random UUID) so Release
// only deletes locks the caller actually owns — protects against
// accidental release of a lock that has since been reclaimed by
// another holder.
type Provider struct {
	pool  *pgxpool.Pool
	table string
}

// New builds a provider against pool using DefaultTable.
func New(pool *pgxpool.Pool) *Provider {
	return &Provider{pool: pool, table: DefaultTable}
}

// WithTable overrides the table name.
func WithTable(pool *pgxpool.Pool, table string) *Provider {
	return &Provider{pool: pool, table: table}
}

// Acquire — see lock.Provider.Acquire.
func (p *Provider) Acquire(ctx context.Context, key string, ttl time.Duration) (lock.Handle, error) {
	if err := lock.EnsurePositiveTTL(ttl); err != nil {
		return nil, err
	}
	holder := uuid.NewString()
	expiresAt := time.Now().Add(ttl)

	// Upsert with WHERE so we only displace an expired holder.
	// RETURNING returns our holder iff we actually inserted or
	// updated; the no-op case (existing non-expired row) returns
	// nothing.
	sql := fmt.Sprintf(
		`INSERT INTO %s (key, holder, expires_at) VALUES ($1, $2, $3)
		 ON CONFLICT (key) DO UPDATE
		    SET holder = EXCLUDED.holder, expires_at = EXCLUDED.expires_at
		    WHERE %s.expires_at <= NOW()
		 RETURNING holder`,
		p.table, p.table)

	var winner string
	err := p.pool.QueryRow(ctx, sql, key, holder, expiresAt).Scan(&winner)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, nil // contended
	}
	if err != nil {
		return nil, fmt.Errorf("%w: %s", lock.ErrBackend, err)
	}
	if winner != holder {
		return nil, nil // someone else won (extremely rare race)
	}
	return &handle{pool: p.pool, table: p.table, key: key, holder: holder}, nil
}

// ReapExpired removes rows whose TTL has elapsed without being
// released and returns the number removed. Optional — Acquire
// reclaims expired rows implicitly via the upsert.
func (p *Provider) ReapExpired(ctx context.Context) (int64, error) {
	sql := fmt.Sprintf("DELETE FROM %s WHERE expires_at <= NOW()", p.table)
	tag, err := p.pool.Exec(ctx, sql)
	if err != nil {
		return 0, fmt.Errorf("%w: %s", lock.ErrBackend, err)
	}
	return tag.RowsAffected(), nil
}

type handle struct {
	pool     *pgxpool.Pool
	table    string
	key      string
	holder   string
	released bool
}

// Release deletes the row only if we still hold it — protects against
// stale releasers stomping a lock that has been reclaimed.
func (h *handle) Release(ctx context.Context) error {
	if h.released {
		return nil
	}
	h.released = true
	sql := fmt.Sprintf("DELETE FROM %s WHERE key = $1 AND holder = $2", h.table)
	if _, err := h.pool.Exec(ctx, sql, h.key, h.holder); err != nil {
		return fmt.Errorf("%w: %s", lock.ErrBackend, err)
	}
	return nil
}

// CreateTableSQL is the schema for the lock table — substitute
// {table} for the actual name.
const CreateTableSQL = `
CREATE TABLE IF NOT EXISTS {table} (
    key TEXT PRIMARY KEY,
    holder TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS {table}_expires_at_idx ON {table} (expires_at);
`

// InitSchema creates DefaultTable. Safe to run repeatedly.
func InitSchema(ctx context.Context, pool *pgxpool.Pool) error {
	return InitSchemaWithTable(ctx, pool, DefaultTable)
}

// InitSchemaWithTable creates the lock table with a custom name.
func InitSchemaWithTable(ctx context.Context, pool *pgxpool.Pool, table string) error {
	sql := strings.ReplaceAll(CreateTableSQL, "{table}", table)
	for _, stmt := range strings.Split(sql, ";") {
		trimmed := strings.TrimSpace(stmt)
		if trimmed == "" {
			continue
		}
		if _, err := pool.Exec(ctx, trimmed); err != nil {
			return fmt.Errorf("init lock schema: %w", err)
		}
	}
	return nil
}
