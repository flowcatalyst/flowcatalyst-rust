package auth_test

import (
	"crypto/sha256"
	"encoding/base64"
	"testing"

	"github.com/stretchr/testify/assert"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/auth"
)

func TestPkceChallengeUsesS256OfVerifier(t *testing.T) {
	p := auth.NewPkceChallenge()

	assert.NotEmpty(t, p.CodeVerifier)
	assert.NotEmpty(t, p.CodeChallenge)
	assert.Equal(t, "S256", p.CodeChallengeMethod)

	sum := sha256.Sum256([]byte(p.CodeVerifier))
	expect := base64.RawURLEncoding.EncodeToString(sum[:])
	assert.Equal(t, expect, p.CodeChallenge)
}

func TestPkceChallengesAreUnique(t *testing.T) {
	a := auth.NewPkceChallenge()
	b := auth.NewPkceChallenge()
	assert.NotEqual(t, a.CodeVerifier, b.CodeVerifier)
	assert.NotEqual(t, a.CodeChallenge, b.CodeChallenge)
}
