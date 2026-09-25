<?php

declare(strict_types=1);

namespace FlowCatalyst\Sync;

/**
 * Represents a connection definition for syncing to FlowCatalyst.
 *
 * A connection is application-owned: the platform assigns its service
 * account itself, so this definition carries nothing environment-specific.
 * Can be used directly or converted to array for the sync API.
 */
final class ConnectionDefinition
{
    /**
     * @param string $code Unique connection code — stable across
     *        environments, unlike its id. What a subscription's
     *        `connectionCode` names.
     * @param string $name Human-readable name
     * @param string|null $description Connection description
     * @param string|null $externalId Your own system's identifier for this
     *        connection, if any
     * @param string|null $applicationCode Application code this connection
     *        belongs to
     * @param string|null $client FlowCatalyst client (identifier slug) this
     *        connection is scoped to. Null = global. Unlike the attribute
     *        default, this is never overridden by `flowcatalyst.client` —
     *        set it explicitly, or scope the whole set via
     *        `SyncDefinitionSet::forClient()` instead.
     */
    public function __construct(
        public readonly string $code,
        public readonly string $name,
        public readonly ?string $description = null,
        public readonly ?string $externalId = null,
        public readonly ?string $applicationCode = null,
        public readonly ?string $client = null,
    ) {}

    /**
     * Create a new connection definition with fluent syntax.
     */
    public static function make(string $code, string $name): self
    {
        return new self($code, $name);
    }

    /**
     * Create a copy with a different description.
     */
    public function withDescription(string $description): self
    {
        return new self(
            code: $this->code,
            name: $this->name,
            description: $description,
            externalId: $this->externalId,
            applicationCode: $this->applicationCode,
            client: $this->client,
        );
    }

    /**
     * Create a copy with a different external id.
     */
    public function withExternalId(string $externalId): self
    {
        return new self(
            code: $this->code,
            name: $this->name,
            description: $this->description,
            externalId: $externalId,
            applicationCode: $this->applicationCode,
            client: $this->client,
        );
    }

    /**
     * Create a copy scoped to a specific client.
     */
    public function forClient(string $client): self
    {
        return new self(
            code: $this->code,
            name: $this->name,
            description: $this->description,
            externalId: $this->externalId,
            applicationCode: $this->applicationCode,
            client: $client,
        );
    }

    /**
     * Convert to array for the sync API. `client` is a routing field (read
     * by the synchronizer to pick which call this row belongs to) rather
     * than a payload field of the connection itself, but is included here —
     * unlike the attribute's `toArray()` — because nothing else resolves it
     * for a programmatically-built definition.
     *
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $data = [
            'code' => $this->code,
            'name' => $this->name,
        ];

        if ($this->description !== null) {
            $data['description'] = $this->description;
        }
        if ($this->externalId !== null) {
            $data['externalId'] = $this->externalId;
        }
        if ($this->applicationCode !== null) {
            $data['applicationCode'] = $this->applicationCode;
        }
        if ($this->client !== null) {
            $data['client'] = $this->client;
        }

        return $data;
    }

    /**
     * Create from array (e.g., from cached definitions).
     *
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        return new self(
            code: $data['code'],
            name: $data['name'],
            description: $data['description'] ?? null,
            externalId: $data['externalId'] ?? null,
            applicationCode: $data['applicationCode'] ?? null,
            client: $data['client'] ?? null,
        );
    }
}
