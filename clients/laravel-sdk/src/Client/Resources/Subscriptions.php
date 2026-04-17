<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Requests\CreateSubscriptionRequest;
use FlowCatalyst\DTOs\Requests\SyncSubscriptionEntry;
use FlowCatalyst\DTOs\Requests\UpdateSubscriptionRequest;
use FlowCatalyst\DTOs\Responses\SubscriptionList;
use FlowCatalyst\DTOs\Responses\SyncResult;
use FlowCatalyst\DTOs\Subscription;

class Subscriptions
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List subscriptions.
     */
    public function list(
        ?string $clientId = null,
        ?string $status = null,
    ): SubscriptionList {
        $queryParams = [];
        if ($clientId !== null) {
            $queryParams['clientId'] = $clientId;
        }
        if ($status !== null) {
            $queryParams['status'] = $status;
        }
        $query = !empty($queryParams) ? '?' . http_build_query($queryParams) : '';
        $response = $this->client->request('GET', "/api/subscriptions{$query}");

        return SubscriptionList::fromArray($response);
    }

    /**
     * Get a subscription by ID.
     */
    public function get(string $id): Subscription
    {
        $response = $this->client->request('GET', "/api/subscriptions/{$id}");

        return Subscription::fromArray($response);
    }

    /**
     * Create a new subscription.
     *
     * Returns the created subscription's ID. Call `get($id)` if you need
     * the full record.
     */
    public function create(CreateSubscriptionRequest $request): string
    {
        $response = $this->client->request('POST', '/api/subscriptions', [
            'json' => $request->toArray(),
        ]);

        return (string) $response['id'];
    }

    /**
     * Update a subscription. The platform responds with 204 No Content.
     */
    public function update(string $id, UpdateSubscriptionRequest $request): void
    {
        $this->client->request('PUT', "/api/subscriptions/{$id}", [
            'json' => $request->toArray(),
        ]);
    }

    /**
     * Delete a subscription.
     */
    public function delete(string $id): void
    {
        $this->client->request('DELETE', "/api/subscriptions/{$id}");
    }

    /**
     * Pause a subscription.
     */
    public function pause(string $id): Subscription
    {
        $response = $this->client->request('POST', "/api/subscriptions/{$id}/pause");

        return Subscription::fromArray($response);
    }

    /**
     * Resume a subscription.
     */
    public function resume(string $id): Subscription
    {
        $response = $this->client->request('POST', "/api/subscriptions/{$id}/resume");

        return Subscription::fromArray($response);
    }

    /**
     * Sync subscriptions for an application. Creates/updates subscriptions
     * with source=`API` and, when `$removeUnlisted` is true, removes
     * API-sourced subscriptions not in the sync list.
     *
     * @param SyncSubscriptionEntry[] $subscriptions
     */
    public function sync(
        string $applicationCode,
        array $subscriptions,
        bool $removeUnlisted = false,
    ): SyncResult {
        $query = $removeUnlisted ? '?removeUnlisted=true' : '';

        $response = $this->client->request(
            'POST',
            "/api/applications/{$applicationCode}/subscriptions/sync{$query}",
            [
                'json' => [
                    'subscriptions' => array_map(
                        fn(SyncSubscriptionEntry $entry) => $entry->toArray(),
                        $subscriptions,
                    ),
                ],
            ],
        );

        return SyncResult::fromArray($response);
    }
}
