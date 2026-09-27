package auth_test

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/lestrrat-go/jwx/v2/jwa"
	"github.com/lestrrat-go/jwx/v2/jwk"
	"github.com/lestrrat-go/jwx/v2/jws"
	"github.com/lestrrat-go/jwx/v2/jwt"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/auth"
)

type oidcFixture struct {
	server    *httptest.Server
	signKey   jwk.Key   // private key for signing
	pubKey    jwk.Key   // public key embedded in JWKS
	issuerURL string
}

func newOIDCFixture(t *testing.T) *oidcFixture {
	t.Helper()
	raw, err := rsa.GenerateKey(rand.Reader, 2048)
	require.NoError(t, err)

	priv, err := jwk.FromRaw(raw)
	require.NoError(t, err)
	require.NoError(t, priv.Set(jwk.KeyIDKey, "test-key"))
	require.NoError(t, priv.Set(jwk.AlgorithmKey, jwa.RS256))

	pub, err := priv.PublicKey()
	require.NoError(t, err)
	require.NoError(t, pub.Set(jwk.KeyUsageKey, "sig"))

	set := jwk.NewSet()
	require.NoError(t, set.AddKey(pub))

	mux := http.NewServeMux()
	var issuerURL string
	mux.HandleFunc("/.well-known/openid-configuration", func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{
			"issuer":   issuerURL,
			"jwks_uri": issuerURL + "/jwks",
		})
	})
	mux.HandleFunc("/jwks", func(w http.ResponseWriter, _ *http.Request) {
		raw, err := json.Marshal(set)
		require.NoError(t, err)
		_, _ = w.Write(raw)
	})
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	issuerURL = srv.URL

	return &oidcFixture{server: srv, signKey: priv, pubKey: pub, issuerURL: issuerURL}
}

func (f *oidcFixture) mint(t *testing.T, iss, aud, sub string, exp time.Time, extras map[string]any) string {
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
	signed, err := jwt.Sign(tok, jwt.WithKey(jwa.RS256, f.signKey, jws.WithProtectedHeaders(jws.NewHeaders())))
	require.NoError(t, err)
	return string(signed)
}

func TestTokenValidatorAcceptsRS256SignedToken(t *testing.T) {
	f := newOIDCFixture(t)
	v := auth.NewTokenValidator(auth.TokenValidatorConfig{
		IssuerURL: f.issuerURL,
		Audience:  "my-app",
	})

	tok := f.mint(t, f.issuerURL, "my-app", "prn_42", time.Now().Add(time.Hour), nil)

	ctx, err := v.Validate(context.Background(), tok)
	require.NoError(t, err)
	assert.Equal(t, "prn_42", ctx.PrincipalID())
	assert.True(t, ctx.IsAnchor())
	assert.True(t, ctx.HasRole("admin"))
	assert.Equal(t, tok, ctx.BearerToken())
}

func TestTokenValidatorRejectsWrongIssuer(t *testing.T) {
	f := newOIDCFixture(t)
	v := auth.NewTokenValidator(auth.TokenValidatorConfig{
		IssuerURL: f.issuerURL,
		Audience:  "my-app",
	})
	tok := f.mint(t, "https://different-issuer.example.com", "my-app", "prn_1",
		time.Now().Add(time.Hour), nil)
	_, err := v.Validate(context.Background(), tok)
	require.Error(t, err)
}

func TestTokenValidatorRejectsWrongAudience(t *testing.T) {
	f := newOIDCFixture(t)
	v := auth.NewTokenValidator(auth.TokenValidatorConfig{
		IssuerURL: f.issuerURL,
		Audience:  "my-app",
	})
	tok := f.mint(t, f.issuerURL, "other-app", "prn_1", time.Now().Add(time.Hour), nil)
	_, err := v.Validate(context.Background(), tok)
	require.Error(t, err)
}

func TestTokenValidatorRejectsExpiredTokenAsTokenExpired(t *testing.T) {
	f := newOIDCFixture(t)
	v := auth.NewTokenValidator(auth.TokenValidatorConfig{
		IssuerURL: f.issuerURL,
		Audience:  "my-app",
	})
	tok := f.mint(t, f.issuerURL, "my-app", "prn_1", time.Now().Add(-time.Minute), nil)
	_, err := v.Validate(context.Background(), tok)
	require.Error(t, err)
	assert.True(t, errors.Is(err, auth.ErrTokenExpired))
}

func TestTokenValidatorRejectsTamperedSignature(t *testing.T) {
	f := newOIDCFixture(t)
	v := auth.NewTokenValidator(auth.TokenValidatorConfig{
		IssuerURL: f.issuerURL,
		Audience:  "my-app",
	})
	tok := f.mint(t, f.issuerURL, "my-app", "prn_1", time.Now().Add(time.Hour), nil)
	// Mutate a byte in the middle of the signature segment so we don't
	// hit base64url last-byte padding bits (where some flips decode to
	// the same bytes).
	dot := strings.LastIndex(tok, ".")
	require.Greater(t, dot, 0)
	sigStart := dot + 1
	require.Less(t, sigStart+5, len(tok))
	flipAt := sigStart + 5
	swap := byte('A')
	if tok[flipAt] == 'A' {
		swap = 'B'
	}
	tampered := tok[:flipAt] + string(swap) + tok[flipAt+1:]
	_, err := v.Validate(context.Background(), tampered)
	require.Error(t, err)
}

func TestTokenValidatorBearerHelper(t *testing.T) {
	f := newOIDCFixture(t)
	v := auth.NewTokenValidator(auth.TokenValidatorConfig{
		IssuerURL: f.issuerURL,
		Audience:  "my-app",
	})
	tok := f.mint(t, f.issuerURL, "my-app", "prn_1", time.Now().Add(time.Hour), nil)

	ctx, err := v.ValidateBearer(context.Background(), "Bearer "+tok)
	require.NoError(t, err)
	assert.Equal(t, "prn_1", ctx.PrincipalID())

	_, err = v.ValidateBearer(context.Background(), tok)
	require.Error(t, err)
}

func TestTokenValidatorSurfacesDiscoveryFailure(t *testing.T) {
	// No /.well-known handler — discovery should fail with KindDiscovery.
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.NotFound(w, nil)
	}))
	defer srv.Close()
	v := auth.NewTokenValidator(auth.TokenValidatorConfig{
		IssuerURL: srv.URL,
		Audience:  "my-app",
	})
	_, err := v.Validate(context.Background(), "any.token.here")
	require.Error(t, err)
}
