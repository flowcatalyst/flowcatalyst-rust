package auth

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"
)

// OAuthConfig configures an OAuthClient for the authorization-code flow.
type OAuthConfig struct {
	// IssuerURL is the FlowCatalyst OIDC server base URL.
	IssuerURL string
	// ClientID is the registered OAuth client id.
	ClientID string
	// ClientSecret is the OAuth client secret (omit for public clients).
	ClientSecret string
	// RedirectURI is the application's callback URL.
	RedirectURI string
	// Scopes requested. Defaults to ["openid", "profile", "email"].
	Scopes []string
	// HTTPClient overrides the default transport. Optional.
	HTTPClient *http.Client
}

// OAuthClient wraps the OAuth2 authorization-code flow + the OIDC
// session-end (RP-initiated logout) endpoint.
type OAuthClient struct {
	cfg  OAuthConfig
	http *http.Client

	// Single-flight refresh: one exchange per refresh token, see RefreshToken.
	refreshMu sync.Mutex
	refreshes map[string]*refreshCall
}

// refreshMemo is how long a completed refresh is reused for a caller that
// read the old refresh token just after the exchange finished.
const refreshMemo = 10 * time.Second

// refreshCall is one refresh exchange, shared by every caller presenting the
// same refresh token while it runs and for refreshMemo after it succeeds.
type refreshCall struct {
	done    chan struct{}
	res     *TokenResponse
	err     error
	settled time.Time
}

// NewOAuthClient builds an OAuthClient. Scopes default to openid/profile/email.
func NewOAuthClient(cfg OAuthConfig) *OAuthClient {
	if len(cfg.Scopes) == 0 {
		cfg.Scopes = []string{"openid", "profile", "email"}
	}
	http := cfg.HTTPClient
	if http == nil {
		http = defaultHTTPClient()
	}
	cfg.IssuerURL = strings.TrimRight(cfg.IssuerURL, "/")
	return &OAuthClient{cfg: cfg, http: http, refreshes: map[string]*refreshCall{}}
}

// AuthorizeParams is the session-stored side of an authorize-URL call.
type AuthorizeParams struct {
	PKCE  PkceChallenge
	State string
	Nonce string
}

// AuthorizeURL builds the URL to redirect users to for login, plus the
// session-stored verifier/state/nonce to validate the callback.
func (c *OAuthClient) AuthorizeURL() (string, AuthorizeParams) {
	pkce := NewPkceChallenge()
	state := randomURLSafe(32)
	nonce := randomURLSafe(32)

	q := url.Values{}
	q.Set("response_type", "code")
	q.Set("client_id", c.cfg.ClientID)
	q.Set("redirect_uri", c.cfg.RedirectURI)
	q.Set("scope", strings.Join(c.cfg.Scopes, " "))
	q.Set("state", state)
	q.Set("nonce", nonce)
	q.Set("code_challenge", pkce.CodeChallenge)
	q.Set("code_challenge_method", pkce.CodeChallengeMethod)

	return c.cfg.IssuerURL + "/oauth/authorize?" + q.Encode(),
		AuthorizeParams{PKCE: pkce, State: state, Nonce: nonce}
}

// TokenResponse is the body of /oauth/token.
type TokenResponse struct {
	AccessToken  string `json:"access_token"`
	TokenType    string `json:"token_type"`
	ExpiresIn    int64  `json:"expires_in"`
	RefreshToken string `json:"refresh_token,omitempty"`
	IDToken      string `json:"id_token,omitempty"`
	Scope        string `json:"scope,omitempty"`
}

// ExchangeCode exchanges an authorization code for tokens. Call this
// from your callback handler after validating state and PKCE.
func (c *OAuthClient) ExchangeCode(ctx context.Context, code, codeVerifier string) (*TokenResponse, error) {
	form := url.Values{}
	form.Set("grant_type", "authorization_code")
	form.Set("code", code)
	form.Set("redirect_uri", c.cfg.RedirectURI)
	form.Set("client_id", c.cfg.ClientID)
	form.Set("code_verifier", codeVerifier)
	if c.cfg.ClientSecret != "" {
		form.Set("client_secret", c.cfg.ClientSecret)
	}
	return c.postToken(ctx, form)
}

// RefreshToken exchanges a refresh token for a fresh access token.
//
// Single-flight (owner ruling 5 of 2026-09-25). The platform rotates refresh
// tokens and revokes the whole family when a rotated-out token is presented
// again (beyond a 10 s leeway kept for exactly this race), so concurrent
// requests of one session that each see the access token expire must not
// each spend it. Callers presenting the same refresh token join one in-flight
// exchange (a joiner stops waiting when its own ctx ends), and a successful
// result is reused for 10 s by a caller that read the old token just after.
// A failure is shared with the callers that joined it, never remembered. Per
// OAuthClient (share one per process): instances behind a load balancer
// still rely on the platform's leeway.
func (c *OAuthClient) RefreshToken(ctx context.Context, refreshToken string) (*TokenResponse, error) {
	c.refreshMu.Lock()
	if c.refreshes == nil { // an OAuthClient not built by NewOAuthClient
		c.refreshes = map[string]*refreshCall{}
	}
	now := time.Now()
	for key, call := range c.refreshes {
		if !call.settled.IsZero() && now.Sub(call.settled) >= refreshMemo {
			delete(c.refreshes, key)
		}
	}
	if call, ok := c.refreshes[refreshToken]; ok {
		c.refreshMu.Unlock()
		select {
		case <-call.done:
		case <-ctx.Done():
			return nil, newErr(KindTokenExchange, ctx.Err().Error())
		}
		if call.err != nil {
			return nil, call.err
		}
		res := *call.res
		return &res, nil
	}
	call := &refreshCall{done: make(chan struct{})}
	c.refreshes[refreshToken] = call
	c.refreshMu.Unlock()

	res, err := c.exchangeRefreshToken(ctx, refreshToken)

	c.refreshMu.Lock()
	call.res, call.err = res, err
	if err != nil {
		delete(c.refreshes, refreshToken)
	} else {
		call.settled = time.Now()
	}
	close(call.done)
	c.refreshMu.Unlock()
	if err != nil {
		return nil, err
	}
	out := *res
	return &out, nil
}

// exchangeRefreshToken is one refresh_token grant against /oauth/token.
func (c *OAuthClient) exchangeRefreshToken(ctx context.Context, refreshToken string) (*TokenResponse, error) {
	form := url.Values{}
	form.Set("grant_type", "refresh_token")
	form.Set("refresh_token", refreshToken)
	form.Set("client_id", c.cfg.ClientID)
	if c.cfg.ClientSecret != "" {
		form.Set("client_secret", c.cfg.ClientSecret)
	}
	return c.postToken(ctx, form)
}

// RevokeToken revokes an access or refresh token per RFC 7009. A 200
// response is required; anything else returns an Error.
func (c *OAuthClient) RevokeToken(ctx context.Context, token string) error {
	form := url.Values{}
	form.Set("token", token)
	form.Set("client_id", c.cfg.ClientID)
	if c.cfg.ClientSecret != "" {
		form.Set("client_secret", c.cfg.ClientSecret)
	}
	resp, err := c.postForm(ctx, c.cfg.IssuerURL+"/oauth/revoke", form)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, _ := io.ReadAll(resp.Body)
		return newErr(KindTokenExchange, fmt.Sprintf("revoke failed (%d): %s", resp.StatusCode, body))
	}
	_, _ = io.Copy(io.Discard, resp.Body)
	return nil
}

// IntrospectionResponse is the body of /oauth/introspect (RFC 7662).
type IntrospectionResponse struct {
	Active    bool   `json:"active"`
	Scope     string `json:"scope,omitempty"`
	ClientID  string `json:"client_id,omitempty"`
	Username  string `json:"username,omitempty"`
	TokenType string `json:"token_type,omitempty"`
	Exp       int64  `json:"exp,omitempty"`
	Iat       int64  `json:"iat,omitempty"`
	Sub       string `json:"sub,omitempty"`
}

// IntrospectToken posts to /oauth/introspect and returns the response.
func (c *OAuthClient) IntrospectToken(ctx context.Context, token string) (*IntrospectionResponse, error) {
	form := url.Values{}
	form.Set("token", token)
	form.Set("client_id", c.cfg.ClientID)
	if c.cfg.ClientSecret != "" {
		form.Set("client_secret", c.cfg.ClientSecret)
	}
	resp, err := c.postForm(ctx, c.cfg.IssuerURL+"/oauth/introspect", form)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, _ := io.ReadAll(resp.Body)
		return nil, newErr(KindTokenExchange, fmt.Sprintf("introspect failed (%d): %s", resp.StatusCode, body))
	}
	var out IntrospectionResponse
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return nil, newErr(KindTokenExchange, "parse introspect response: "+err.Error())
	}
	return &out, nil
}

// UserInfoResponse is the body of /oauth/userinfo. Extra carries any
// custom claims the platform adds.
type UserInfoResponse struct {
	Sub           string                 `json:"sub"`
	Name          string                 `json:"name,omitempty"`
	Email         string                 `json:"email,omitempty"`
	EmailVerified *bool                  `json:"email_verified,omitempty"`
	Extra         map[string]any         `json:"-"`
}

// UnmarshalJSON keeps standard fields typed and bucketing extras.
func (u *UserInfoResponse) UnmarshalJSON(b []byte) error {
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(b, &raw); err != nil {
		return err
	}
	get := func(k string, dst any) error {
		v, ok := raw[k]
		if !ok {
			return nil
		}
		delete(raw, k)
		return json.Unmarshal(v, dst)
	}
	if err := get("sub", &u.Sub); err != nil {
		return err
	}
	if err := get("name", &u.Name); err != nil {
		return err
	}
	if err := get("email", &u.Email); err != nil {
		return err
	}
	if err := get("email_verified", &u.EmailVerified); err != nil {
		return err
	}
	if len(raw) > 0 {
		u.Extra = make(map[string]any, len(raw))
		for k, v := range raw {
			var any any
			if err := json.Unmarshal(v, &any); err != nil {
				return err
			}
			u.Extra[k] = any
		}
	}
	return nil
}

// UserInfo fetches /oauth/userinfo using the access token.
func (c *OAuthClient) UserInfo(ctx context.Context, accessToken string) (*UserInfoResponse, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.cfg.IssuerURL+"/oauth/userinfo", nil)
	if err != nil {
		return nil, newErr(KindTokenExchange, err.Error())
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	req.Header.Set("Accept", "application/json")
	resp, err := c.http.Do(req)
	if err != nil {
		return nil, newErr(KindTokenExchange, err.Error())
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, _ := io.ReadAll(resp.Body)
		return nil, newErr(KindTokenExchange, fmt.Sprintf("userinfo failed (%d): %s", resp.StatusCode, body))
	}
	var out UserInfoResponse
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return nil, newErr(KindTokenExchange, "parse userinfo response: "+err.Error())
	}
	return &out, nil
}

// LogoutURL builds the RP-Initiated Logout URL.
//
// When postLogoutRedirectURI is non-empty, idTokenHint must also be set
// — FlowCatalyst uses the hint's aud claim to verify the redirect URI
// against the client's registered postLogoutRedirectUris (OIDC RP-
// Initiated Logout 1.0 §2). Omitting the hint causes the OP to refuse
// the redirect.
func (c *OAuthClient) LogoutURL(postLogoutRedirectURI, idTokenHint, state string) string {
	base := c.cfg.IssuerURL + "/auth/oidc/session/end"
	q := url.Values{}
	if postLogoutRedirectURI != "" {
		q.Set("post_logout_redirect_uri", postLogoutRedirectURI)
	}
	if idTokenHint != "" {
		q.Set("id_token_hint", idTokenHint)
	}
	if state != "" {
		q.Set("state", state)
	}
	if len(q) == 0 {
		return base
	}
	return base + "?" + q.Encode()
}

// postToken posts a form to /oauth/token and decodes the TokenResponse.
func (c *OAuthClient) postToken(ctx context.Context, form url.Values) (*TokenResponse, error) {
	resp, err := c.postForm(ctx, c.cfg.IssuerURL+"/oauth/token", form)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, _ := io.ReadAll(resp.Body)
		return nil, newErr(KindTokenExchange, fmt.Sprintf("token exchange failed (%d): %s", resp.StatusCode, body))
	}
	var out TokenResponse
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return nil, newErr(KindTokenExchange, "parse token response: "+err.Error())
	}
	return &out, nil
}

func (c *OAuthClient) postForm(ctx context.Context, fullURL string, form url.Values) (*http.Response, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, fullURL, strings.NewReader(form.Encode()))
	if err != nil {
		return nil, newErr(KindTokenExchange, err.Error())
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("Accept", "application/json")
	resp, err := c.http.Do(req)
	if err != nil {
		return nil, newErr(KindTokenExchange, err.Error())
	}
	return resp, nil
}

func defaultHTTPClient() *http.Client {
	return &http.Client{Timeout: 15 * time.Second}
}
