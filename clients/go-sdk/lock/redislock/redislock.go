// Package redislock is a Redis-backed distributed lock that
// satisfies lock.Provider. Opt-in: importing this package pulls in
// github.com/redis/go-redis/v9; the root lock package has no driver
// dependency.
//
// SET NX PX <ttl_ms> for Acquire (atomic with TTL), Lua
// check-and-delete for Release so we only delete locks whose token we
// still own. This protects against a stale releaser stomping a lock
// that's already been reclaimed after a TTL expiry.
package redislock

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/google/uuid"
	"github.com/redis/go-redis/v9"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/lock"
)

// DefaultPrefix prepended to every lock key when New is used.
const DefaultPrefix = "fc:lock"

// Provider is a Redis-backed implementation of lock.Provider. Pass
// any redis.UniversalClient — standalone, sentinel, or cluster — so
// consumers control connection / reconnect handling.
type Provider struct {
	client redis.UniversalClient
	prefix string
	script *redis.Script
}

// New builds a provider with DefaultPrefix.
func New(client redis.UniversalClient) *Provider {
	return WithPrefix(client, DefaultPrefix)
}

// WithPrefix builds a provider that prepends prefix + ":" to every key.
func WithPrefix(client redis.UniversalClient, prefix string) *Provider {
	return &Provider{
		client: client,
		prefix: prefix,
		script: redis.NewScript(releaseScript),
	}
}

// Lua: delete the key only if its current value matches our token.
const releaseScript = `
if redis.call("GET", KEYS[1]) == ARGV[1] then
    return redis.call("DEL", KEYS[1])
else
    return 0
end
`

func (p *Provider) fullKey(key string) string { return p.prefix + ":" + key }

// Acquire — see lock.Provider.Acquire.
func (p *Provider) Acquire(ctx context.Context, key string, ttl time.Duration) (lock.Handle, error) {
	if err := lock.EnsurePositiveTTL(ttl); err != nil {
		return nil, err
	}
	token := uuid.NewString()
	ok, err := p.client.SetNX(ctx, p.fullKey(key), token, ttl).Result()
	if err != nil {
		return nil, fmt.Errorf("%w: %s", lock.ErrBackend, err)
	}
	if !ok {
		return nil, nil // contended
	}
	return &handle{client: p.client, fullKey: p.fullKey(key), token: token, script: p.script}, nil
}

type handle struct {
	client   redis.UniversalClient
	fullKey  string
	token    string
	script   *redis.Script
	released bool
}

// Release runs the Lua check-and-delete script.
func (h *handle) Release(ctx context.Context) error {
	if h.released {
		return nil
	}
	h.released = true
	_, err := h.script.Run(ctx, h.client, []string{h.fullKey}, h.token).Result()
	if err != nil && !errors.Is(err, redis.Nil) {
		return fmt.Errorf("%w: %s", lock.ErrBackend, err)
	}
	return nil
}
