<?php

namespace FlowCatalyst\Generated;

class Client extends \FlowCatalyst\Generated\Runtime\Client\Client
{
    /**
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncDispatchPoolsRequest $requestBody
     * @param array{
     *    "removeUnlisted"?: bool, //Archive pools not in list
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeDispatchPoolsSyncBadRequestException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncResultResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiApplicationsByAppCodeDispatchPoolsSync(string $appCode, ?\FlowCatalyst\Generated\Model\SyncDispatchPoolsRequest $requestBody = null, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiApplicationsByAppCodeDispatchPoolsSync($appCode, $requestBody, $queryParameters), $fetch);
    }
    /**
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncEventTypesRequest $requestBody
     * @param array{
     *    "removeUnlisted"?: bool, //Remove API-sourced event types not in list
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeEventTypesSyncBadRequestException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncResultResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiApplicationsByAppCodeEventTypesSync(string $appCode, ?\FlowCatalyst\Generated\Model\SyncEventTypesRequest $requestBody = null, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiApplicationsByAppCodeEventTypesSync($appCode, $requestBody, $queryParameters), $fetch);
    }
    /**
     * Versioned: the prior CURRENT (if any) is flipped to ARCHIVED with computed
     * change-notes; the incoming document becomes the new CURRENT. Re-sending an
     * unchanged spec is a no-op (returns `unchanged: true`).
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncOpenApiSpecRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeOpenapiSyncBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeOpenapiSyncForbiddenException
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeOpenapiSyncNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncOpenApiSpecResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiApplicationsByAppCodeOpenapiSync(string $appCode, ?\FlowCatalyst\Generated\Model\SyncOpenApiSpecRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiApplicationsByAppCodeOpenapiSync($appCode, $requestBody), $fetch);
    }
    /**
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncPrincipalsRequest $requestBody
     * @param array{
     *    "removeUnlisted"?: bool, //Remove SDK_SYNC roles from unlisted principals
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodePrincipalsSyncBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodePrincipalsSyncNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncResultResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiApplicationsByAppCodePrincipalsSync(string $appCode, ?\FlowCatalyst\Generated\Model\SyncPrincipalsRequest $requestBody = null, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiApplicationsByAppCodePrincipalsSync($appCode, $requestBody, $queryParameters), $fetch);
    }
    /**
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncProcessesRequest $requestBody
     * @param array{
     *    "removeUnlisted"?: bool, //Remove API-sourced processes not in list
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeProcessesSyncBadRequestException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncResultResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiApplicationsByAppCodeProcessesSync(string $appCode, ?\FlowCatalyst\Generated\Model\SyncProcessesRequest $requestBody = null, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiApplicationsByAppCodeProcessesSync($appCode, $requestBody, $queryParameters), $fetch);
    }
    /**
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncRolesRequest $requestBody
     * @param array{
     *    "removeUnlisted"?: bool, //Remove SDK roles not in list
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeRolesSyncBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeRolesSyncNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncResultResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiApplicationsByAppCodeRolesSync(string $appCode, ?\FlowCatalyst\Generated\Model\SyncRolesRequest $requestBody = null, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiApplicationsByAppCodeRolesSync($appCode, $requestBody, $queryParameters), $fetch);
    }
    /**
     * Body specifies the target client (or null for platform-scoped). Caller
     * must have access to that client (or be anchor for platform-scoped).
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncScheduledJobsRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeScheduledJobsSyncBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeScheduledJobsSyncForbiddenException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncScheduledJobsResultResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiApplicationsByAppCodeScheduledJobsSync(string $appCode, ?\FlowCatalyst\Generated\Model\SyncScheduledJobsRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiApplicationsByAppCodeScheduledJobsSync($appCode, $requestBody), $fetch);
    }
    /**
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncSubscriptionsRequest $requestBody
     * @param array{
     *    "removeUnlisted"?: bool, //Remove API-sourced subscriptions not in list
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeSubscriptionsSyncBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiApplicationsByAppCodeSubscriptionsSyncNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncResultResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiApplicationsByAppCodeSubscriptionsSync(string $appCode, ?\FlowCatalyst\Generated\Model\SyncSubscriptionsRequest $requestBody = null, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiApplicationsByAppCodeSubscriptionsSync($appCode, $requestBody, $queryParameters), $fetch);
    }
    /**
    * @param array{
    *    "after"?: string, //Opaque cursor returned by a previous page's `nextCursor`. Omit for
    the first page.
    *    "pageSize"?: int, //Page size (default 50, capped at 200).
    *    "entityType"?: string, //Filter by entity type
    *    "entityId"?: string, //Filter by entity ID
    *    "operation"?: string, //Filter by operation (Java calls this "operation", maps to action internally)
    *    "principalId"?: string, //Filter by principal ID
    * } $queryParameters
    
    * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
    *
    * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuditLogListResponse : \Psr\Http\Message\ResponseInterface)
    */
    public function getApiAuditLogs(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiAuditLogs($queryParameters), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ApplicationIdsResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiAuditLogsApplicationIds(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiAuditLogsApplicationIds(), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientIdsResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiAuditLogsClientIds(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiAuditLogsClientIds(), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EntityTypesResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiAuditLogsEntityTypes(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiAuditLogsEntityTypes(), $fetch);
    }
    /**
     * @param string $entityType Entity type
     * @param string $entityId Entity ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EntityAuditLogsResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiAuditLogsEntityByEntityTypeByEntityId(string $entityType, string $entityId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiAuditLogsEntityByEntityTypeByEntityId($entityType, $entityId), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\OperationsResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiAuditLogsOperations(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiAuditLogsOperations(), $fetch);
    }
    /**
     * @param string $principalId Principal ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuditLogResponse[] : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiAuditLogsPrincipalByPrincipalId(string $principalId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiAuditLogsPrincipalByPrincipalId($principalId), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuditLogResponse[] : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiAuditLogsRecent(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiAuditLogsRecent(), $fetch);
    }
    /**
     * @param string $id Audit log ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiAuditLogsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuditLogDetailResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiAuditLogsById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiAuditLogsById($id), $fetch);
    }
    /**
     * @param array{
     *    "page"?: int, //Page number
     *    "limit"?: int, //Items per page
     *    "status"?: string, //Filter by status
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientListResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiClients(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiClients($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateClientRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiClientsBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiClientsConflictException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiClients(?\FlowCatalyst\Generated\Model\CreateClientRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiClients($requestBody), $fetch);
    }
    /**
     * @param string $identifier Client identifier/slug
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiClientsByIdentifierByIdentifierNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiClientsByIdentifierByIdentifier(string $identifier, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiClientsByIdentifierByIdentifier($identifier), $fetch);
    }
    /**
     * @param array{
     *    "q"?: string, //Search term
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientListResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiClientsSearch(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiClientsSearch($queryParameters), $fetch);
    }
    /**
     * @param string $id Client ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\DeleteApiClientsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteApiClientsById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteApiClientsById($id), $fetch);
    }
    /**
     * @param string $id Client ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiClientsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiClientsById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiClientsById($id), $fetch);
    }
    /**
     * @param string $id Client ID
     * @param null|\FlowCatalyst\Generated\Model\UpdateClientRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PutApiClientsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function putApiClientsById(string $id, ?\FlowCatalyst\Generated\Model\UpdateClientRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PutApiClientsById($id, $requestBody), $fetch);
    }
    /**
     * Transitions a suspended or pending client to active status.
     * @param string $id Client ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiClientsByIdActivateForbiddenException
     * @throws \FlowCatalyst\Generated\Exception\PostApiClientsByIdActivateNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiClientsByIdActivate(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiClientsByIdActivate($id), $fetch);
    }
    /**
     * @param string $id Client ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiClientsByIdApplicationsNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientApplicationsResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiClientsByIdApplications(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiClientsByIdApplications($id), $fetch);
    }
    /**
     * @param string $id Client ID
     * @param null|\FlowCatalyst\Generated\Model\UpdateClientApplicationsRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PutApiClientsByIdApplicationsNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function putApiClientsByIdApplications(string $id, ?\FlowCatalyst\Generated\Model\UpdateClientApplicationsRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PutApiClientsByIdApplications($id, $requestBody), $fetch);
    }
    /**
     * @param string $id Client ID
     * @param string $applicationId Application ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiClientsByIdApplicationsByAppIdDisableNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiClientsByIdApplicationsByAppIdDisable(string $id, string $applicationId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiClientsByIdApplicationsByAppIdDisable($id, $applicationId), $fetch);
    }
    /**
     * @param string $id Client ID
     * @param string $applicationId Application ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiClientsByIdApplicationsByAppIdEnableNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiClientsByIdApplicationsByAppIdEnable(string $id, string $applicationId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiClientsByIdApplicationsByAppIdEnable($id, $applicationId), $fetch);
    }
    /**
     * Deactivates/soft-deletes a client. Requires a reason.
     * @param string $id Client ID
     * @param null|\FlowCatalyst\Generated\Model\StatusChangeRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiClientsByIdDeactivateForbiddenException
     * @throws \FlowCatalyst\Generated\Exception\PostApiClientsByIdDeactivateNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiClientsByIdDeactivate(string $id, ?\FlowCatalyst\Generated\Model\StatusChangeRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiClientsByIdDeactivate($id, $requestBody), $fetch);
    }
    /**
     * @param string $id Client ID
     * @param null|\FlowCatalyst\Generated\Model\AddNoteRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiClientsByIdNotesNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AddNoteResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiClientsByIdNotes(string $id, ?\FlowCatalyst\Generated\Model\AddNoteRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiClientsByIdNotes($id, $requestBody), $fetch);
    }
    /**
     * Suspends a client (e.g., for billing issues). Requires a reason.
     * @param string $id Client ID
     * @param null|\FlowCatalyst\Generated\Model\StatusChangeRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiClientsByIdSuspendForbiddenException
     * @throws \FlowCatalyst\Generated\Exception\PostApiClientsByIdSuspendNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiClientsByIdSuspend(string $id, ?\FlowCatalyst\Generated\Model\StatusChangeRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiClientsByIdSuspend($id, $requestBody), $fetch);
    }
    /**
     * @param array{
     *    "size"?: int, //Result size. Default 50, capped at 1000.
     *    "eventId"?: string, //Filter by event ID
     *    "correlationId"?: string, //Filter by correlation ID
     *    "subscriptionId"?: string, //Filter by subscription ID
     *    "clientIds"?: string, //Filter by client IDs (comma-separated)
     *    "statuses"?: string, //Filter by statuses (comma-separated)
     *    "applications"?: string, //Filter by application codes (comma-separated)
     *    "subdomains"?: string, //Filter by subdomains (comma-separated)
     *    "aggregates"?: string, //Filter by aggregates (comma-separated)
     *    "codes"?: string, //Filter by codes (comma-separated)
     *    "source"?: string, //Free-text search across code, subject, source
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchJobReadResponse[] : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiDispatchJobs(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiDispatchJobs($queryParameters), $fetch);
    }
    /**
     * Creates and queues a new dispatch job for webhook delivery.
     * @param null|\FlowCatalyst\Generated\Model\CreateDispatchJobRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiDispatchJobsBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiDispatchJobsForbiddenException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiDispatchJobs(?\FlowCatalyst\Generated\Model\CreateDispatchJobRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiDispatchJobs($requestBody), $fetch);
    }
    /**
     * @param string $eventId Event ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchJobResponse[] : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiDispatchJobsByEventByEventId(string $eventId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiDispatchJobsByEventByEventId($eventId), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchJobFilterOptionsResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiDispatchJobsFilterOptions(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiDispatchJobsFilterOptions(), $fetch);
    }
    /**
     * @param array{
     *    "size"?: int, //Result size. Default 50, capped at 1000.
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchJobResponse[] : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiDispatchJobsRaw(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiDispatchJobsRaw($queryParameters), $fetch);
    }
    /**
     * @param string $id Dispatch job ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiDispatchJobsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchJobResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiDispatchJobsById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiDispatchJobsById($id), $fetch);
    }
    /**
     * Retrieves the full history of webhook delivery attempts for a job.
     * @param string $id Dispatch job ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiDispatchJobsByIdAttemptsNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchAttemptResponse[] : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiDispatchJobsByIdAttempts(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiDispatchJobsByIdAttempts($id), $fetch);
    }
    /**
     * Returns the full DispatchJob entity serialized directly as JSON (not the DTO).
     * @param string $id Dispatch job ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiDispatchJobsByIdRawNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiDispatchJobsByIdRaw(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiDispatchJobsByIdRaw($id), $fetch);
    }
    /**
     * @param array{
     *    "pagination": array,
     *    "application"?: string, //Filter by application
     *    "clientId"?: string, //Filter by client ID
     *    "status"?: string, //Filter by status
     *    "subdomain"?: string, //Filter by subdomain
     *    "aggregate"?: string, //Filter by aggregate
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventTypeListResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiEventTypes(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiEventTypes($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateEventTypeRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiEventTypesBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiEventTypesConflictException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiEventTypes(?\FlowCatalyst\Generated\Model\CreateEventTypeRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiEventTypes($requestBody), $fetch);
    }
    /**
     * @param string $code Event type code
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiEventTypesByCodeByCodeNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventTypeResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiEventTypesByCodeByCode(string $code, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiEventTypesByCodeByCode($code), $fetch);
    }
    /**
     * @param string $id Event type ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\DeleteApiEventTypesByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteApiEventTypesById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteApiEventTypesById($id), $fetch);
    }
    /**
     * @param string $id Event type ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiEventTypesByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventTypeResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiEventTypesById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiEventTypesById($id), $fetch);
    }
    /**
     * @param string $id Event type ID
     * @param null|\FlowCatalyst\Generated\Model\UpdateEventTypeRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PutApiEventTypesByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function putApiEventTypesById(string $id, ?\FlowCatalyst\Generated\Model\UpdateEventTypeRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PutApiEventTypesById($id, $requestBody), $fetch);
    }
    /**
     * @param string $id Event type ID
     * @param null|\FlowCatalyst\Generated\Model\AddSchemaVersionRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiEventTypesByIdSchemasNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventTypeResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiEventTypesByIdSchemas(string $id, ?\FlowCatalyst\Generated\Model\AddSchemaVersionRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiEventTypesByIdSchemas($id, $requestBody), $fetch);
    }
    /**
     * @param array{
     *    "size"?: int, //Result size. Default 50, capped at 1000.
     *    "clientIds"?: string, //Filter by client IDs (comma-separated)
     *    "types"?: string, //Filter by event types (comma-separated)
     *    "applications"?: string, //Filter by application codes (comma-separated)
     *    "subdomains"?: string, //Filter by subdomains (comma-separated)
     *    "aggregates"?: string, //Filter by aggregates (comma-separated)
     *    "correlationId"?: string, //Filter by correlation ID
     *    "source"?: string, //Free-text search across type, source, subject
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventRead[] : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiEvents(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiEvents($queryParameters), $fetch);
    }
    /**
     * Creates a new event in the event store. If a deduplicationId is provided and
     * an event with that ID already exists, the existing event is returned (idempotent operation).
     * Dispatch jobs are automatically created for matching subscriptions.
     * @param null|\FlowCatalyst\Generated\Model\CreateEventRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiEventsBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiEventsForbiddenException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreateEventResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiEvents(?\FlowCatalyst\Generated\Model\CreateEventRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiEvents($requestBody), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventFilterOptions : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiEventsFilterOptions(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiEventsFilterOptions(), $fetch);
    }
    /**
     * @param array{
     *    "size"?: int, //Result size. Default 50, capped at 1000.
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventSummaryResponse[] : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiEventsRaw(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiEventsRaw($queryParameters), $fetch);
    }
    /**
     * @param string $id Event ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiEventsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiEventsById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiEventsById($id), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CircuitBreakersResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiMonitoringCircuitBreakers(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiMonitoringCircuitBreakers(), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DashboardMetrics : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiMonitoringDashboard(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiMonitoringDashboard(), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\InFlightMessagesResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiMonitoringInFlightMessages(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiMonitoringInFlightMessages(), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PoolStatsResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiMonitoringPoolStats(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiMonitoringPoolStats(), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StandbyStatus : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiMonitoringStandbyStatus(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiMonitoringStandbyStatus(), $fetch);
    }
    /**
     * @param array{
     *    "pagination": array,
     *    "active"?: bool, //Filter by active status
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\OAuthClientListResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiOauthClients(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiOauthClients($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateOAuthClientRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiOauthClientsBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiOauthClientsConflictException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreateOAuthClientResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiOauthClients(?\FlowCatalyst\Generated\Model\CreateOAuthClientRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiOauthClients($requestBody), $fetch);
    }
    /**
     * @param string $clientId OAuth client_id (public identifier)
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiOauthClientsByClientIdNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\OAuthClientResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiOauthClientsByClientId(string $clientId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiOauthClientsByClientId($clientId), $fetch);
    }
    /**
     * @param string $id OAuth client ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\DeleteApiOauthClientsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteApiOauthClientsById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteApiOauthClientsById($id), $fetch);
    }
    /**
     * @param string $id OAuth client ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiOauthClientsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\OAuthClientResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiOauthClientsById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiOauthClientsById($id), $fetch);
    }
    /**
     * @param string $id OAuth client ID
     * @param null|\FlowCatalyst\Generated\Model\UpdateOAuthClientRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PutApiOauthClientsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function putApiOauthClientsById(string $id, ?\FlowCatalyst\Generated\Model\UpdateOAuthClientRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PutApiOauthClientsById($id, $requestBody), $fetch);
    }
    /**
     * @param string $id OAuth client ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiOauthClientsActivateNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SuccessResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiOauthClientsActivate(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiOauthClientsActivate($id), $fetch);
    }
    /**
     * @param string $id OAuth client ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiOauthClientsDeactivateNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SuccessResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiOauthClientsDeactivate(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiOauthClientsDeactivate($id), $fetch);
    }
    /**
     * @param string $id OAuth client ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiOauthClientsRegenerateSecretNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RegenerateSecretResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiOauthClientsRegenerateSecret(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiOauthClientsRegenerateSecret($id), $fetch);
    }
    /**
     * @param string $id OAuth client ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiOauthClientsRotateSecretNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RegenerateSecretResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiOauthClientsRotateSecret(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiOauthClientsRotateSecret($id), $fetch);
    }
    /**
     * @param array{
     *    "page"?: int, //Page number
     *    "limit"?: int, //Items per page
     *    "type"?: string, //Filter by type
     *    "scope"?: string, //Filter by scope
     *    "client_id"?: string, //Filter by client ID
     *    "email"?: string, //Exact email match (case-insensitive)
     *    "q"?: string, //Search by name or email (substring)
     *    "active"?: bool, //Filter by active status
     *    "roles"?: string, //Filter by roles (comma-separated)
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PrincipalListResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiPrincipals(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiPrincipals($queryParameters), $fetch);
    }
    /**
     * @param array{
     *    "domain": string, //Email domain to check
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CheckEmailDomainResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiPrincipalsCheckEmailDomain(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiPrincipalsCheckEmailDomain($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateUserRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsUsersBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsUsersConflictException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PrincipalResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiPrincipalsUsers(?\FlowCatalyst\Generated\Model\CreateUserRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiPrincipalsUsers($requestBody), $fetch);
    }
    /**
     * @param string $id Principal ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\DeleteApiPrincipalsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteApiPrincipalsById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteApiPrincipalsById($id), $fetch);
    }
    /**
     * @param string $id Principal ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiPrincipalsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PrincipalResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiPrincipalsById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiPrincipalsById($id), $fetch);
    }
    /**
     * @param string $id Principal ID
     * @param null|\FlowCatalyst\Generated\Model\UpdatePrincipalRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PutApiPrincipalsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PrincipalResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function putApiPrincipalsById(string $id, ?\FlowCatalyst\Generated\Model\UpdatePrincipalRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PutApiPrincipalsById($id, $requestBody), $fetch);
    }
    /**
     * Reactivates a deactivated principal.
     * @param string $id Principal ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdActivateForbiddenException
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdActivateNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiPrincipalsByIdActivate(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiPrincipalsByIdActivate($id), $fetch);
    }
    /**
     * Returns all applications the principal has been granted access to.
     * @param string $id Principal ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiPrincipalsByIdApplicationAccessNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ApplicationAccessListResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiPrincipalsByIdApplicationAccess(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiPrincipalsByIdApplicationAccess($id), $fetch);
    }
    /**
     * Replaces all application access with the provided list.
     * @param string $id Principal ID
     * @param null|\FlowCatalyst\Generated\Model\SetApplicationAccessRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PutApiPrincipalsByIdApplicationAccessNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SetApplicationAccessResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function putApiPrincipalsByIdApplicationAccess(string $id, ?\FlowCatalyst\Generated\Model\SetApplicationAccessRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PutApiPrincipalsByIdApplicationAccess($id, $requestBody), $fetch);
    }
    /**
     * ANCHOR users see all active applications.
     * CLIENT users see only applications enabled for their accessible client configs.
     * @param string $id Principal ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiPrincipalsByIdAvailableApplicationsNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AvailableApplicationsResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiPrincipalsByIdAvailableApplications(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiPrincipalsByIdAvailableApplications($id), $fetch);
    }
    /**
     * @param string $id Principal ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiPrincipalsByIdClientAccessNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientAccessListResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiPrincipalsByIdClientAccess(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiPrincipalsByIdClientAccess($id), $fetch);
    }
    /**
     * @param string $id Principal ID
     * @param null|\FlowCatalyst\Generated\Model\GrantClientAccessRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdClientAccessNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientAccessGrantResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiPrincipalsByIdClientAccess(string $id, ?\FlowCatalyst\Generated\Model\GrantClientAccessRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiPrincipalsByIdClientAccess($id, $requestBody), $fetch);
    }
    /**
     * @param string $id Principal ID
     * @param string $clientId Client ID to revoke
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\DeleteApiPrincipalsByIdClientAccessByClientIdNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteApiPrincipalsByIdClientAccessByClientId(string $id, string $clientId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteApiPrincipalsByIdClientAccessByClientId($id, $clientId), $fetch);
    }
    /**
     * Deactivates an active principal.
     * @param string $id Principal ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdDeactivateForbiddenException
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdDeactivateNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiPrincipalsByIdDeactivate(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiPrincipalsByIdDeactivate($id), $fetch);
    }
    /**
     * Resets the password for an internal auth user. Does not work for OIDC users.
     * @param string $id Principal ID
     * @param null|\FlowCatalyst\Generated\Model\ResetPasswordRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdResetPasswordBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdResetPasswordForbiddenException
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdResetPasswordNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiPrincipalsByIdResetPassword(string $id, ?\FlowCatalyst\Generated\Model\ResetPasswordRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiPrincipalsByIdResetPassword($id, $requestBody), $fetch);
    }
    /**
     * @param string $id Principal ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiPrincipalsByIdRolesNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RolesListResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiPrincipalsByIdRoles(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiPrincipalsByIdRoles($id), $fetch);
    }
    /**
     * @param string $id Principal ID
     * @param null|\FlowCatalyst\Generated\Model\AssignRoleRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdRolesNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PrincipalResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiPrincipalsByIdRoles(string $id, ?\FlowCatalyst\Generated\Model\AssignRoleRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiPrincipalsByIdRoles($id, $requestBody), $fetch);
    }
    /**
     * @param string $id Principal ID
     * @param null|\FlowCatalyst\Generated\Model\BatchAssignRolesRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PutApiPrincipalsByIdRolesNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\BatchAssignRolesResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function putApiPrincipalsByIdRoles(string $id, ?\FlowCatalyst\Generated\Model\BatchAssignRolesRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PutApiPrincipalsByIdRoles($id, $requestBody), $fetch);
    }
    /**
     * @param string $id Principal ID
     * @param string $role Role to remove
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\DeleteApiPrincipalsByIdRolesByRoleNameNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PrincipalResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteApiPrincipalsByIdRolesByRoleName(string $id, string $role, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteApiPrincipalsByIdRolesByRoleName($id, $role), $fetch);
    }
    /**
     * Sends the same single-use email as the user-initiated
     * `/auth/password-reset/request` flow. The user clicks the link and sets
     * their own password; the admin never sees or handles the password.
     *
     * Rejects OIDC-federated users (they manage credentials at their IDP) and
     * users without an email address.
     * @param string $id Principal ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdSendPasswordResetBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdSendPasswordResetForbiddenException
     * @throws \FlowCatalyst\Generated\Exception\PostApiPrincipalsByIdSendPasswordResetNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiPrincipalsByIdSendPasswordReset(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiPrincipalsByIdSendPasswordReset($id), $fetch);
    }
    /**
     * @param array{
     *    "pagination": array,
     *    "application"?: string,
     *    "subdomain"?: string,
     *    "status"?: string,
     *    "search"?: string,
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ProcessListResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiProcesses(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiProcesses($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateProcessRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiProcessesBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiProcessesConflictException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiProcesses(?\FlowCatalyst\Generated\Model\CreateProcessRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiProcesses($requestBody), $fetch);
    }
    /**
     * @param string $code Process code
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiProcessesByCodeNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ProcessResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiProcessesByCode(string $code, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiProcessesByCode($code), $fetch);
    }
    /**
     * @param string $id Process ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\DeleteApiProcessesByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteApiProcessesById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteApiProcessesById($id), $fetch);
    }
    /**
     * @param string $id Process ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiProcessesByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ProcessResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiProcessesById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiProcessesById($id), $fetch);
    }
    /**
     * @param string $id Process ID
     * @param null|\FlowCatalyst\Generated\Model\UpdateProcessRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PutApiProcessesByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function putApiProcessesById(string $id, ?\FlowCatalyst\Generated\Model\UpdateProcessRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PutApiProcessesById($id, $requestBody), $fetch);
    }
    /**
     * @param string $id Process ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiProcessesByIdArchiveNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiProcessesByIdArchive(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiProcessesByIdArchive($id), $fetch);
    }
    /**
     * @param array{
     *    "pagination": array,
     *    "applicationCode"?: string, //Filter by application code
     *    "source"?: string, //Filter by source
     *    "clientManaged"?: bool, //Filter client-managed roles only
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RoleListResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiRoles(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiRoles($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateRoleRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiRolesBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiRolesConflictException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiRoles(?\FlowCatalyst\Generated\Model\CreateRoleRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiRoles($requestBody), $fetch);
    }
    /**
     * @param string $applicationId Application ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RoleResponse[] : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiRolesByApplicationByApplicationId(string $applicationId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiRolesByApplicationByApplicationId($applicationId), $fetch);
    }
    /**
     * @param string $code Role code
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiRolesByCodeByCodeNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RoleResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiRolesByCodeByCode(string $code, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiRolesByCodeByCode($code), $fetch);
    }
    /**
     * @param string $source Role source (CODE, DATABASE, SDK)
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiRolesBySourceBySourceBadRequestException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RoleResponse[] : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiRolesBySourceBySource(string $source, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiRolesBySourceBySource($source), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ApplicationOptionsResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiRolesFiltersApplications(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiRolesFiltersApplications(), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PermissionListResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiRolesPermissions(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiRolesPermissions(), $fetch);
    }
    /**
     * @param string $permission Permission string
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiRolesPermissionsByPermissionNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PermissionResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiRolesPermissionsByPermission(string $permission, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiRolesPermissionsByPermission($permission), $fetch);
    }
    /**
     * @param string $roleName Role name (code) or ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\DeleteApiRolesByNameNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteApiRolesByName(string $roleName, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteApiRolesByName($roleName), $fetch);
    }
    /**
     * The frontend calls this with the role name (e.g., "platform:super-admin"),
     * so we try by code first if it contains ":", otherwise by ID.
     * @param string $roleName Role name (code) or ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiRolesByNameNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RoleResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiRolesByName(string $roleName, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiRolesByName($roleName), $fetch);
    }
    /**
     * @param string $roleName Role name (code) or ID
     * @param null|\FlowCatalyst\Generated\Model\UpdateRoleRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PutApiRolesByNameNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function putApiRolesByName(string $roleName, ?\FlowCatalyst\Generated\Model\UpdateRoleRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PutApiRolesByName($roleName, $requestBody), $fetch);
    }
    /**
     * @param string $roleName Role name (code) or ID
     * @param null|\FlowCatalyst\Generated\Model\GrantPermissionRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiRolesByNamePermissionsNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RoleResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiRolesByNamePermissions(string $roleName, ?\FlowCatalyst\Generated\Model\GrantPermissionRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiRolesByNamePermissions($roleName, $requestBody), $fetch);
    }
    /**
     * @param string $roleName Role name (code) or ID
     * @param string $permission Permission to revoke
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\DeleteApiRolesByNamePermissionsByPermissionNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RoleResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteApiRolesByNamePermissionsByPermission(string $roleName, string $permission, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteApiRolesByNamePermissionsByPermission($roleName, $permission), $fetch);
    }
    /**
     * @param array{
     *    "clientId"?: string, //Filter by client. Pass the literal `platform` to filter platform-scoped.
     *    "status"?: string,
     *    "search"?: string,
     *    "pagination": array,
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PaginatedResponseScheduledJobResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiScheduledJobs(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiScheduledJobs($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateScheduledJobRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsForbiddenException
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsConflictException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiScheduledJobs(?\FlowCatalyst\Generated\Model\CreateScheduledJobRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiScheduledJobs($requestBody), $fetch);
    }
    /**
     * @param string $code Scheduled job code
     * @param array{
     *    "clientId"?: string,
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiScheduledJobsByCodeNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ScheduledJobResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiScheduledJobsByCode(string $code, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiScheduledJobsByCode($code, $queryParameters), $fetch);
    }
    /**
     * @param string $instanceId Instance ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiScheduledJobsInstancesByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ScheduledJobInstanceResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiScheduledJobsInstancesById(string $instanceId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiScheduledJobsInstancesById($instanceId), $fetch);
    }
    /**
     * @param string $instanceId Instance ID
     * @param null|\FlowCatalyst\Generated\Model\InstanceCompleteRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsInstancesByIdCompleteForbiddenException
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsInstancesByIdCompleteNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiScheduledJobsInstancesByIdComplete(string $instanceId, ?\FlowCatalyst\Generated\Model\InstanceCompleteRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiScheduledJobsInstancesByIdComplete($instanceId, $requestBody), $fetch);
    }
    /**
     * @param string $instanceId Instance ID
     * @param null|\FlowCatalyst\Generated\Model\InstanceLogRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsInstancesByIdLogForbiddenException
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsInstancesByIdLogNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiScheduledJobsInstancesByIdLog(string $instanceId, ?\FlowCatalyst\Generated\Model\InstanceLogRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiScheduledJobsInstancesByIdLog($instanceId, $requestBody), $fetch);
    }
    /**
     * @param string $instanceId Instance ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiScheduledJobsInstancesByIdLogsNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\InstanceLogResponse[] : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiScheduledJobsInstancesByIdLogs(string $instanceId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiScheduledJobsInstancesByIdLogs($instanceId), $fetch);
    }
    /**
     * @param string $id Scheduled job ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\DeleteApiScheduledJobsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteApiScheduledJobsById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteApiScheduledJobsById($id), $fetch);
    }
    /**
     * @param string $id Scheduled job ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiScheduledJobsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ScheduledJobResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiScheduledJobsById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiScheduledJobsById($id), $fetch);
    }
    /**
     * @param string $id Scheduled job ID
     * @param null|\FlowCatalyst\Generated\Model\UpdateScheduledJobRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PutApiScheduledJobsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function putApiScheduledJobsById(string $id, ?\FlowCatalyst\Generated\Model\UpdateScheduledJobRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PutApiScheduledJobsById($id, $requestBody), $fetch);
    }
    /**
     * @param string $id Scheduled job ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsByIdArchiveNotFoundException
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsByIdArchiveConflictException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiScheduledJobsByIdArchive(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiScheduledJobsByIdArchive($id), $fetch);
    }
    /**
     * @param string $id Scheduled job ID
     * @param null|\FlowCatalyst\Generated\Model\FireRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsByIdFireNotFoundException
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsByIdFireConflictException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiScheduledJobsByIdFire(string $id, ?\FlowCatalyst\Generated\Model\FireRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiScheduledJobsByIdFire($id, $requestBody), $fetch);
    }
    /**
     * @param string $id Scheduled job ID
     * @param array{
     *    "status"?: string,
     *    "triggerKind"?: string,
     *    "from"?: string,
     *    "to"?: string,
     *    "pagination": array,
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PaginatedResponseScheduledJobInstanceResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiScheduledJobsByIdInstances(string $id, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiScheduledJobsByIdInstances($id, $queryParameters), $fetch);
    }
    /**
     * @param string $id Scheduled job ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsByIdPauseNotFoundException
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsByIdPauseConflictException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiScheduledJobsByIdPause(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiScheduledJobsByIdPause($id), $fetch);
    }
    /**
     * @param string $id Scheduled job ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsByIdResumeNotFoundException
     * @throws \FlowCatalyst\Generated\Exception\PostApiScheduledJobsByIdResumeConflictException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiScheduledJobsByIdResume(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiScheduledJobsByIdResume($id), $fetch);
    }
    /**
     * @param array{
     *    "pagination": array,
     *    "clientId"?: string, //Filter by client ID
     *    "status"?: string, //Filter by status
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SubscriptionListResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiSubscriptions(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiSubscriptions($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateSubscriptionRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiSubscriptionsBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostApiSubscriptionsConflictException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiSubscriptions(?\FlowCatalyst\Generated\Model\CreateSubscriptionRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiSubscriptions($requestBody), $fetch);
    }
    /**
     * @param string $id Subscription ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\DeleteApiSubscriptionsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteApiSubscriptionsById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteApiSubscriptionsById($id), $fetch);
    }
    /**
     * @param string $id Subscription ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetApiSubscriptionsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SubscriptionResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getApiSubscriptionsById(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApiSubscriptionsById($id), $fetch);
    }
    /**
     * @param string $id Subscription ID
     * @param null|\FlowCatalyst\Generated\Model\UpdateSubscriptionRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PutApiSubscriptionsByIdNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function putApiSubscriptionsById(string $id, ?\FlowCatalyst\Generated\Model\UpdateSubscriptionRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PutApiSubscriptionsById($id, $requestBody), $fetch);
    }
    /**
     * @param string $id Subscription ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiSubscriptionsByIdPauseNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SubscriptionResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiSubscriptionsByIdPause(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiSubscriptionsByIdPause($id), $fetch);
    }
    /**
     * @param string $id Subscription ID
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostApiSubscriptionsByIdResumeNotFoundException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SubscriptionResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postApiSubscriptionsByIdResume(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostApiSubscriptionsByIdResume($id), $fetch);
    }
    /**
     * Determines how a user with the given email should authenticate:
     * - Internal: username/password
     * - OIDC: external identity provider
     *
     * This is called before showing the login form to determine
     * if the user should be redirected to an external IDP.
     * @param array{
     *    "email": string, //Email address to check
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DomainCheckResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getAuthCheckDomain(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetAuthCheckDomain($queryParameters), $fetch);
    }
    /**
     * Authenticates a user with email and password credentials.
     * Returns an access token on success and sets a session cookie.
     * @param null|\FlowCatalyst\Generated\Model\LoginRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostAuthLoginUnauthorizedException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\LoginResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postAuthLogin(?\FlowCatalyst\Generated\Model\LoginRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostAuthLogin($requestBody), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function postAuthLogout(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostAuthLogout(), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetAuthMeUnauthorizedException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CurrentUserResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function getAuthMe(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetAuthMe(), $fetch);
    }
    /**
     * Exchange a refresh token for a new access token.
     * The refresh token is rotated (old one invalidated, new one issued).
     * @param null|\FlowCatalyst\Generated\Model\RefreshTokenRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostAuthRefreshUnauthorizedException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\TokenRefreshResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postAuthRefresh(?\FlowCatalyst\Generated\Model\RefreshTokenRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostAuthRefresh($requestBody), $fetch);
    }
    /**
     * Returns a `PublicKeyCredentialRequestOptions` challenge. The response
     * shape is identical for known and unknown emails (deterministic-fake
     * `allowCredentials` is generated for unknown / federated / no-credentials
     * cases) — clients cannot distinguish them.
     * @param null|\FlowCatalyst\Generated\Model\AuthenticateBeginRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuthenticateBeginResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postWebauthnAuthenticateBegin(?\FlowCatalyst\Generated\Model\AuthenticateBeginRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostWebauthnAuthenticateBegin($requestBody), $fetch);
    }
    /**
     * Validates the assertion, applies counter / backup-state updates,
     * re-checks the federation gate (hard cutover), and on success issues a
     * session cookie. All failure modes return 401 `INVALID_CREDENTIALS` with
     * an identical shape to defeat enumeration.
     * @param null|\FlowCatalyst\Generated\Model\AuthenticateCompleteRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostWebauthnAuthenticateCompleteUnauthorizedException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuthenticateCompleteResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postWebauthnAuthenticateComplete(?\FlowCatalyst\Generated\Model\AuthenticateCompleteRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostWebauthnAuthenticateComplete($requestBody), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\GetWebauthnCredentialsUnauthorizedException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CredentialSummary[] : \Psr\Http\Message\ResponseInterface)
     */
    public function getWebauthnCredentials(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetWebauthnCredentials(), $fetch);
    }
    /**
     * @param string $id Credential id (pkc_…)
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\DeleteWebauthnCredentialUnauthorizedException
     * @throws \FlowCatalyst\Generated\Exception\DeleteWebauthnCredentialNotFoundException
     *
     * @return ($fetch is 'object' ? null : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteWebauthnCredential(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteWebauthnCredential($id), $fetch);
    }
    /**
     * Returns a WebAuthn `PublicKeyCredentialCreationOptions` challenge. The
     * browser passes this to `navigator.credentials.create()` and posts the
     * result to `/auth/webauthn/register/complete`.
     * @param null|\FlowCatalyst\Generated\Model\RegisterBeginRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostWebauthnRegisterBeginBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostWebauthnRegisterBeginUnauthorizedException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RegisterBeginResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postWebauthnRegisterBegin(?\FlowCatalyst\Generated\Model\RegisterBeginRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostWebauthnRegisterBegin($requestBody), $fetch);
    }
    /**
     * Validates the browser's attestation response and stores the credential.
     * @param null|\FlowCatalyst\Generated\Model\RegisterCompleteRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     * @throws \FlowCatalyst\Generated\Exception\PostWebauthnRegisterCompleteBadRequestException
     * @throws \FlowCatalyst\Generated\Exception\PostWebauthnRegisterCompleteUnauthorizedException
     * @throws \FlowCatalyst\Generated\Exception\PostWebauthnRegisterCompleteForbiddenException
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RegisterCompleteResponse : \Psr\Http\Message\ResponseInterface)
     */
    public function postWebauthnRegisterComplete(?\FlowCatalyst\Generated\Model\RegisterCompleteRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PostWebauthnRegisterComplete($requestBody), $fetch);
    }
    public static function create($httpClient = null, array $additionalPlugins = [], array $additionalNormalizers = [])
    {
        if (null === $httpClient) {
            $httpClient = \Http\Discovery\Psr18ClientDiscovery::find();
            $plugins = [];
            if (count($additionalPlugins) > 0) {
                $plugins = array_merge($plugins, $additionalPlugins);
            }
            $httpClient = new \Http\Client\Common\PluginClient($httpClient, $plugins);
        }
        $requestFactory = \Http\Discovery\Psr17FactoryDiscovery::findRequestFactory();
        $streamFactory = \Http\Discovery\Psr17FactoryDiscovery::findStreamFactory();
        $normalizers = [new \Symfony\Component\Serializer\Normalizer\ArrayDenormalizer(), new \FlowCatalyst\Generated\Normalizer\JaneObjectNormalizer()];
        if (count($additionalNormalizers) > 0) {
            $normalizers = array_merge($normalizers, $additionalNormalizers);
        }
        $serializer = new \Symfony\Component\Serializer\Serializer($normalizers, [new \Symfony\Component\Serializer\Encoder\JsonEncoder(new \Symfony\Component\Serializer\Encoder\JsonEncode(), new \Symfony\Component\Serializer\Encoder\JsonDecode(['json_decode_associative' => true]))]);
        return new static($httpClient, $requestFactory, $serializer, $streamFactory);
    }
}