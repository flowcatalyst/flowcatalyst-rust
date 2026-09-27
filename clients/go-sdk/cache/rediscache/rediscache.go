// Package rediscache is a Redis-backed cache that satisfies
// cache.Cache. Opt-in: importing this package pulls in
// github.com/redis/go-redis/v9; the root cache package has no driver
// dependency.
//
// Uses SET key value PX millis for writes (atomic with TTL) and GET
// for reads. TTL is enforced by Redis itself, so there's no separate
// reaper to run — expired keys disappear automatically.
package rediscache

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/redis/go-redis/v9"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/cache"
)

// Cache is a Redis-backed implementation of cache.Cache. Pass any
// implementation of redis.UniversalClient — standalone, sentinel, or
// cluster — so consumers control connection / reconnect handling.
type Cache struct {
	client redis.UniversalClient
	prefix string
}

// New builds a cache with no key prefix.
func New(client redis.UniversalClient) *Cache {
	return &Cache{client: client}
}

// WithPrefix prepends prefix + ":" to every key. Useful to keep
// multiple SDK consumers from colliding on a shared Redis instance.
func WithPrefix(client redis.UniversalClient, prefix string) *Cache {
	return &Cache{client: client, prefix: prefix}
}

func (c *Cache) fullKey(key string) string {
	if c.prefix == "" {
		return key
	}
	return c.prefix + ":" + key
}

// GetBytes — see cache.Cache.GetBytes.
func (c *Cache) GetBytes(ctx context.Context, key string) ([]byte, bool, error) {
	v, err := c.client.Get(ctx, c.fullKey(key)).Bytes()
	if errors.Is(err, redis.Nil) {
		return nil, false, nil
	}
	if err != nil {
		return nil, false, fmt.Errorf("%w: %s", cache.ErrBackend, err)
	}
	return v, true, nil
}

// SetBytes — see cache.Cache.SetBytes.
func (c *Cache) SetBytes(ctx context.Context, key string, value []byte, ttl time.Duration) error {
	if err := cache.EnsurePositiveTTL(ttl); err != nil {
		return err
	}
	if err := c.client.Set(ctx, c.fullKey(key), value, ttl).Err(); err != nil {
		return fmt.Errorf("%w: %s", cache.ErrBackend, err)
	}
	return nil
}

// Delete — see cache.Cache.Delete.
func (c *Cache) Delete(ctx context.Context, key string) error {
	if err := c.client.Del(ctx, c.fullKey(key)).Err(); err != nil {
		return fmt.Errorf("%w: %s", cache.ErrBackend, err)
	}
	return nil
}
