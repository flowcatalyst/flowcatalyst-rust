<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth\Support;

use FlowCatalyst\Auth\Contracts\OidcUserHandler;
use FlowCatalyst\Auth\DTOs\FlowCatalystUser;
use FlowCatalyst\Auth\Http\Controllers\OidcAuthController;
use FlowCatalyst\Exceptions\AuthenticationException;
use GuzzleHttp\Client;
use GuzzleHttp\Exception\GuzzleException;

/**
 * Exchanges a stored refresh_token for a fresh token set and re-stores the
 * resulting principal via the bound OidcUserHandler.
 *
 * Shared by the manual "refresh my session" route
 * ({@see OidcAuthController::refresh()}) and {@see \FlowCatalyst\Auth\Http\Middleware\AuthenticateFc}'s
 * automatic mid-session refresh (when the stored access token has expired,
 * or — opt-in — when the platform reports the principal changed since the
 * token was minted), so the token-exchange logic lives in exactly one place.
 */
final class TokenRefresher
{
    private Client $httpClient;

    public function __construct(
        private readonly OidcUserHandler $userHandler,
        private readonly IdTokenValidator $idTokenValidator,
    ) {
        $this->httpClient = new Client(['timeout' => 30, 'http_errors' => false]);
    }

    /**
     * Exchange $current's refresh_token for a fresh token set, re-store the
     * resulting principal, and return it.
     *
     * @throws AuthenticationException on a missing refresh_token or a failed exchange.
     */
    public function refresh(FlowCatalystUser $current): FlowCatalystUser
    {
        $refreshToken = $current->refreshToken;
        if (empty($refreshToken)) {
            throw AuthenticationException::tokenFetchFailed("session can't be refreshed — no refresh token on file");
        }

        $config = OidcConfig::resolve();
        $tokens = $this->exchangeRefreshToken($config, $refreshToken);

        // exp/iat anchor for session capping / the opt-in revocation check —
        // always sourced from the access token, whichever branch below wins
        // authority.
        $accessTokenClaims = JwtDecoder::decodePayload($tokens['access_token'] ?? '') ?? [];

        // Roles/clients/applications live in the id_token, never a normal
        // client's access token (authority-free — see
        // TokenIssuer.identityAccessToken). The refresh grant only mints a
        // fresh id_token when the original login's scope included `openid`;
        // rebuild the principal from THAT (verified — never trust it
        // unverified, same rule as login) when present. If it is absent, or
        // fails verification, keep the PREVIOUS principal's claims rather
        // than overwriting authority with the access token's empties.
        $idTokenClaims = null;
        if (!empty($tokens['id_token'])) {
            $idTokenClaims = $this->idTokenValidator->validate($tokens['id_token']);
        }

        if ($idTokenClaims !== null) {
            $claims = $idTokenClaims;
            $claims['sub'] ??= $current->sub;
            $claims['email'] ??= $current->email;
            $claims['name'] ??= $current->name;
        } else {
            $claims = $current->claims;
        }

        $fcUser = FlowCatalystUser::fromClaims(
            claims: $claims,
            accessToken: $tokens['access_token'] ?? null,
            refreshToken: $tokens['refresh_token'] ?? $refreshToken,
            accessTokenClaims: $accessTokenClaims,
        )->withMechanism('session');

        // The raw id_token is kept purely as a logout hint (id_token_hint for
        // RP-Initiated Logout) regardless of whether it verified above — it
        // never grants authority on its own.
        if (!empty($tokens['id_token'])) {
            session()->put(OidcAuthController::ID_TOKEN_SESSION_KEY, $tokens['id_token']);
        }

        $this->userHandler->handleAuthenticatedUser($fcUser);

        return $fcUser;
    }

    /**
     * @return array{access_token: string, refresh_token?: string, id_token?: string}
     * @throws AuthenticationException
     */
    private function exchangeRefreshToken(array $config, string $refreshToken): array
    {
        $tokenUrl = rtrim($config['base_url'], '/') . '/oauth/token';

        $params = [
            'grant_type' => 'refresh_token',
            'refresh_token' => $refreshToken,
            'client_id' => $config['client_id'],
        ];
        if (!empty($config['client_secret'])) {
            $params['client_secret'] = $config['client_secret'];
        }

        try {
            $response = $this->httpClient->post($tokenUrl, [
                'form_params' => $params,
                'headers' => ['Accept' => 'application/json'],
            ]);

            $statusCode = $response->getStatusCode();
            $body = json_decode((string) $response->getBody(), true);

            if ($statusCode !== 200 || empty($body['access_token'])) {
                $error = $body['error_description'] ?? $body['error'] ?? 'Token refresh failed';
                throw AuthenticationException::tokenFetchFailed($error);
            }

            return $body;
        } catch (GuzzleException $e) {
            throw AuthenticationException::tokenFetchFailed($e->getMessage());
        }
    }
}
