package usecasepgx

import (
	"context"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/usecase"
)

// Persist is implemented by a repository to upsert and delete aggregates
// of type A within a transaction. Implement on the *repository*, not the
// aggregate — the CLAUDE.md "aggregates don't persist themselves" rule
// applies in Go too.
//
// Example:
//
//	type OrderRepository struct{ pool *pgxpool.Pool }
//
//	func (r *OrderRepository) Persist(ctx context.Context, o *Order, tx *usecasepgx.DbTx) error {
//	    _, err := tx.Inner().Exec(ctx,
//	        `INSERT INTO orders (id, customer_id, total) VALUES ($1, $2, $3)
//	         ON CONFLICT (id) DO UPDATE SET customer_id = $2, total = $3`,
//	        o.ID, o.CustomerID, o.Total)
//	    return err
//	}
//
//	func (r *OrderRepository) Delete(ctx context.Context, o *Order, tx *usecasepgx.DbTx) error {
//	    _, err := tx.Inner().Exec(ctx, `DELETE FROM orders WHERE id = $1`, o.ID)
//	    return err
//	}
type Persist[A usecase.HasID] interface {
	Persist(ctx context.Context, agg *A, tx *DbTx) error
	Delete(ctx context.Context, agg *A, tx *DbTx) error
}
