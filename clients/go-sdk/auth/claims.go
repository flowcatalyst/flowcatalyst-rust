package auth

import (
	"encoding/json"
	"fmt"
	"strings"
)

// Audience is a JWT aud claim. RFC 7519 allows aud to be either a
// string or an array of strings. FlowCatalyst-minted tokens emit a
// single string (matching the Rust SDK), but tokens minted with other
// tooling (e.g. jwx itself) emit an array. We accept both forms on
// unmarshal and always marshal as a single string for wire parity
// with Rust.
type Audience string

// UnmarshalJSON accepts either a string or a one-element string array.
// Multi-element arrays take the first entry, matching how the Rust SDK
// would behave if aud were typed as String.
func (a *Audience) UnmarshalJSON(b []byte) error {
	var s string
	if err := json.Unmarshal(b, &s); err == nil {
		*a = Audience(s)
		return nil
	}
	var arr []string
	if err := json.Unmarshal(b, &arr); err == nil {
		if len(arr) == 0 {
			*a = ""
		} else {
			*a = Audience(arr[0])
		}
		return nil
	}
	return fmt.Errorf("aud claim must be a string or array of strings, got %s", b)
}

func (a Audience) String() string { return string(a) }

// AccessTokenClaims is the JWT payload shape issued by FlowCatalyst's
// /oauth/token endpoint. JSON tags match the Rust SDK byte-for-byte so
// the same token deserialises identically across SDKs.
//
// Claim shape (the Go platform's, which the Rust platform issues too):
// "tier" is the tenancy tier; "scope" is the granted permissions as a
// space-delimited string (absent when there are none); "token_use" is
// "api" or "identity"; "clients" holds "*" or "{id}:{identifier}" entries;
// "applications" holds "*" or "{id}:{code}" entries, with
// "all_applications" alongside. Bare ids (the older form) are still
// accepted, and a token that predates "tier" (tier carried in "scope")
// still reads its tier correctly — see TenancyTier.
type AccessTokenClaims struct {
	// Sub is the principal id, e.g. "prn_0HZXEQ5Y8JY5Z".
	Sub string `json:"sub"`
	// Iss is the issuer URL.
	Iss string `json:"iss"`
	// Aud is the audience. Accepts string or []string on the wire.
	Aud Audience `json:"aud"`
	// Exp is the expiration time (Unix seconds).
	Exp int64 `json:"exp"`
	// Iat is the issued-at time (Unix seconds).
	Iat int64 `json:"iat"`
	// Nbf is the not-before time (Unix seconds).
	Nbf int64 `json:"nbf"`
	// Jti is the JWT id.
	Jti string `json:"jti"`
	// PrincipalType is "USER" or "SERVICE". The wire field name is
	// "type" to match the Rust SDK's serde rename.
	PrincipalType string `json:"type"`
	// Tier is the tenancy tier: "ANCHOR", "PARTNER", or "CLIENT". Empty on
	// a token that predates the claim; use TenancyTier, which falls back to
	// the legacy scope-as-tier form.
	Tier string `json:"tier,omitempty"`
	// Scope is the granted permissions, space-delimited (the OAuth "scope"
	// claim); empty when the token carries none. See GrantedPermissions.
	// Tokens minted before "tier" existed carried the tenancy tier here.
	Scope string `json:"scope,omitempty"`
	// Email is present for USER principals, absent for SERVICE.
	Email string `json:"email,omitempty"`
	// Name is the display name.
	Name string `json:"name"`
	// Clients are the clients this principal can access:
	// "{id}:{identifier}" entries (bare ids on older tokens), or ["*"] for
	// anchor users.
	Clients []string `json:"clients"`
	// Roles are the role codes assigned to this principal.
	Roles []string `json:"roles,omitempty"`
	// Applications are the applications this principal can access:
	// "{id}:{code}" entries (bare ids on older tokens), or ["*"] for every
	// application.
	Applications []string `json:"applications,omitempty"`
	// AllApplications grants access to every application, present and
	// future; when true Applications is not a restriction.
	AllApplications bool `json:"all_applications,omitempty"`
	// TokenUse is the access-token class: "api" (carries authority, valid
	// as a platform API bearer) or "identity" (interactive login; no
	// roles, clients, applications or scope). Empty on older tokens.
	TokenUse string `json:"token_use,omitempty"`
}

var tiers = map[string]bool{"ANCHOR": true, "PARTNER": true, "CLIENT": true}

// entryID is the id part of a "{id}:{label}" claim entry (the whole entry
// when it has no label).
func entryID(entry string) string {
	id, _, _ := strings.Cut(entry, ":")
	return id
}

// TenancyTier returns the "tier" claim or, on a token that predates it, a
// tier value carried in "scope". Empty when neither is present.
func (c *AccessTokenClaims) TenancyTier() string {
	if c.Tier != "" {
		return c.Tier
	}
	if tiers[c.Scope] {
		return c.Scope
	}
	return ""
}

// GrantedPermissions splits the space-delimited "scope" claim. Empty when
// the token carries none, and for a legacy token whose scope held the tier.
func (c *AccessTokenClaims) GrantedPermissions() []string {
	if c.Tier == "" && tiers[c.Scope] {
		return nil
	}
	return strings.Fields(c.Scope)
}

// ClientIDList returns the accessible client ids ("*" for all), dropping
// the identifier part of each "{id}:{identifier}" entry.
func (c *AccessTokenClaims) ClientIDList() []string {
	out := make([]string, 0, len(c.Clients))
	for _, e := range c.Clients {
		out = append(out, entryID(e))
	}
	return out
}

// HasAllApplications reports whether the principal reaches every
// application: the "*" entry or the all_applications claim.
func (c *AccessTokenClaims) HasAllApplications() bool {
	if c.AllApplications {
		return true
	}
	for _, a := range c.Applications {
		if a == "*" {
			return true
		}
	}
	return false
}

// ApplicationIDs returns the accessible application ids, dropping the code
// part of each "{id}:{code}" entry. Empty when HasAllApplications is true.
func (c *AccessTokenClaims) ApplicationIDs() []string {
	if c.HasAllApplications() {
		return nil
	}
	out := make([]string, 0, len(c.Applications))
	for _, a := range c.Applications {
		out = append(out, entryID(a))
	}
	return out
}

// HasApplicationAccess reports whether the principal can access the given
// application (by id).
func (c *AccessTokenClaims) HasApplicationAccess(applicationID string) bool {
	if c.HasAllApplications() {
		return true
	}
	for _, a := range c.Applications {
		if a == applicationID || entryID(a) == applicationID {
			return true
		}
	}
	return false
}

// IsIdentityToken reports whether this is an identity-only access token
// (token_use "identity"), which carries no authority.
func (c *AccessTokenClaims) IsIdentityToken() bool { return c.TokenUse == "identity" }

// HasClientAccess reports whether the principal can access the given
// client (by id). Anchor principals (clients == ["*"]) always return true.
func (c *AccessTokenClaims) HasClientAccess(clientID string) bool {
	for _, e := range c.Clients {
		if e == "*" || e == clientID || entryID(e) == clientID {
			return true
		}
	}
	return false
}

// HasRole reports whether the principal has the given role code.
func (c *AccessTokenClaims) HasRole(role string) bool {
	for _, r := range c.Roles {
		if r == role {
			return true
		}
	}
	return false
}

// IsAnchor reports whether this is an anchor (platform-wide) principal:
// the ANCHOR tier, or a "*" entry in clients.
func (c *AccessTokenClaims) IsAnchor() bool {
	if c.TenancyTier() == "ANCHOR" {
		return true
	}
	for _, e := range c.Clients {
		if e == "*" {
			return true
		}
	}
	return false
}

// IsService reports whether this is a service account.
func (c *AccessTokenClaims) IsService() bool { return c.PrincipalType == "SERVICE" }

// PrincipalID returns the Sub claim.
func (c *AccessTokenClaims) PrincipalID() string { return c.Sub }

// AuthContext is a validated token plus the raw JWT, ready for
// authorization checks and forwarding to downstream services.
type AuthContext struct {
	Claims AccessTokenClaims
	Token  string
}

// NewAuthContext constructs an AuthContext.
func NewAuthContext(claims AccessTokenClaims, token string) *AuthContext {
	return &AuthContext{Claims: claims, Token: token}
}

func (a *AuthContext) PrincipalID() string                  { return a.Claims.Sub }
func (a *AuthContext) Email() string                        { return a.Claims.Email }
func (a *AuthContext) Name() string                         { return a.Claims.Name }
func (a *AuthContext) IsAnchor() bool                       { return a.Claims.IsAnchor() }
func (a *AuthContext) IsService() bool                      { return a.Claims.IsService() }
func (a *AuthContext) HasClientAccess(clientID string) bool { return a.Claims.HasClientAccess(clientID) }
func (a *AuthContext) HasRole(role string) bool             { return a.Claims.HasRole(role) }
func (a *AuthContext) ClientIDs() []string                  { return a.Claims.Clients }
func (a *AuthContext) Roles() []string                      { return a.Claims.Roles }

// BearerToken returns the raw JWT for forwarding to downstream services.
func (a *AuthContext) BearerToken() string { return a.Token }
