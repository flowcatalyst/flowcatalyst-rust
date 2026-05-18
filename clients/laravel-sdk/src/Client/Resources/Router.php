<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\Exceptions\FlowCatalystException;
use GuzzleHttp\Client as HttpClient;
use GuzzleHttp\Exception\GuzzleException;

/**
 * Router monitoring resource.
 *
 * Talks to the message router (a separate process from the platform) at
 * the URL configured via `FlowCatalystClient`'s `$routerBaseUrl`.
 *
 * Designed for an external recovery / replay process that maintains its
 * own list of "messages that look stuck" and wants to confirm whether the
 * router is still actively processing each one before re-enqueueing.
 *
 * The router monitoring endpoints don't require authentication, so this
 * resource uses its own bare Guzzle client rather than the platform's
 * authenticated request flow.
 */
class Router
{
    /** Server-side cap on batch size. Mirrored client-side. */
    public const CHECK_BATCH_LIMIT = 5000;

    private readonly FlowCatalystClient $client;
    private ?HttpClient $httpClient = null;

    public function __construct(FlowCatalystClient $client)
    {
        $this->client = $client;
    }

    private function http(): HttpClient
    {
        return $this->httpClient ??= new HttpClient([
            'base_uri' => $this->client->getRouterBaseUrl(),
            'http_errors' => false,
            'timeout' => 30,
        ]);
    }

    /**
     * Check whether a single application message ID is currently held in
     * the router's in-pipeline map. O(1) on the server side.
     *
     * @return array{messageId: string, inPipeline: bool, detail?: array<string, mixed>}
     *
     * @throws FlowCatalystException
     */
    public function inPipeline(string $messageId): array
    {
        try {
            $response = $this->http()->get('/monitoring/in-flight-messages/check', [
                'query' => ['messageId' => $messageId],
            ]);
        } catch (GuzzleException $e) {
            throw new FlowCatalystException(
                'Router check failed: ' . $e->getMessage(),
                0,
                $e
            );
        }

        $status = $response->getStatusCode();
        $body = (string) $response->getBody();

        if ($status < 200 || $status >= 300) {
            throw new FlowCatalystException(
                "Router check failed (HTTP {$status}): {$body}"
            );
        }

        /** @var array{messageId: string, inPipeline: bool, detail?: array<string, mixed>} $decoded */
        $decoded = json_decode($body, true, flags: JSON_THROW_ON_ERROR);
        return $decoded;
    }

    /**
     * Batch-check whether each given application message ID is currently
     * held in the router's in-pipeline map. Returns `messageId => bool`.
     * The server caps the batch at `CHECK_BATCH_LIMIT` ids; longer arrays
     * raise HTTP 400.
     *
     * @param list<string> $messageIds
     * @return array<string, bool>
     *
     * @throws FlowCatalystException
     */
    public function inPipelineBatch(array $messageIds): array
    {
        try {
            $response = $this->http()->post(
                '/monitoring/in-flight-messages/check-batch',
                ['json' => ['messageIds' => $messageIds]]
            );
        } catch (GuzzleException $e) {
            throw new FlowCatalystException(
                'Router batch check failed: ' . $e->getMessage(),
                0,
                $e
            );
        }

        $status = $response->getStatusCode();
        $body = (string) $response->getBody();

        if ($status < 200 || $status >= 300) {
            throw new FlowCatalystException(
                "Router batch check failed (HTTP {$status}): {$body}"
            );
        }

        /** @var array<string, bool> $decoded */
        $decoded = json_decode($body, true, flags: JSON_THROW_ON_ERROR);
        return $decoded;
    }
}
