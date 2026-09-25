<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Support;

/**
 * A real local OIDC issuer for end-to-end SDK tests, backed by a genuine
 * RS256 keypair — mirrors the TypeScript SDK's `mock-issuer.ts`.
 *
 * The SDK's OidcAuthController and TokenRefresher each build their own
 * internal Guzzle client (not dependency-injected), so a MockHandler swap
 * cannot intercept their HTTP calls. This spins up a real `php -S` server
 * instead: JWKS/discovery are served statically, and `/oauth/token`
 * responses are prepared (signed) here in the test process — which holds
 * the private key — and handed to the router process via a small JSON
 * state file, keyed by the authorization code / refresh token presented.
 *
 * Only what the SDK actually calls is implemented: discovery, JWKS, and
 * POST /oauth/token for the authorization_code and refresh_token grants.
 * There is no real /oauth/authorize — tests drive the callback/refresh
 * routes directly with a pre-queued code, exactly as the TS mock issuer's
 * tests do via `setNextCode`/`signIdToken`.
 */
final class MockIssuer
{
    public readonly string $baseUrl;
    public readonly string $issuer;

    /** @var resource */
    private $process;
    private string $stateFile;
    private string $jwksFile;
    private string $privateKeyPem;
    private string $kid;

    /** Second, unregistered keypair — signs a token an attacker controls but whose `kid` claims a real one. */
    private string $roguePrivateKeyPem;

    public function __construct()
    {
        [$this->privateKeyPem, $n, $e] = self::generateKeyPair();
        [$this->roguePrivateKeyPem] = self::generateKeyPair();
        $this->kid = 'test-key-1';

        $jwks = ['keys' => [[
            'kty' => 'RSA',
            'kid' => $this->kid,
            'alg' => 'RS256',
            'use' => 'sig',
            'n' => $n,
            'e' => $e,
        ]]];

        $this->jwksFile = tempnam(sys_get_temp_dir(), 'fc_mock_jwks_');
        file_put_contents($this->jwksFile, json_encode($jwks));

        $this->stateFile = tempnam(sys_get_temp_dir(), 'fc_mock_state_');
        file_put_contents($this->stateFile, json_encode(['codes' => [], 'refresh' => []]));

        $port = self::findFreePort();
        $this->baseUrl = "http://127.0.0.1:{$port}";
        $this->issuer = $this->baseUrl;

        $router = __DIR__ . '/mock-issuer-router.php';
        $descriptors = [1 => ['pipe', 'w'], 2 => ['pipe', 'w']];
        $env = array_merge(getenv() ?: [], [
            'FC_MOCK_ISSUER' => $this->issuer,
            'FC_MOCK_JWKS_FILE' => $this->jwksFile,
            'FC_MOCK_STATE_FILE' => $this->stateFile,
        ]);

        $cmd = escapeshellarg(PHP_BINARY) . ' -S 127.0.0.1:' . $port . ' ' . escapeshellarg($router);
        $process = proc_open($cmd, $descriptors, $pipes, sys_get_temp_dir(), $env);
        if ($process === false) {
            throw new \RuntimeException('failed to start mock issuer server');
        }
        $this->process = $process;
        stream_set_blocking($pipes[1], false);
        stream_set_blocking($pipes[2], false);

        self::waitUntilUp($this->baseUrl);
    }

    public function stop(): void
    {
        if (is_resource($this->process)) {
            proc_terminate($this->process);
            proc_close($this->process);
        }
        @unlink($this->stateFile);
        @unlink($this->jwksFile);
    }

    /**
     * Sign `$claims` as a JWT with the issuer's registered key/kid — this is
     * what a genuine token from this issuer looks like.
     */
    public function sign(array $claims): string
    {
        return self::signWith($claims, $this->privateKeyPem, $this->kid);
    }

    /**
     * Sign `$claims` with a DIFFERENT, unregistered private key while still
     * claiming the issuer's real `kid` in the header. The JWKS lookup
     * succeeds (the kid is real) but `openssl_verify` must fail — this is
     * what "signed by an unknown key" looks like on the wire, and is the
     * only way to exercise the signature check itself rather than the kid
     * lookup.
     */
    public function signWithRogueKey(array $claims): string
    {
        return self::signWith($claims, $this->roguePrivateKeyPem, $this->kid);
    }

    /**
     * Queue the JSON body `/oauth/token` returns for `grant_type=authorization_code&code=$code`.
     */
    public function queueAuthCode(string $code, array $tokenResponseBody): void
    {
        $this->mutateState(function (array &$state) use ($code, $tokenResponseBody): void {
            $state['codes'][$code] = $tokenResponseBody;
        });
    }

    /**
     * Queue the JSON body `/oauth/token` returns for `grant_type=refresh_token&refresh_token=$refreshToken`.
     */
    public function queueRefreshToken(string $refreshToken, array $tokenResponseBody): void
    {
        $this->mutateState(function (array &$state) use ($refreshToken, $tokenResponseBody): void {
            $state['refresh'][$refreshToken] = $tokenResponseBody;
        });
    }

    private function mutateState(callable $mutator): void
    {
        $fh = fopen($this->stateFile, 'c+');
        if ($fh === false) {
            throw new \RuntimeException('cannot open mock issuer state file');
        }
        flock($fh, LOCK_EX);
        $raw = stream_get_contents($fh);
        $state = is_string($raw) ? (json_decode($raw, true) ?: ['codes' => [], 'refresh' => []]) : ['codes' => [], 'refresh' => []];
        $mutator($state);
        ftruncate($fh, 0);
        rewind($fh);
        fwrite($fh, json_encode($state));
        fflush($fh);
        flock($fh, LOCK_UN);
        fclose($fh);
    }

    private static function signWith(array $claims, string $privateKeyPem, string $kid): string
    {
        $header = ['alg' => 'RS256', 'typ' => 'JWT', 'kid' => $kid];
        $h64 = self::b64u(json_encode($header));
        $p64 = self::b64u(json_encode($claims));
        $signingInput = "{$h64}.{$p64}";
        $ok = openssl_sign($signingInput, $signature, $privateKeyPem, OPENSSL_ALGO_SHA256);
        if (!$ok) {
            throw new \RuntimeException('failed to sign mock JWT');
        }
        return $signingInput . '.' . self::b64u($signature);
    }

    /**
     * @return array{0: string, 1: string, 2: string} [privateKeyPem, n (b64url), e (b64url)]
     */
    private static function generateKeyPair(): array
    {
        $resource = openssl_pkey_new([
            'private_key_bits' => 2048,
            'private_key_type' => OPENSSL_KEYTYPE_RSA,
        ]);
        if ($resource === false) {
            throw new \RuntimeException('openssl_pkey_new failed - is the openssl extension enabled?');
        }
        openssl_pkey_export($resource, $privateKeyPem);
        $details = openssl_pkey_get_details($resource);
        $n = self::b64u($details['rsa']['n']);
        $e = self::b64u($details['rsa']['e']);
        return [$privateKeyPem, $n, $e];
    }

    private static function b64u(string $data): string
    {
        return rtrim(strtr(base64_encode($data), '+/', '-_'), '=');
    }

    private static function findFreePort(): int
    {
        $socket = stream_socket_server('tcp://127.0.0.1:0', $errno, $errstr);
        if ($socket === false) {
            throw new \RuntimeException("cannot find a free port: {$errstr}");
        }
        $name = stream_socket_get_name($socket, false);
        fclose($socket);
        $parts = explode(':', $name);
        return (int) end($parts);
    }

    private static function waitUntilUp(string $baseUrl): void
    {
        $deadline = microtime(true) + 5.0;
        while (microtime(true) < $deadline) {
            $ctx = stream_context_create(['http' => ['timeout' => 0.2, 'ignore_errors' => true]]);
            $result = @file_get_contents($baseUrl . '/.well-known/openid-configuration', false, $ctx);
            if ($result !== false) {
                return;
            }
            usleep(20_000);
        }
        throw new \RuntimeException("mock issuer server at {$baseUrl} did not come up in time");
    }
}
