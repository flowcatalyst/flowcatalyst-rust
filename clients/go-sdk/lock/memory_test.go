package lock_test

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/lock"
)

func TestNoOpAlwaysSucceeds(t *testing.T) {
	p := lock.NewNoOp()
	h, err := p.Acquire(context.Background(), "k", time.Second)
	require.NoError(t, err)
	require.NotNil(t, h)
	require.NoError(t, h.Release(context.Background()))
}

func TestMemoryExcludesConcurrentHolders(t *testing.T) {
	p := lock.NewMemory()
	h1, err := p.Acquire(context.Background(), "k", 30*time.Second)
	require.NoError(t, err)
	require.NotNil(t, h1)

	h2, err := p.Acquire(context.Background(), "k", 30*time.Second)
	require.NoError(t, err)
	assert.Nil(t, h2, "second acquire should fail while h1 holds")

	require.NoError(t, h1.Release(context.Background()))
	h3, err := p.Acquire(context.Background(), "k", 30*time.Second)
	require.NoError(t, err)
	require.NotNil(t, h3, "should reacquire after release")
	require.NoError(t, h3.Release(context.Background()))
}

func TestMemoryExpiresAfterTTL(t *testing.T) {
	p := lock.NewMemory()
	_, err := p.Acquire(context.Background(), "k", 10*time.Millisecond)
	require.NoError(t, err)
	time.Sleep(20 * time.Millisecond)
	h2, err := p.Acquire(context.Background(), "k", 30*time.Second)
	require.NoError(t, err)
	require.NotNil(t, h2, "should acquire after previous TTL expires")
}

func TestMemoryRejectsZeroTTL(t *testing.T) {
	p := lock.NewMemory()
	_, err := p.Acquire(context.Background(), "k", 0)
	require.Error(t, err)
	assert.True(t, errors.Is(err, lock.ErrInvalidTTL))
}

func TestMemoryReleaseIsIdempotent(t *testing.T) {
	p := lock.NewMemory()
	h, err := p.Acquire(context.Background(), "k", time.Second)
	require.NoError(t, err)
	require.NoError(t, h.Release(context.Background()))
	require.NoError(t, h.Release(context.Background()))
}

func TestMemoryReleaseDoesntStompReclaimedLock(t *testing.T) {
	p := lock.NewMemory()
	h1, err := p.Acquire(context.Background(), "k", 5*time.Millisecond)
	require.NoError(t, err)
	time.Sleep(10 * time.Millisecond)

	// h2 reclaims after TTL expiry.
	h2, err := p.Acquire(context.Background(), "k", 30*time.Second)
	require.NoError(t, err)
	require.NotNil(t, h2)

	// Stale h1.Release must not displace the live h2 lease.
	require.NoError(t, h1.Release(context.Background()))

	h3, err := p.Acquire(context.Background(), "k", 30*time.Second)
	require.NoError(t, err)
	assert.Nil(t, h3, "h2 should still hold the key")
}
