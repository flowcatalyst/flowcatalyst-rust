<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\MyClient;
use FlowCatalyst\DTOs\Responses\MyApplicationList;
use FlowCatalyst\DTOs\Responses\MyClientList;

/**
 * Me Resource
 *
 * User-scoped endpoints for accessing resources the authenticated user
 * has access to. These endpoints do NOT require admin permissions.
 */
class Me
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * Get clients the authenticated user has access to.
     *
     * Access is determined by user scope:
     * - ANCHOR: All active clients
     * - PARTNER: IDP granted clients + explicit grants
     * - CLIENT: Home client + IDP additional clients + explicit grants
     */
    public function getClients(): MyClientList
    {
        $response = $this->client->request('GET', '/api/me/clients');

        return MyClientList::fromArray($response);
    }

    /**
     * Get a specific client the user has access to.
     */
    public function getClient(string $clientId): MyClient
    {
        $response = $this->client->request('GET', "/api/me/clients/{$clientId}");

        return MyClient::fromArray($response);
    }

    /**
     * Get applications enabled for a client the user has access to.
     */
    public function getClientApplications(string $clientId): MyApplicationList
    {
        $response = $this->client->request('GET', "/api/me/clients/{$clientId}/applications");

        return MyApplicationList::fromArray($response);
    }
}
