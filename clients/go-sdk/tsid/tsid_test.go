package tsid_test

import (
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/tsid"
)

func TestGenerateTypedID(t *testing.T) {
	id := tsid.Generate(tsid.Client)
	assert.Len(t, id, 17)                   // "clt_" + 13 chars
	assert.True(t, strings.HasPrefix(id, "clt_"))
}

func TestGenerateWithCustomPrefix(t *testing.T) {
	id := tsid.GenerateWithPrefix("ord")
	assert.Len(t, id, 17)
	assert.True(t, strings.HasPrefix(id, "ord_"))
}

func TestGenerateUntyped(t *testing.T) {
	id := tsid.GenerateUntyped()
	assert.Len(t, id, 13)
}

func TestUniqueness(t *testing.T) {
	seen := make(map[string]struct{}, 10000)
	for i := 0; i < 10000; i++ {
		id := tsid.Generate(tsid.Client)
		_, dup := seen[id]
		require.False(t, dup, "duplicate TSID at iteration %d: %s", i, id)
		seen[id] = struct{}{}
	}
}

func TestPrefixCoverage(t *testing.T) {
	// Every EntityType variant must return a non-empty 3-char prefix.
	for et := tsid.Client; et <= tsid.Process; et++ {
		p := et.Prefix()
		assert.Len(t, p, 3, "entity %d has bad-length prefix %q", et, p)
		assert.NotEqual(t, "unk", p, "entity %d returned the unknown fallback", et)
	}
}

func TestRoundTripDecode(t *testing.T) {
	raw := tsid.GenerateUntyped()
	n, ok := tsid.DecodeCrockford(raw)
	require.True(t, ok)
	assert.Equal(t, raw, encodeViaToLong(n))
}

func TestToLongHandlesTypedAndRaw(t *testing.T) {
	typed := tsid.Generate(tsid.Client)
	raw := strings.TrimPrefix(typed, "clt_")

	a, ok := tsid.ToLong(typed)
	require.True(t, ok)
	b, ok := tsid.ToLong(raw)
	require.True(t, ok)
	assert.Equal(t, a, b)
}

func TestDecodeCrockfordRejectsShort(t *testing.T) {
	_, ok := tsid.DecodeCrockford("ABC")
	assert.False(t, ok)
}

func TestDecodeCrockfordRejectsInvalidChars(t *testing.T) {
	// 'I' is not in the Crockford alphabet (skipped to avoid 1/I ambiguity).
	_, ok := tsid.DecodeCrockford("IIIIIIIIIIIII")
	assert.False(t, ok)
}

func encodeViaToLong(n uint64) string {
	return tsid.FromLong(int64(n))
}
