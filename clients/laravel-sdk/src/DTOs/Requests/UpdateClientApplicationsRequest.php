<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for PUT /api/clients/{id}/applications.
 *
 * Declarative: the given set of application IDs becomes the enabled set
 * for the client. IDs omitted from this list are disabled.
 */
final class UpdateClientApplicationsRequest
{
    /**
     * @param string[] $enabledApplicationIds
     */
    public function __construct(
        public readonly array $enabledApplicationIds,
    ) {}

    /**
     * @return array<string, string[]>
     */
    public function toArray(): array
    {
        return [
            'enabledApplicationIds' => $this->enabledApplicationIds,
        ];
    }
}
