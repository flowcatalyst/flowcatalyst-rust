<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth\Http\Controllers;

use FlowCatalyst\Auth\Contracts\OidcUserHandler;
use FlowCatalyst\Auth\DTOs\FlowCatalystUser;
use FlowCatalyst\Auth\Support\IdTokenValidator;
use FlowCatalyst\Auth\Support\JwtDecoder;
use FlowCatalyst\Auth\Support\OidcConfig;
use FlowCatalyst\Auth\Support\PkceGenerator;
use FlowCatalyst\Auth\Support\TokenRefresher;
use FlowCatalyst\Exceptions\AuthenticationException;
use GuzzleHttp\Client;
use GuzzleHttp\Exception\GuzzleException;
use Illuminate\Http\RedirectResponse;
use Illuminate\Http\Request;
use Illuminate\Routing\Controller;
use Illuminate\Support\Facades\Log;

/**
 * OIDC Authentication Controller.
 *
 * Handles the OAuth 2.0 Authorization Code flow with PKCE to authenticate
 * users against FlowCatalyst's OIDC server.
 *
 * Routes:
 * - GET /flowcatalyst/login  - Initiates OIDC login
 * - GET /flowcatalyst/callback - Handles callback from FlowCatalyst
 * - POST /flowcatalyst/logout - Logs out the user
 */
class OidcAuthController extends Controller
{
    private const STATE_SESSION_KEY = 'flowcatalyst_oidc_state';
    private const VERIFIER_SESSION_KEY = 'flowcatalyst_oidc_verifier';
    private const NONCE_SESSION_KEY = 'flowcatalyst_oidc_nonce';
    private const RETURN_URL_SESSION_KEY = 'flowcatalyst_oidc_return_url';

    /**
     * Session key holding the raw OIDC ID token, kept so logout can pass it as
     * id_token_hint for RP-Initiated Logout (single sign-out at the IdP).
     * Public: also written by {@see TokenRefresher} after a token refresh.
     */
    public const ID_TOKEN_SESSION_KEY = 'flowcatalyst_oidc_id_token';

    private Client $httpClient;

    public function __construct(
        private readonly OidcUserHandler $userHandler,
        private readonly TokenRefresher $tokenRefresher,
        private readonly IdTokenValidator $idTokenValidator,
    ) {
        $this->httpClient = new Client([
            'timeout' => 30,
            'http_errors' => false,
        ]);
    }

    /**
     * Initiate OIDC login flow.
     *
     * Redirects the user to FlowCatalyst's authorization endpoint.
     */
    public function login(Request $request): RedirectResponse
    {
        $config = OidcConfig::resolve();

        // Generate PKCE values
        $codeVerifier = PkceGenerator::generateCodeVerifier();
        $codeChallenge = PkceGenerator::generateCodeChallenge($codeVerifier);
        $state = PkceGenerator::generateState();
        $nonce = PkceGenerator::generateNonce();

        // Store in session for callback validation
        session()->put(self::STATE_SESSION_KEY, $state);
        session()->put(self::VERIFIER_SESSION_KEY, $codeVerifier);
        session()->put(self::NONCE_SESSION_KEY, $nonce);

        // Store the return URL if provided. `returnTo` is the current name
        // (what RequireSession/RequireAuth send when bouncing a guest);
        // `return_url` is kept as a deprecated alias so existing deep links
        // keep working. Validated later, right before the redirect
        // ({@see sanitizeReturnTo}) — never here, since this is only a
        // session write.
        $returnTo = $request->input('returnTo') ?? $request->input('return_url');
        if ($returnTo !== null) {
            session()->put(self::RETURN_URL_SESSION_KEY, $returnTo);
        }

        // Provider-direct login: route the user straight to a named upstream
        // IdP instead of the FlowCatalyst login page. Per-request ?provider=
        // wins over the configured default. Note the platform reuses a fresh
        // fc_session BEFORE honouring provider=, so pair with prompt=login
        // when a fresh handshake against the IdP is required. (Ignored in
        // portal mode — the portal plane routes IdPs server-side from the
        // user's email domain.)
        $provider = $request->input('provider') ?? config('flowcatalyst.oidc.provider');
        $prompt = $request->input('prompt');

        // Branded sign-in: name the FlowCatalyst client whose login theme the
        // sign-in pages should wear. Purely cosmetic — the platform falls back
        // to its own theme when this is absent or unrecognised, and it never
        // affects who may sign in or what they may do. Per-request ?client=
        // wins over the configured default, mirroring ?provider=.
        $client = $request->input('client') ?? config('flowcatalyst.oidc.client');

        // Build authorization URL
        $authUrl = $this->buildAuthorizationUrl($config, $state, $nonce, $codeChallenge, $provider, $prompt, $client);

        return redirect()->away($authUrl);
    }

    /**
     * Handle OIDC callback from FlowCatalyst.
     *
     * Exchanges the authorization code for tokens and calls the user handler.
     */
    public function callback(Request $request): RedirectResponse
    {
        // Check for errors from the authorization server
        if ($request->has('error')) {
            $error = $request->input('error');
            $description = $request->input('error_description', 'Authentication failed');
            Log::warning("OIDC callback error: {$error} - {$description}");
            return $this->errorRedirect($description);
        }

        // Validate required parameters
        $code = $request->input('code');
        $state = $request->input('state');

        if (empty($code)) {
            return $this->errorRedirect('No authorization code received');
        }

        // Validate state to prevent CSRF
        $expectedState = session()->pull(self::STATE_SESSION_KEY);
        if (empty($state) || $state !== $expectedState) {
            Log::warning('OIDC state mismatch', [
                'expected' => $expectedState,
                'received' => $state,
            ]);
            return $this->errorRedirect('Invalid state parameter. Please try again.');
        }

        // Get stored PKCE verifier and nonce
        $codeVerifier = session()->pull(self::VERIFIER_SESSION_KEY);
        $nonce = session()->pull(self::NONCE_SESSION_KEY);
        $returnUrl = session()->pull(self::RETURN_URL_SESSION_KEY);

        if (empty($codeVerifier)) {
            return $this->errorRedirect('Session expired. Please try again.');
        }

        try {
            $config = OidcConfig::resolve();

            // Exchange code for tokens
            $tokens = $this->exchangeCodeForTokens($config, $code, $codeVerifier);

            // Parse and validate ID token
            $claims = $this->parseIdToken($tokens['id_token']);

            // Validate nonce
            if (!empty($nonce) && ($claims['nonce'] ?? null) !== $nonce) {
                throw new AuthenticationException('Invalid nonce in ID token');
            }

            // The ID token's own `exp` is deliberately short-lived (~5 minutes) —
            // it's a login-moment identity proof, not a session-length signal.
            // Decode the access token too so session capping / the opt-in
            // revocation check have the access token's own exp/iat to anchor on
            // from the very first login, not just after the first refresh.
            $accessTokenClaims = JwtDecoder::decodePayload($tokens['access_token'] ?? '') ?? [];

            // Create FlowCatalystUser from claims
            $fcUser = FlowCatalystUser::fromClaims(
                $claims,
                $tokens['access_token'] ?? null,
                $tokens['refresh_token'] ?? null,
                $accessTokenClaims,
            );

            // Keep the raw ID token so logout() can send it as id_token_hint
            // for RP-Initiated Logout (ends the user's IdP session too).
            if (!empty($tokens['id_token'])) {
                session()->put(self::ID_TOKEN_SESSION_KEY, $tokens['id_token']);
            }

            // Call the user handler (this is where the app customizes login)
            $this->userHandler->handleAuthenticatedUser($fcUser);

            // Redirect to post-login URL. `returnTo`/`return_url` are
            // caller-supplied — validate before ever redirecting to them
            // (open-redirect guard); anything that isn't a same-origin
            // relative path falls back to the configured post-login redirect.
            $redirectUrl = $this->sanitizeReturnTo($returnUrl) ?? $this->userHandler->getPostLoginRedirect();
            return redirect()->to($redirectUrl);

        } catch (AuthenticationException $e) {
            Log::error('OIDC authentication failed', ['error' => $e->getMessage()]);
            return $this->errorRedirect($e->getMessage());
        } catch (\Exception $e) {
            Log::error('OIDC callback error', ['error' => $e->getMessage()]);
            return $this->errorRedirect('Authentication failed. Please try again.');
        }
    }

    /**
     * Handle logout.
     *
     * Calls the user handler's logout method and redirects.
     */
    public function logout(Request $request): RedirectResponse
    {
        // Capture the ID token before the handler tears the session down, so it
        // can be sent as id_token_hint for RP-Initiated Logout.
        $idToken = session(self::ID_TOKEN_SESSION_KEY);
        session()->forget(self::ID_TOKEN_SESSION_KEY);

        $this->userHandler->handleLogout();

        $postLogout = $this->userHandler->getPostLogoutRedirect();

        // RP-Initiated Logout: when enabled, bounce through the IdP's
        // end-session endpoint so the user is signed out of FlowCatalyst too —
        // not just locally. The post_logout_redirect_uri MUST be registered in
        // this client's postLogoutRedirectUris whitelist on the platform.
        if (config('flowcatalyst.oidc.single_logout', false)) {
            $base = rtrim((string) config('flowcatalyst.base_url'), '/');
            $absolute = (str_starts_with($postLogout, 'http://') || str_starts_with($postLogout, 'https://'))
                ? $postLogout
                : url($postLogout);

            $params = ['post_logout_redirect_uri' => $absolute];
            if (!empty($idToken)) {
                $params['id_token_hint'] = $idToken;                 // preferred
            } elseif ($clientId = config('flowcatalyst.oidc.client_id')) {
                $params['client_id'] = $clientId;                    // spec-sanctioned fallback
            }

            return redirect()->away($base . '/auth/oidc/session/end?' . http_build_query($params));
        }

        return redirect()->to($postLogout);
    }

    /**
     * Refresh the current session principal using the stored refresh token.
     *
     * With stateless auth, permissions live in the access token's `scope` claim,
     * so they go stale when roles/permissions change on the platform. This swaps
     * the stored access token for a fresh one (without a full re-login) and
     * re-stores the principal — picking up the new permissions on the next
     * request. Re-runs the bound OidcUserHandler, so a DB/Spatie-syncing handler
     * also re-syncs.
     */
    public function refresh(Request $request): RedirectResponse
    {
        $current = \FlowCatalyst\Auth\DefaultOidcUserHandler::getCurrentUser();
        if ($current === null) {
            return $this->errorRedirect('Your session can\'t be refreshed — please sign in again.');
        }

        try {
            $this->tokenRefresher->refresh($current);

            $requestedReturnTo = $request->input('returnTo') ?: $request->input('return_url');
            $returnUrl = $this->sanitizeReturnTo($requestedReturnTo)
                ?? (url()->previous() ?: $this->userHandler->getPostLoginRedirect());
            return redirect()->to($returnUrl)->with('status', 'Session refreshed.');
        } catch (\Throwable $e) {
            Log::error('OIDC refresh failed', ['error' => $e->getMessage()]);
            return $this->errorRedirect('Could not refresh your session. Please sign in again.');
        }
    }

    /**
     * Build the authorization URL for the OIDC server.
     */
    private function buildAuthorizationUrl(
        array $config,
        string $state,
        string $nonce,
        string $codeChallenge,
        ?string $provider = null,
        ?string $prompt = null,
        ?string $client = null,
    ): string {
        $baseUrl = rtrim($config['base_url'], '/');
        // Portal mode (flowcatalyst.oidc.portal): this app is a PORTAL for a
        // client's customers, so the flow enters through the portal identity
        // plane — a separate end-user population, no platform SSO reuse. The
        // token exchange and callback are identical; only the entry differs.
        // provider/prompt are meaningless there (the portal login page
        // routes IdPs from the email domain) and are omitted. So is client:
        // per-client login branding covers the platform sign-in pages only,
        // never the portal identity plane.
        $portal = (bool) config('flowcatalyst.oidc.portal', false);
        $authorizeUrl = $baseUrl . ($portal ? '/portal/authorize' : '/oauth/authorize');
        if ($portal) {
            $provider = null;
            $prompt = null;
            $client = null;
        }

        $params = [
            'response_type' => 'code',
            'client_id' => $config['client_id'],
            'redirect_uri' => $this->getCallbackUrl(),
            'scope' => $config['scope'] ?? 'openid profile email',
            'state' => $state,
            'nonce' => $nonce,
            'code_challenge' => $codeChallenge,
            'code_challenge_method' => 'S256',
        ];
        if (!empty($provider)) {
            $params['provider'] = $provider;
        }
        if (!empty($prompt)) {
            $params['prompt'] = $prompt;
        }
        if (!empty($client)) {
            $params['client'] = $client;
        }

        return $authorizeUrl . '?' . http_build_query($params);
    }

    /**
     * Exchange authorization code for tokens.
     *
     * @return array{access_token: string, id_token: string, refresh_token?: string}
     * @throws AuthenticationException
     */
    private function exchangeCodeForTokens(array $config, string $code, string $codeVerifier): array
    {
        $baseUrl = rtrim($config['base_url'], '/');
        $tokenUrl = $baseUrl . '/oauth/token';

        $params = [
            'grant_type' => 'authorization_code',
            'code' => $code,
            'redirect_uri' => $this->getCallbackUrl(),
            'client_id' => $config['client_id'],
            'code_verifier' => $codeVerifier,
        ];

        // Add client secret if configured (for confidential clients)
        if (!empty($config['client_secret'])) {
            $params['client_secret'] = $config['client_secret'];
        }

        try {
            $response = $this->httpClient->post($tokenUrl, [
                'form_params' => $params,
                'headers' => [
                    'Accept' => 'application/json',
                ],
            ]);

            $statusCode = $response->getStatusCode();
            $body = json_decode((string) $response->getBody(), true);

            if ($statusCode !== 200) {
                $error = $body['error_description'] ?? $body['error'] ?? 'Token exchange failed';
                throw AuthenticationException::tokenFetchFailed($error);
            }

            if (empty($body['id_token'])) {
                throw AuthenticationException::tokenFetchFailed('No ID token received');
            }

            return $body;

        } catch (GuzzleException $e) {
            throw AuthenticationException::tokenFetchFailed($e->getMessage());
        }
    }

    /**
     * Verify and decode an ID token (JWT).
     *
     * Verified the same way the access token is ({@see IdTokenValidator}):
     * RS256 signature against JWKS, `iss`, `aud`, `exp`, `nbf`. A failure
     * here is a login failure — never a silent fallback to the token's
     * unverified claims, since the whole principal (roles included) is
     * built from this.
     *
     * @return array<string, mixed>
     * @throws AuthenticationException
     */
    private function parseIdToken(string $idToken): array
    {
        $payload = $this->idTokenValidator->validate($idToken);
        if ($payload === null) {
            throw new AuthenticationException('Invalid ID token');
        }

        if (empty($payload['email']) && empty($payload['preferred_username'])) {
            throw new AuthenticationException('Missing email claim in ID token');
        }

        return $payload;
    }

    /**
     * Validate a caller-supplied return URL (the `returnTo`/`return_url`
     * request param) before ever redirecting to it. Only a same-origin
     * relative path is accepted — a single leading `/`, never `//host`
     * (protocol-relative), any scheme, or any absolute URL — mirroring the
     * TypeScript SDK's `sanitizeReturnTo`
     * (`typescript-sdk/src/fastify/plugin.ts`). Anything else returns null
     * so the caller falls back to its own configured redirect.
     */
    private function sanitizeReturnTo(?string $raw): ?string
    {
        if ($raw === null || $raw === '') {
            return null;
        }
        $decoded = rawurldecode($raw);
        if ($decoded === '' || !str_starts_with($decoded, '/') || str_starts_with($decoded, '//')) {
            return null;
        }
        return $decoded;
    }

    /**
     * Get the callback URL for this application.
     */
    private function getCallbackUrl(): string
    {
        return url(config('flowcatalyst.oidc.callback_route', '/flowcatalyst/callback'));
    }

    /**
     * Redirect to error page with message.
     */
    private function errorRedirect(string $message): RedirectResponse
    {
        $errorRoute = config('flowcatalyst.oidc.error_redirect', '/');
        return redirect()->to($errorRoute)->with('error', $message);
    }
}
