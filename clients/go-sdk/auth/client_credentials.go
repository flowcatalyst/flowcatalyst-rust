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

// ClientCredentialsConfig configures a ClientCredentialsProvider.
type ClientCredentialsConfig struct {
	// IssuerURL is the FlowCatalyst OIDC server.
	IssuerURL string
	// ClientID is the service-account / confidential client id.
	ClientID string
	// ClientSecret is the matching secret.
	ClientSecret string
	// Scopes requested. May be empty: the token then carries every
	// permission the service account holds. A narrowed list used for the
	// router's in-flight checks (client.Router) must include
	// "platform:messaging:router:view".
	Scopes []string
	// Audience requested. May be empty.
	Audience string
	// SafetyWindow is how long before expiry to refresh proactively.
	// Defaults to 60s.
	SafetyWindow time.Duration
	// HTTPClient overrides the default transport. Optional.
	HTTPClient *http.Client
}

// ClientCredentialsProvider obtains and refreshes service-account
// access tokens via the OAuth2 client_credentials grant. Plug it into
// client.WithTokenProvider:
//
//	cc := auth.NewClientCredentialsProvider(...)
//	c := client.New(base, client.WithTokenProvider(cc.Token))
type ClientCredentialsProvider struct {
	cfg  ClientCredentialsConfig
	http *http.Client

	mu        sync.Mutex
	token     string
	expiresAt time.Time
	now       func() time.Time
}

// NewClientCredentialsProvider builds a provider with sensible defaults.
func NewClientCredentialsProvider(cfg ClientCredentialsConfig) *ClientCredentialsProvider {
	if cfg.SafetyWindow == 0 {
		cfg.SafetyWindow = 60 * time.Second
	}
	if cfg.HTTPClient == nil {
		cfg.HTTPClient = defaultHTTPClient()
	}
	cfg.IssuerURL = strings.TrimRight(cfg.IssuerURL, "/")
	return &ClientCredentialsProvider{
		cfg:  cfg,
		http: cfg.HTTPClient,
		now:  time.Now,
	}
}

// Token returns a valid access token, fetching a fresh one if the
// cached token has expired or is within the safety window. Safe for
// concurrent use.
func (p *ClientCredentialsProvider) Token(ctx context.Context) (string, error) {
	p.mu.Lock()
	defer p.mu.Unlock()

	if p.token != "" && p.now().Before(p.expiresAt.Add(-p.cfg.SafetyWindow)) {
		return p.token, nil
	}
	if err := p.refreshLocked(ctx); err != nil {
		return "", err
	}
	return p.token, nil
}

// Invalidate clears the cached token. The next Token call will fetch.
func (p *ClientCredentialsProvider) Invalidate() {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.token = ""
	p.expiresAt = time.Time{}
}

func (p *ClientCredentialsProvider) refreshLocked(ctx context.Context) error {
	form := url.Values{}
	form.Set("grant_type", "client_credentials")
	form.Set("client_id", p.cfg.ClientID)
	form.Set("client_secret", p.cfg.ClientSecret)
	if len(p.cfg.Scopes) > 0 {
		form.Set("scope", strings.Join(p.cfg.Scopes, " "))
	}
	if p.cfg.Audience != "" {
		form.Set("audience", p.cfg.Audience)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost,
		p.cfg.IssuerURL+"/oauth/token", strings.NewReader(form.Encode()))
	if err != nil {
		return newErr(KindTokenExchange, err.Error())
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("Accept", "application/json")
	resp, err := p.http.Do(req)
	if err != nil {
		return newErr(KindTokenExchange, err.Error())
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, _ := io.ReadAll(resp.Body)
		return newErr(KindTokenExchange,
			fmt.Sprintf("client_credentials grant failed (%d): %s", resp.StatusCode, body))
	}
	var out TokenResponse
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return newErr(KindTokenExchange, "parse token response: "+err.Error())
	}
	if out.AccessToken == "" {
		return newErr(KindTokenExchange, "token response missing access_token")
	}
	p.token = out.AccessToken
	if out.ExpiresIn > 0 {
		p.expiresAt = p.now().Add(time.Duration(out.ExpiresIn) * time.Second)
	} else {
		// Without exp info, treat the token as ~5 minutes long so we still refresh.
		p.expiresAt = p.now().Add(5 * time.Minute)
	}
	return nil
}
