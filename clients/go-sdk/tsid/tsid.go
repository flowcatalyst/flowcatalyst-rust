package tsid

import (
	"crypto/rand"
	"encoding/binary"
	"strings"
	"sync/atomic"
	"time"
)

// alphabet is the Crockford Base32 alphabet (excludes I, L, O, U).
var alphabet = []byte("0123456789ABCDEFGHJKMNPQRSTVWXYZ")

var counter atomic.Uint32

// Generate returns a typed ID with the entity's 3-char prefix: "clt_0HZXEQ5Y8JY5Z".
func Generate(e EntityType) string {
	return e.Prefix() + "_" + raw()
}

// GenerateWithPrefix returns a typed ID with a custom 3-char prefix.
// Use this for application-specific entity types not in EntityType.
func GenerateWithPrefix(prefix string) string {
	return prefix + "_" + raw()
}

// GenerateUntyped returns the raw 13-char TSID with no prefix.
// Use for non-entity IDs (execution IDs, trace IDs, outbox message IDs).
func GenerateUntyped() string {
	return raw()
}

// raw builds the 13-char Crockford Base32 TSID.
//   - 42 bits: milliseconds since Unix epoch
//   - 10 bits: cryptographically-random
//   - 12 bits: process-local atomic counter (mod 4096)
func raw() string {
	now := uint64(time.Now().UnixMilli()) & 0x3FFFFFFFFFF
	r := uint64(randomU16()) & 0x3FF
	c := uint64(counter.Add(1)) & 0xFFF
	// Layout: 42 bits timestamp | 10 bits random | 12 bits counter.
	// Matches crates/fc-common-types/src/tsid.rs exactly.
	v := (now << 22) | (r << 12) | c
	return encodeCrockford(v)
}

// encodeCrockford renders a 64-bit value as 13 Crockford Base32 chars
// (no padding, base 32 = 5 bits per char × 13 = 65 bits).
func encodeCrockford(v uint64) string {
	out := make([]byte, 13)
	for i := 12; i >= 0; i-- {
		out[i] = alphabet[v&0x1F]
		v >>= 5
	}
	return string(out)
}

// DecodeCrockford parses a 13-char Crockford Base32 string back to its
// 64-bit value. Returns 0, false on malformed input.
func DecodeCrockford(s string) (uint64, bool) {
	if len(s) != 13 {
		return 0, false
	}
	var v uint64
	for i := 0; i < 13; i++ {
		c := s[i]
		if c >= 'a' && c <= 'z' {
			c -= 'a' - 'A'
		}
		var d uint64
		switch {
		case c >= '0' && c <= '9':
			d = uint64(c - '0')
		case c >= 'A' && c <= 'H':
			d = uint64(c-'A') + 10
		case c >= 'J' && c <= 'K':
			d = uint64(c-'J') + 18
		case c >= 'M' && c <= 'N':
			d = uint64(c-'M') + 20
		case c >= 'P' && c <= 'T':
			d = uint64(c-'P') + 22
		case c >= 'V' && c <= 'Z':
			d = uint64(c-'V') + 27
		default:
			return 0, false
		}
		v = (v << 5) | d
	}
	return v, true
}

// ToLong converts a TSID to its numeric form. Handles both typed
// ("clt_0HZXEQ5Y8JY5Z") and raw ("0HZXEQ5Y8JY5Z") inputs.
func ToLong(s string) (int64, bool) {
	r := s
	if i := strings.Index(s, "_"); i >= 0 && len(s) > 14 {
		r = s[i+1:]
	}
	v, ok := DecodeCrockford(r)
	return int64(v), ok
}

// FromLong renders a numeric TSID as a raw 13-char string (no prefix).
func FromLong(v int64) string { return encodeCrockford(uint64(v)) }

// randomU16 returns 16 bits of crypto-grade randomness.
func randomU16() uint16 {
	var b [2]byte
	if _, err := rand.Read(b[:]); err != nil {
		// crypto/rand should never fail on supported platforms; fall
		// back to time-based mixing if it does.
		now := uint64(time.Now().UnixNano())
		return uint16(now ^ (now >> 16))
	}
	return binary.BigEndian.Uint16(b[:])
}
