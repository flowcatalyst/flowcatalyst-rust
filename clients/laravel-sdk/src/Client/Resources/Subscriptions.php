<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Subscription;
use FlowCatalyst\Enums\DispatchMode;
use FlowCatalyst\Enums\SubscriptionSource;
use FlowCatalyst\Enums\SubscriptionStatus;

class Subscriptions
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List all subscriptions with optional filters.
     *
     * @param array $filters Optional filters: clientId, status
     * @return array{subscriptions: Subscription[], total: int}
     */
    public function list(array $filters = []): array
    {
        $query = http_build_query($filters);
        $endpoint = '/api/subscriptions' . ($query ? "?{$query}" : '');

        $response = $this->client->request('GET', $endpoint);

        return [
            'subscriptions' => array_map(
                fn(array $item) => Subscription::fromArray($item),
                $response['subscriptions'] ?? []
            ),
            'total' => $response['total'] ?? count($response['subscriptions'] ?? []),
        ];
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
     * @param array{
     *     code: string,
     *     name: string,
     *     description?: string,
     *     clientId?: string,
     *     eventTypes: array<array{eventTypeId?: string, eventTypeCode?: string, specVersion?: string}>,
     *     connectionId: string,
     *     queue: string,
     *     dispatchPoolId: string,
     *     customConfig?: array<array{key: string, value: string}>,
     *     source?: SubscriptionSource|string,
     *     maxAgeSeconds?: int,
     *     delaySeconds?: int,
     *     sequence?: int,
     *     mode?: DispatchMode|string,
     *     timeoutSeconds?: int,
     *     dataOnly?: bool
     * } $data
     */
    public function create(array $data): Subscription
    {
        // Convert enums to strings
        if (isset($data['source']) && $data['source'] instanceof SubscriptionSource) {
            $data['source'] = $data['source']->value;
        }
        if (isset($data['mode']) && $data['mode'] instanceof DispatchMode) {
            $data['mode'] = $data['mode']->value;
        }

        // Default source to API when created via SDK
        $data['source'] ??= SubscriptionSource::API->value;

        $response = $this->client->request('POST', '/api/subscriptions', [
            'json' => $data,
        ]);

        return Subscription::fromArray($response);
    }

    /**
     * Update a subscription.
     *
     * @param array{
     *     name?: string,
     *     description?: string,
     *     eventTypes?: array<array{eventTypeId?: string, eventTypeCode?: string, specVersion?: string}>,
     *     connectionId?: string,
     *     queue?: string,
     *     dispatchPoolId?: string,
     *     customConfig?: array<array{key: string, value: string}>,
     *     status?: SubscriptionStatus|string,
     *     maxAgeSeconds?: int,
     *     delaySeconds?: int,
     *     sequence?: int,
     *     mode?: DispatchMode|string,
     *     timeoutSeconds?: int,
     *     dataOnly?: bool
     * } $data
     */
    public function update(string $id, array $data): Subscription
    {
        // Convert enums to strings
        if (isset($data['status']) && $data['status'] instanceof SubscriptionStatus) {
            $data['status'] = $data['status']->value;
        }
        if (isset($data['mode']) && $data['mode'] instanceof DispatchMode) {
            $data['mode'] = $data['mode']->value;
        }

        $response = $this->client->request('PUT', "/api/subscriptions/{$id}", [
            'json' => $data,
        ]);

        return Subscription::fromArray($response);
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
    public function pause(string $id): array
    {
        return $this->client->request('POST', "/api/subscriptions/{$id}/pause");
    }

    /**
     * Resume a subscription.
     */
    public function resume(string $id): array
    {
        return $this->client->request('POST', "/api/subscriptions/{$id}/resume");
    }

    /**
     * List anchor-level subscriptions for an application.
     *
     * @param string $appCode The application code
     * @param string|null $source Filter by source (API or UI)
     * @return array{subscriptions: Subscription[], total: int}
     */
    public function listForApplication(string $appCode, ?string $source = null): array
    {
        $query = $source ? "?source={$source}" : '';
        $response = $this->client->request('GET', "/api/applications/{$appCode}/subscriptions{$query}");

        return [
            'subscriptions' => array_map(
                fn(array $item) => Subscription::fromArray($item),
                $response['subscriptions'] ?? []
            ),
            'total' => $response['total'] ?? count($response['subscriptions'] ?? []),
        ];
    }

    /**
     * Sync subscriptions for an application.
     *
     * This creates/updates anchor-level subscriptions with source=API and optionally
     * removes API-sourced subscriptions not in the sync list.
     *
     * @param string $appCode The application code
     * @param array<array{
     *     code: string,
     *     name: string,
     *     description?: string,
     *     eventTypes?: array<array{eventTypeCode: string, specVersion?: string}>,
     *     connectionId: string,
     *     queue: string,
     *     dispatchPoolCode: string,
     *     customConfig?: array<array{key: string, value: string}>,
     *     maxAgeSeconds?: int,
     *     delaySeconds?: int,
     *     sequence?: int,
     *     mode?: DispatchMode|string,
     *     timeoutSeconds?: int,
     *     maxRetries?: int,
     *     dataOnly?: bool
     * }> $subscriptions The subscriptions to sync
     * @param bool $removeUnlisted If true, removes API-sourced subscriptions not in the list
     * @return array{created: int, updated: int, deleted: int, subscriptions: Subscription[]}
     */
    public function sync(string $appCode, array $subscriptions, bool $removeUnlisted = false): array
    {
        // Convert enums to strings in subscription items
        foreach ($subscriptions as &$sub) {
            if (isset($sub['mode']) && $sub['mode'] instanceof DispatchMode) {
                $sub['mode'] = $sub['mode']->value;
            }
        }

        $query = $removeUnlisted ? '?removeUnlisted=true' : '';

        $response = $this->client->request('POST', "/api/applications/{$appCode}/subscriptions/sync{$query}", [
            'json' => ['subscriptions' => $subscriptions],
        ]);

        return [
            'created' => $response['created'] ?? 0,
            'updated' => $response['updated'] ?? 0,
            'deleted' => $response['deleted'] ?? 0,
            'subscriptions' => array_map(
                fn(array $item) => Subscription::fromArray($item),
                $response['subscriptions'] ?? []
            ),
        ];
    }
}
