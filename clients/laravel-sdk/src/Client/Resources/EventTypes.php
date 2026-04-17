<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\EventType;
use FlowCatalyst\DTOs\Requests\AddSchemaVersionRequest;
use FlowCatalyst\DTOs\Requests\CreateEventTypeRequest;
use FlowCatalyst\DTOs\Requests\SyncEventTypeEntry;
use FlowCatalyst\DTOs\Requests\UpdateEventTypeRequest;
use FlowCatalyst\DTOs\Responses\EventTypeList;
use FlowCatalyst\DTOs\Responses\SyncResult;

class EventTypes
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List event types.
     */
    public function list(
        ?string $application = null,
        ?string $clientId = null,
        ?string $status = null,
    ): EventTypeList {
        $queryParams = [];
        if ($application !== null) {
            $queryParams['application'] = $application;
        }
        if ($clientId !== null) {
            $queryParams['clientId'] = $clientId;
        }
        if ($status !== null) {
            $queryParams['status'] = $status;
        }
        $query = !empty($queryParams) ? '?' . http_build_query($queryParams) : '';
        $response = $this->client->request('GET', "/api/event-types{$query}");

        return EventTypeList::fromArray($response);
    }

    /**
     * Get an event type by ID.
     */
    public function get(string $id): EventType
    {
        $response = $this->client->request('GET', "/api/event-types/{$id}");

        return EventType::fromArray($response);
    }

    /**
     * Get an event type by its full code.
     */
    public function getByCode(string $code): EventType
    {
        $response = $this->client->request('GET', "/api/event-types/by-code/{$code}");

        return EventType::fromArray($response);
    }

    /**
     * Create a new event type.
     *
     * Returns the created event type's ID. Call `get($id)` if you need
     * the full record.
     */
    public function create(CreateEventTypeRequest $request): string
    {
        $response = $this->client->request('POST', '/api/event-types', [
            'json' => $request->toArray(),
        ]);

        return (string) $response['id'];
    }

    /**
     * Update an event type. The platform responds with 204 No Content.
     */
    public function update(string $id, UpdateEventTypeRequest $request): void
    {
        $this->client->request('PUT', "/api/event-types/{$id}", [
            'json' => $request->toArray(),
        ]);
    }

    /**
     * Add a new schema version to an event type. The platform
     * auto-increments the version number.
     */
    public function addSchemaVersion(string $id, AddSchemaVersionRequest $request): EventType
    {
        $response = $this->client->request('POST', "/api/event-types/{$id}/versions", [
            'json' => $request->toArray(),
        ]);

        return EventType::fromArray($response);
    }

    /**
     * Delete (archive) an event type.
     */
    public function delete(string $id): void
    {
        $this->client->request('DELETE', "/api/event-types/{$id}");
    }

    /**
     * Sync event types for an application. Creates/updates event types
     * whose source is `API` and, when `$removeUnlisted` is true, archives
     * API-sourced event types not in the sync list.
     *
     * @param SyncEventTypeEntry[] $eventTypes
     */
    public function sync(
        string $applicationCode,
        array $eventTypes,
        bool $removeUnlisted = false,
    ): SyncResult {
        $query = $removeUnlisted ? '?removeUnlisted=true' : '';

        $response = $this->client->request(
            'POST',
            "/api/applications/{$applicationCode}/event-types/sync{$query}",
            [
                'json' => [
                    'eventTypes' => array_map(
                        fn(SyncEventTypeEntry $entry) => $entry->toArray(),
                        $eventTypes,
                    ),
                ],
            ],
        );

        return SyncResult::fromArray($response);
    }
}
