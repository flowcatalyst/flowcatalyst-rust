<?php

declare(strict_types=1);

/**
 * Router script for the PHP built-in web server (`php -S`), driven by
 * {@see \FlowCatalyst\Tests\Support\MockIssuer}. Speaks just enough of the
 * OIDC dance the SDK's OidcAuthController / TokenRefresher actually make
 * real HTTP calls for:
 *
 *   GET  /.well-known/openid-configuration
 *   GET  /.well-known/jwks.json
 *   POST /oauth/token   (authorization_code and refresh_token grants)
 *
 * The built-in server invokes this script fresh for every request (no
 * persistent process state), so everything it needs is either static
 * per-run config (env vars set by MockIssuer before starting the server) or
 * read from a shared JSON state file MockIssuer mutates from the test
 * process. Signing happens in the TEST process (which holds the private
 * key); this router only ever echoes back pre-signed token responses it
 * finds in the state file, keyed by the authorization code or refresh token
 * presented.
 */

$issuer = (string) getenv('FC_MOCK_ISSUER');
$jwksFile = (string) getenv('FC_MOCK_JWKS_FILE');
$stateFile = (string) getenv('FC_MOCK_STATE_FILE');

$path = parse_url($_SERVER['REQUEST_URI'] ?? '/', PHP_URL_PATH);
header('Content-Type: application/json');

function fc_mock_read_state(string $stateFile): array
{
    $raw = @file_get_contents($stateFile);
    $decoded = $raw === false ? null : json_decode($raw, true);
    return is_array($decoded) ? $decoded : ['codes' => [], 'refresh' => []];
}

if ($path === '/.well-known/openid-configuration') {
    echo json_encode([
        'issuer' => $issuer,
        'authorization_endpoint' => $issuer . '/oauth/authorize',
        'token_endpoint' => $issuer . '/oauth/token',
        'jwks_uri' => $issuer . '/.well-known/jwks.json',
        'response_types_supported' => ['code'],
        'grant_types_supported' => ['authorization_code', 'refresh_token'],
        'id_token_signing_alg_values_supported' => ['RS256'],
    ]);
    return;
}

if ($path === '/.well-known/jwks.json') {
    $raw = @file_get_contents($jwksFile);
    echo $raw === false ? json_encode(['keys' => []]) : $raw;
    return;
}

if ($path === '/oauth/token' && ($_SERVER['REQUEST_METHOD'] ?? '') === 'POST') {
    $body = file_get_contents('php://input') ?: '';
    parse_str($body, $params);
    $grant = $params['grant_type'] ?? null;
    $state = fc_mock_read_state($stateFile);

    $response = null;
    if ($grant === 'authorization_code') {
        $code = (string) ($params['code'] ?? '');
        $response = $state['codes'][$code] ?? null;
    } elseif ($grant === 'refresh_token') {
        $refreshToken = (string) ($params['refresh_token'] ?? '');
        $response = $state['refresh'][$refreshToken] ?? null;
    }

    if ($response === null) {
        http_response_code(400);
        echo json_encode(['error' => 'invalid_grant', 'error_description' => 'unknown code/refresh_token in mock issuer']);
        return;
    }

    http_response_code(200);
    echo json_encode($response);
    return;
}

http_response_code(404);
echo json_encode(['error' => 'not_found', 'path' => $path]);
