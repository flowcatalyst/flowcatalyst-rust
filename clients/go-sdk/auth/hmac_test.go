package auth_test

import (
	"errors"
	"testing"
	"time"

	"github.com/lestrrat-go/jwx/v2/jwa"
	"github.com/lestrrat-go/jwx/v2/jwt"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/auth"
)

const testSecret = "shared-secret-for-testing-12345!"

func mintHS256(t *testing.T, secret, iss, aud, sub string, exp time.Time, extras map[string]any) string {
	t.Helper()
	tok := jwt.New()
	require.NoError(t, tok.Set(jwt.SubjectKey, sub))
	require.NoError(t, tok.Set(jwt.IssuerKey, iss))
	require.NoError(t, tok.Set(jwt.AudienceKey, aud))
	require.NoError(t, tok.Set(jwt.ExpirationKey, exp))
	require.NoError(t, tok.Set(jwt.IssuedAtKey, time.Now().Add(-time.Minute)))
	require.NoError(t, tok.Set(jwt.NotBeforeKey, time.Now().Add(-time.Minute)))
	require.NoError(t, tok.Set(jwt.JwtIDKey, "j1"))
	require.NoError(t, tok.Set("type", "USER"))
	require.NoError(t, tok.Set("scope", "ANCHOR"))
	require.NoError(t, tok.Set("name", "Tester"))
	require.NoError(t, tok.Set("clients", []string{"*"}))
	require.NoError(t, tok.Set("roles", []string{"admin"}))
	for k, v := range extras {
		require.NoError(t, tok.Set(k, v))
	}
	signed, err := jwt.Sign(tok, jwt.WithKey(jwa.HS256, []byte(secret)))
	require.NoError(t, err)
	return string(signed)
}

func TestHmacValidatorAcceptsValidToken(t *testing.T) {
	v := auth.NewHmacTokenValidator(testSecret, "fc", "fc")
	tok := mintHS256(t, testSecret, "fc", "fc", "prn_1", time.Now().Add(time.Hour), map[string]any{
		"email": "u@example.com",
	})

	ctx, err := v.Validate(tok)
	require.NoError(t, err)
	assert.Equal(t, "prn_1", ctx.PrincipalID())
	assert.True(t, ctx.IsAnchor())
	assert.True(t, ctx.HasRole("admin"))
	assert.Equal(t, tok, ctx.BearerToken())
}

func TestHmacValidatorRejectsWrongSecret(t *testing.T) {
	tok := mintHS256(t, testSecret, "fc", "fc", "prn_1", time.Now().Add(time.Hour), nil)
	v := auth.NewHmacTokenValidator("different-secret-1234567890!", "fc", "fc")
	_, err := v.Validate(tok)
	require.Error(t, err)
}

func TestHmacValidatorRejectsWrongIssuer(t *testing.T) {
	tok := mintHS256(t, testSecret, "wrong", "fc", "prn_1", time.Now().Add(time.Hour), nil)
	v := auth.NewHmacTokenValidator(testSecret, "fc", "fc")
	_, err := v.Validate(tok)
	require.Error(t, err)
}

func TestHmacValidatorRejectsWrongAudience(t *testing.T) {
	tok := mintHS256(t, testSecret, "fc", "wrong-aud", "prn_1", time.Now().Add(time.Hour), nil)
	v := auth.NewHmacTokenValidator(testSecret, "fc", "fc")
	_, err := v.Validate(tok)
	require.Error(t, err)
}

func TestHmacValidatorRejectsExpiredTokenAsTokenExpired(t *testing.T) {
	tok := mintHS256(t, testSecret, "fc", "fc", "prn_1", time.Now().Add(-time.Minute), nil)
	v := auth.NewHmacTokenValidator(testSecret, "fc", "fc")
	_, err := v.Validate(tok)
	require.Error(t, err)
	assert.True(t, errors.Is(err, auth.ErrTokenExpired), "want ErrTokenExpired, got %v", err)
}

func TestHmacValidatorBearerStripsPrefix(t *testing.T) {
	tok := mintHS256(t, testSecret, "fc", "fc", "prn_1", time.Now().Add(time.Hour), nil)
	v := auth.NewHmacTokenValidator(testSecret, "fc", "fc")

	ctx, err := v.ValidateBearer("Bearer " + tok)
	require.NoError(t, err)
	assert.Equal(t, "prn_1", ctx.PrincipalID())

	_, err = v.ValidateBearer(tok)
	require.Error(t, err)
}
