package cache_test

import (
	"context"
	"errors"
	"sync/atomic"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/cache"
)

func TestMemoryRoundTripBytes(t *testing.T) {
	c := cache.NewMemory()
	require.NoError(t, c.SetBytes(context.Background(), "k", []byte("hello"), time.Minute))
	v, ok, err := c.GetBytes(context.Background(), "k")
	require.NoError(t, err)
	require.True(t, ok)
	assert.Equal(t, []byte("hello"), v)
}

func TestMemoryMissReturnsFalse(t *testing.T) {
	c := cache.NewMemory()
	_, ok, err := c.GetBytes(context.Background(), "nope")
	require.NoError(t, err)
	assert.False(t, ok)
}

func TestMemoryDelete(t *testing.T) {
	c := cache.NewMemory()
	require.NoError(t, c.SetBytes(context.Background(), "k", []byte("v"), time.Minute))
	require.NoError(t, c.Delete(context.Background(), "k"))
	_, ok, _ := c.GetBytes(context.Background(), "k")
	assert.False(t, ok)
}

func TestMemoryExpiredEntryIsMiss(t *testing.T) {
	c := cache.NewMemory()
	require.NoError(t, c.SetBytes(context.Background(), "k", []byte("v"), 10*time.Millisecond))
	time.Sleep(20 * time.Millisecond)
	_, ok, _ := c.GetBytes(context.Background(), "k")
	assert.False(t, ok)
}

func TestMemoryRejectsZeroTTL(t *testing.T) {
	c := cache.NewMemory()
	err := c.SetBytes(context.Background(), "k", []byte("v"), 0)
	require.Error(t, err)
	assert.True(t, errors.Is(err, cache.ErrInvalidTTL))
}

func TestMemoryByteCopyOnRead(t *testing.T) {
	c := cache.NewMemory()
	require.NoError(t, c.SetBytes(context.Background(), "k", []byte("hello"), time.Minute))
	v1, _, _ := c.GetBytes(context.Background(), "k")
	v1[0] = 'X' // mutate caller copy
	v2, _, _ := c.GetBytes(context.Background(), "k")
	assert.Equal(t, byte('h'), v2[0], "stored value must not be mutated by caller")
}

func TestTypedSetAndGet(t *testing.T) {
	c := cache.NewMemory()
	type User struct {
		ID, Name string
	}
	require.NoError(t, cache.Set(context.Background(), c, "u:1", User{"1", "Alice"}, time.Minute))
	u, ok, err := cache.Get[User](context.Background(), c, "u:1")
	require.NoError(t, err)
	require.True(t, ok)
	assert.Equal(t, "Alice", u.Name)
}

func TestGetReturnsDeserializeErrorOnTypeMismatch(t *testing.T) {
	c := cache.NewMemory()
	require.NoError(t, c.SetBytes(context.Background(), "k", []byte(`"hi"`), time.Minute))
	_, _, err := cache.Get[int](context.Background(), c, "k")
	require.Error(t, err)
	var de *cache.DeserializeError
	assert.True(t, errors.As(err, &de))
}

func TestGetOrSetHitsCache(t *testing.T) {
	c := cache.NewMemory()
	require.NoError(t, cache.Set(context.Background(), c, "k", "cached", time.Minute))
	var calls atomic.Int32
	v, err := cache.GetOrSet(context.Background(), c, "k", time.Minute,
		func(_ context.Context) (string, error) {
			calls.Add(1)
			return "fresh", nil
		})
	require.NoError(t, err)
	assert.Equal(t, "cached", v)
	assert.Equal(t, int32(0), calls.Load())
}

func TestGetOrSetCallsSupplierOnMiss(t *testing.T) {
	c := cache.NewMemory()
	v, err := cache.GetOrSet(context.Background(), c, "k", time.Minute,
		func(_ context.Context) (string, error) { return "fresh", nil })
	require.NoError(t, err)
	assert.Equal(t, "fresh", v)
	stored, ok, _ := cache.Get[string](context.Background(), c, "k")
	require.True(t, ok)
	assert.Equal(t, "fresh", stored)
}

func TestReapExpiredDropsStale(t *testing.T) {
	c := cache.NewMemory()
	require.NoError(t, c.SetBytes(context.Background(), "stale", []byte("1"), 5*time.Millisecond))
	require.NoError(t, c.SetBytes(context.Background(), "alive", []byte("2"), time.Minute))
	time.Sleep(15 * time.Millisecond)
	c.ReapExpired(context.Background())
	_, staleOK, _ := c.GetBytes(context.Background(), "stale")
	_, aliveOK, _ := c.GetBytes(context.Background(), "alive")
	assert.False(t, staleOK)
	assert.True(t, aliveOK)
}
