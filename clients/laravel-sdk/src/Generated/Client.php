<?php

namespace FlowCatalyst\Generated;

class Client extends \FlowCatalyst\Generated\Runtime\Client\Client
{
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AnchorDomainListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listAnchorDomains(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListAnchorDomains(), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateAnchorDomainRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createAnchorDomain(?\FlowCatalyst\Generated\Model\CreateAnchorDomainRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateAnchorDomain($requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteAnchorDomain(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteAnchorDomain($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdateAnchorDomainRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updateAnchorDomain(string $id, ?\FlowCatalyst\Generated\Model\UpdateAnchorDomainRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdateAnchorDomain($id, $requestBody), $fetch);
    }
    /**
     * @param array{
     *    "type"?: string,
     *    "active"?: string,
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ApplicationListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listApplications(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListApplications($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateApplicationRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createApplication(?\FlowCatalyst\Generated\Model\CreateApplicationRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateApplication($requestBody), $fetch);
    }
    /**
     * @param string $code
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ApplicationResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getApplicationByCode(string $code, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApplicationByCode($code), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ApplicationRolesResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listApplicationRoles(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListApplicationRoles($id), $fetch);
    }
    /**
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncDispatchPoolsRequest $requestBody
     * @param array{
     *    "removeUnlisted"?: bool, //Archive pools not in the list
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncResultResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function syncDispatchPools(string $appCode, ?\FlowCatalyst\Generated\Model\SyncDispatchPoolsRequest $requestBody = null, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SyncDispatchPools($appCode, $requestBody, $queryParameters), $fetch);
    }
    /**
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncDocsRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncResultResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function syncAppDocs(string $appCode, ?\FlowCatalyst\Generated\Model\SyncDocsRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SyncAppDocs($appCode, $requestBody), $fetch);
    }
    /**
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncEventTypesRequest $requestBody
     * @param array{
     *    "removeUnlisted"?: bool, //Remove API-sourced event types not in the list
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncResultResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function syncEventTypes(string $appCode, ?\FlowCatalyst\Generated\Model\SyncEventTypesRequest $requestBody = null, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SyncEventTypes($appCode, $requestBody, $queryParameters), $fetch);
    }
    /**
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncOpenapiRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncOpenApiSpecResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function syncOpenapi(string $appCode, ?\FlowCatalyst\Generated\Model\SyncOpenapiRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SyncOpenapi($appCode, $requestBody), $fetch);
    }
    /**
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncPrincipalsRequest $requestBody
     * @param array{
     *    "removeUnlisted"?: bool, //Strip SDK_SYNC roles from unlisted principals
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncResultResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function syncPrincipals(string $appCode, ?\FlowCatalyst\Generated\Model\SyncPrincipalsRequest $requestBody = null, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SyncPrincipals($appCode, $requestBody, $queryParameters), $fetch);
    }
    /**
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncProcessesRequest $requestBody
     * @param array{
     *    "removeUnlisted"?: bool, //Remove API/CODE processes not in the list
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncResultResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function syncProcesses(string $appCode, ?\FlowCatalyst\Generated\Model\SyncProcessesRequest $requestBody = null, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SyncProcesses($appCode, $requestBody, $queryParameters), $fetch);
    }
    /**
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncRolesRequest $requestBody
     * @param array{
     *    "removeUnlisted"?: bool, //Remove SDK roles not in the list
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncResultResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function syncRoles(string $appCode, ?\FlowCatalyst\Generated\Model\SyncRolesRequest $requestBody = null, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SyncRoles($appCode, $requestBody, $queryParameters), $fetch);
    }
    /**
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncScheduledJobsRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncScheduledJobsResultResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function syncScheduledJobs(string $appCode, ?\FlowCatalyst\Generated\Model\SyncScheduledJobsRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SyncScheduledJobs($appCode, $requestBody), $fetch);
    }
    /**
     * @param string $appCode Application code
     * @param null|\FlowCatalyst\Generated\Model\SyncSubscriptionsRequest $requestBody
     * @param array{
     *    "removeUnlisted"?: bool, //Remove API/CODE subscriptions not in the list
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncResultResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function syncSubscriptions(string $appCode, ?\FlowCatalyst\Generated\Model\SyncSubscriptionsRequest $requestBody = null, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SyncSubscriptions($appCode, $requestBody, $queryParameters), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteApplication(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteApplication($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ApplicationResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getApplication(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApplication($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdateApplicationRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updateApplication(string $id, ?\FlowCatalyst\Generated\Model\UpdateApplicationRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdateApplication($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ApplicationResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function activateApplication(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ActivateApplication($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientConfigListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listApplicationClientConfigs(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListApplicationClientConfigs($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $clientId
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientConfigResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getApplicationClientConfig(string $id, string $clientId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApplicationClientConfig($id, $clientId), $fetch);
    }
    /**
     * @param string $id
     * @param string $clientId
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function disableApplicationForClient(string $id, string $clientId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DisableApplicationForClient($id, $clientId), $fetch);
    }
    /**
     * @param string $id
     * @param string $clientId
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function enableApplicationForClient(string $id, string $clientId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\EnableApplicationForClient($id, $clientId), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ApplicationResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deactivateApplication(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeactivateApplication($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\ProvisionLoginClientRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ApplicationProvisionLoginClientResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function provisionApplicationLoginClient(string $id, ?\FlowCatalyst\Generated\Model\ProvisionLoginClientRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ProvisionApplicationLoginClient($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ApplicationProvisionServiceAccountResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function provisionApplicationServiceAccount(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ProvisionApplicationServiceAccount($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\AttachServiceAccountRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function attachApplicationServiceAccount(string $id, ?\FlowCatalyst\Generated\Model\AttachServiceAccountRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\AttachApplicationServiceAccount($id, $requestBody), $fetch);
    }
    /**
     * @param array{
     *    "after"?: string, //Opaque cursor from a previous page's nextCursor
     *    "pageSize"?: int, //Page size (default 50, capped at 200)
     *    "entityType"?: string,
     *    "entityId"?: string,
     *    "principalId"?: string,
     *    "operation"?: string,
     *    "applicationIds"?: string, //CSV of application ids
     *    "clientIds"?: string, //CSV of client ids
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuditLogListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listAuditLogs(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListAuditLogs($queryParameters), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuditLogApplicationIDsResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function auditLogApplicationIDs(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\AuditLogApplicationIDs(), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuditLogClientIDsResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function auditLogClientIDs(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\AuditLogClientIDs(), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuditLogEntityTypesResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function auditLogEntityTypes(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\AuditLogEntityTypes(), $fetch);
    }
    /**
     * @param string $entityType
     * @param string $entityId
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuditLogListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function auditLogsByEntity(string $entityType, string $entityId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\AuditLogsByEntity($entityType, $entityId), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuditLogOperationsResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function auditLogOperations(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\AuditLogOperations(), $fetch);
    }
    /**
     * @param string $principalId
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuditLogListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function auditLogsByPrincipal(string $principalId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\AuditLogsByPrincipal($principalId), $fetch);
    }
    /**
     * @param array{
     *    "after"?: string, //Opaque cursor from a previous page's nextCursor
     *    "pageSize"?: int, //Page size (default 50, capped at 200)
     *    "entityType"?: string,
     *    "entityId"?: string,
     *    "principalId"?: string,
     *    "operation"?: string,
     *    "applicationIds"?: string, //CSV of application ids
     *    "clientIds"?: string, //CSV of client ids
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuditLogListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listAuditLogsRecent(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListAuditLogsRecent($queryParameters), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuditLogResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getAuditLog(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetAuditLog($id), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuthConfigListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listAuthConfigs(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListAuthConfigs(), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateAuthConfigRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createAuthConfig(?\FlowCatalyst\Generated\Model\CreateAuthConfigRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateAuthConfig($requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteAuthConfig(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteAuthConfig($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdateAuthConfigRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updateAuthConfig(string $id, ?\FlowCatalyst\Generated\Model\UpdateAuthConfigRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdateAuthConfig($id, $requestBody), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listClients(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListClients(), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateClientRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createClient(?\FlowCatalyst\Generated\Model\CreateClientRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateClient($requestBody), $fetch);
    }
    /**
     * @param string $identifier
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getClientByIdentifier(string $identifier, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetClientByIdentifier($identifier), $fetch);
    }
    /**
     * @param array{
     *    "q"?: string,
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function searchClientsByQuery(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SearchClientsByQuery($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\SearchClientRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function searchClients(?\FlowCatalyst\Generated\Model\SearchClientRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SearchClients($requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteClient(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteClient($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getClient(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetClient($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdateClientRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updateClient(string $id, ?\FlowCatalyst\Generated\Model\UpdateClientRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdateClient($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function activateClient(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ActivateClient($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientApplicationsResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getClientApplications(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetClientApplications($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdateClientApplicationsRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updateClientApplications(string $id, ?\FlowCatalyst\Generated\Model\UpdateClientApplicationsRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdateClientApplications($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $applicationId
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function disableClientApplication(string $id, string $applicationId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DisableClientApplication($id, $applicationId), $fetch);
    }
    /**
     * @param string $id
     * @param string $applicationId
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function enableClientApplication(string $id, string $applicationId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\EnableClientApplication($id, $applicationId), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\StatusChangeRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deactivateClient(string $id, ?\FlowCatalyst\Generated\Model\StatusChangeRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeactivateClient($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\AddNoteRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function addClientNote(string $id, ?\FlowCatalyst\Generated\Model\AddNoteRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\AddClientNote($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\SuspendClientRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function suspendClient(string $id, ?\FlowCatalyst\Generated\Model\SuspendClientRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SuspendClient($id, $requestBody), $fetch);
    }
    /**
     * @param string $app
     * @param string $section
     * @param string $property
     * @param array{
     *    "clientId"?: string,
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deletePlatformConfigProperty(string $app, string $section, string $property, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeletePlatformConfigProperty($app, $section, $property, $queryParameters), $fetch);
    }
    /**
     * @param string $app
     * @param string $section
     * @param string $property
     * @param array{
     *    "clientId"?: string,
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ConfigResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getPlatformConfigProperty(string $app, string $section, string $property, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetPlatformConfigProperty($app, $section, $property, $queryParameters), $fetch);
    }
    /**
     * @param string $app
     * @param string $section
     * @param string $property
     * @param null|\FlowCatalyst\Generated\Model\SetPropertyRequest $requestBody
     * @param array{
     *    "clientId"?: string,
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ConfigResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function setPlatformConfigProperty(string $app, string $section, string $property, ?\FlowCatalyst\Generated\Model\SetPropertyRequest $requestBody = null, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SetPlatformConfigProperty($app, $section, $property, $requestBody, $queryParameters), $fetch);
    }
    /**
     * @param array{
     *    "status"?: string,
     *    "clientId"?: string,
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ConnectionListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listConnections(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListConnections($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateConnectionRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ConnectionResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createConnection(?\FlowCatalyst\Generated\Model\CreateConnectionRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateConnection($requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteConnection(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteConnection($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ConnectionResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getConnection(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetConnection($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdateConnectionRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updateConnection(string $id, ?\FlowCatalyst\Generated\Model\UpdateConnectionRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdateConnection($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ConnectionResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function activateConnection(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ActivateConnection($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ConnectionResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function pauseConnection(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PauseConnection($id), $fetch);
    }
    /**
     * @param array{
     *    "status"?: string,
     *    "clientId"?: string,
     *    "dispatchPoolId"?: string,
     *    "subscriptionId"?: string,
     *    "code"?: string,
     *    "since"?: string, //RFC3339 timestamp
     *    "until"?: string, //RFC3339 timestamp
     *    "limit"?: int,
     *    "offset"?: int,
     *    "size"?: int, //Max rows (default 50, max 1000)
     *    "clientIds"?: string, //CSV of client ids
     *    "statuses"?: string, //CSV of statuses
     *    "applications"?: string, //CSV of application codes
     *    "subdomains"?: string, //CSV of subdomains
     *    "aggregates"?: string, //CSV of aggregates
     *    "codes"?: string, //CSV of codes
     *    "source"?: string, //Free-text source filter
     *    "sort"?: string, //createdAt.asc | createdAt.desc (default)
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchJobRead[]|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listDispatchJobs(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListDispatchJobs($queryParameters), $fetch);
    }
    /**
     * @param string $eventId
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchJobRead[]|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function dispatchJobsByEventAlias(string $eventId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DispatchJobsByEventAlias($eventId), $fetch);
    }
    /**
     * @param string $eventId
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchJobRead[]|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function dispatchJobsByEvent(string $eventId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DispatchJobsByEvent($eventId), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchJobFilterOptionsResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function dispatchJobFilterOptions(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DispatchJobFilterOptions(), $fetch);
    }
    /**
     * @param array{
     *    "status"?: string,
     *    "clientId"?: string,
     *    "dispatchPoolId"?: string,
     *    "subscriptionId"?: string,
     *    "code"?: string,
     *    "since"?: string, //RFC3339 timestamp
     *    "until"?: string, //RFC3339 timestamp
     *    "limit"?: int,
     *    "offset"?: int,
     *    "size"?: int, //Max rows (default 50, max 1000)
     *    "clientIds"?: string, //CSV of client ids
     *    "statuses"?: string, //CSV of statuses
     *    "applications"?: string, //CSV of application codes
     *    "subdomains"?: string, //CSV of subdomains
     *    "aggregates"?: string, //CSV of aggregates
     *    "codes"?: string, //CSV of codes
     *    "source"?: string, //Free-text source filter
     *    "sort"?: string, //createdAt.asc | createdAt.desc (default)
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchJobRead[]|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listDispatchJobsRaw(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListDispatchJobsRaw($queryParameters), $fetch);
    }
    /**
     * @param array{
     *    "status"?: string,
     *    "clientId"?: string,
     *    "dispatchPoolId"?: string,
     *    "subscriptionId"?: string,
     *    "code"?: string,
     *    "since"?: string, //RFC3339 timestamp
     *    "until"?: string, //RFC3339 timestamp
     *    "limit"?: int,
     *    "offset"?: int,
     *    "size"?: int, //Max rows (default 50, max 1000)
     *    "clientIds"?: string, //CSV of client ids
     *    "statuses"?: string, //CSV of statuses
     *    "applications"?: string, //CSV of application codes
     *    "subdomains"?: string, //CSV of subdomains
     *    "aggregates"?: string, //CSV of aggregates
     *    "codes"?: string, //CSV of codes
     *    "source"?: string, //Free-text source filter
     *    "sort"?: string, //createdAt.asc | createdAt.desc (default)
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchJobRead[]|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listDispatchJobsRawAlias(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListDispatchJobsRawAlias($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\RequeueRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RequeueResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function requeueDispatchJobs(?\FlowCatalyst\Generated\Model\RequeueRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\RequeueDispatchJobs($requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchJobResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getDispatchJob(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetDispatchJob($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AttemptDTO[]|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listDispatchJobAttempts(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListDispatchJobAttempts($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchJobResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function cancelDispatchJob(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CancelDispatchJob($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchJobResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function completeDispatchJob(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CompleteDispatchJob($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchJobResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getDispatchJobRaw(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetDispatchJobRaw($id), $fetch);
    }
    /**
     * @param array{
     *    "status"?: string, //Filter by status (ACTIVE, SUSPENDED, ARCHIVED)
     *    "clientId"?: string, //Filter by client id
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchPoolListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listDispatchPools(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListDispatchPools($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateDispatchPoolRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createDispatchPool(?\FlowCatalyst\Generated\Model\CreateDispatchPoolRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateDispatchPool($requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteDispatchPool(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteDispatchPool($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DispatchPoolResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getDispatchPool(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetDispatchPool($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdateDispatchPoolRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updateDispatchPool(string $id, ?\FlowCatalyst\Generated\Model\UpdateDispatchPoolRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdateDispatchPool($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function activateDispatchPool(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ActivateDispatchPool($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function archiveDispatchPool(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ArchiveDispatchPool($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function suspendDispatchPool(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SuspendDispatchPool($id), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RouterConfig|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getRouterConfig(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetRouterConfig(), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DocListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listDocs(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListDocs(), $fetch);
    }
    /**
     * @param string $appCode
     * @param string $slug
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DocResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getApplicationDoc(string $appCode, string $slug, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetApplicationDoc($appCode, $slug), $fetch);
    }
    /**
     * @param string $slug
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DocResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getPlatformDoc(string $slug, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetPlatformDoc($slug), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\MappingListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listEmailDomainMappings(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListEmailDomainMappings(), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateMappingRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createEmailDomainMapping(?\FlowCatalyst\Generated\Model\CreateMappingRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateEmailDomainMapping($requestBody), $fetch);
    }
    /**
     * @param string $domain Email domain to look up (e.g. example.com)
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\MappingResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getEmailDomainMappingByDomain(string $domain, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetEmailDomainMappingByDomain($domain), $fetch);
    }
    /**
     * @param array{
     *    "domain"?: string, //Email domain to look up (e.g. example.com)
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function lookupEmailDomainMapping(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\LookupEmailDomainMapping($queryParameters), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteEmailDomainMapping(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteEmailDomainMapping($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\MappingResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getEmailDomainMapping(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetEmailDomainMapping($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdateMappingRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updateEmailDomainMapping(string $id, ?\FlowCatalyst\Generated\Model\UpdateMappingRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdateEmailDomainMapping($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\MoveProviderRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\MoveProviderResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function moveEmailDomainMappingProvider(string $id, ?\FlowCatalyst\Generated\Model\MoveProviderRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\MoveEmailDomainMappingProvider($id, $requestBody), $fetch);
    }
    /**
     * @param array{
     *    "application"?: string, //Filter by application code
     *    "clientId"?: string, //Filter by client id
     *    "status"?: string, //Filter by status (CURRENT, ARCHIVED)
     *    "subdomain"?: string, //Filter by subdomain
     *    "aggregate"?: string, //Filter by aggregate
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventTypeListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listEventTypes(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListEventTypes($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateEventTypeRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createEventType(?\FlowCatalyst\Generated\Model\CreateEventTypeRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateEventType($requestBody), $fetch);
    }
    /**
     * @param string $code Event type code (e.g. platform:iam:user:created)
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventTypeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getEventTypeByCode(string $code, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetEventTypeByCode($code), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteEventType(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteEventType($id), $fetch);
    }
    /**
     * @param string $id Event type id (TSID)
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventTypeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getEventType(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetEventType($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdateEventTypeRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updateEventType(string $id, ?\FlowCatalyst\Generated\Model\UpdateEventTypeRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdateEventType($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\AddSchemaRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventTypeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function addEventTypeSchema(string $id, ?\FlowCatalyst\Generated\Model\AddSchemaRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\AddEventTypeSchema($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\AddSchemaRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventTypeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function addEventTypeVersion(string $id, ?\FlowCatalyst\Generated\Model\AddSchemaRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\AddEventTypeVersion($id, $requestBody), $fetch);
    }
    /**
     * @param array{
     *    "type"?: string,
     *    "source"?: string,
     *    "subject"?: string,
     *    "clientId"?: string,
     *    "principalId"?: string,
     *    "correlationId"?: string,
     *    "since"?: string, //RFC3339 timestamp
     *    "until"?: string, //RFC3339 timestamp
     *    "limit"?: int,
     *    "offset"?: int,
     *    "size"?: int, //Max rows (default 50, max 1000)
     *    "clientIds"?: string, //CSV of client ids
     *    "applications"?: string, //CSV of application codes
     *    "subdomains"?: string, //CSV of subdomains
     *    "aggregates"?: string, //CSV of aggregates
     *    "types"?: string, //CSV of event types
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventRead[]|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listEvents(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListEvents($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateEventRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreateEventResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createEvent(?\FlowCatalyst\Generated\Model\CreateEventRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateEvent($requestBody), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\BatchRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\BatchResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function batchIngestEvents(?\FlowCatalyst\Generated\Model\BatchRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\BatchIngestEvents($requestBody), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventFilterOptionsResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function eventFilterOptions(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\EventFilterOptions(), $fetch);
    }
    /**
     * @param array{
     *    "type"?: string,
     *    "source"?: string,
     *    "subject"?: string,
     *    "clientId"?: string,
     *    "principalId"?: string,
     *    "correlationId"?: string,
     *    "since"?: string, //RFC3339 timestamp
     *    "until"?: string, //RFC3339 timestamp
     *    "limit"?: int,
     *    "offset"?: int,
     *    "size"?: int, //Max rows (default 50, max 1000)
     *    "clientIds"?: string, //CSV of client ids
     *    "applications"?: string, //CSV of application codes
     *    "subdomains"?: string, //CSV of subdomains
     *    "aggregates"?: string, //CSV of aggregates
     *    "types"?: string, //CSV of event types
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventRead[]|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listEventsRaw(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListEventsRaw($queryParameters), $fetch);
    }
    /**
     * @param array{
     *    "type"?: string,
     *    "source"?: string,
     *    "subject"?: string,
     *    "clientId"?: string,
     *    "principalId"?: string,
     *    "correlationId"?: string,
     *    "since"?: string, //RFC3339 timestamp
     *    "until"?: string, //RFC3339 timestamp
     *    "limit"?: int,
     *    "offset"?: int,
     *    "size"?: int, //Max rows (default 50, max 1000)
     *    "clientIds"?: string, //CSV of client ids
     *    "applications"?: string, //CSV of application codes
     *    "subdomains"?: string, //CSV of subdomains
     *    "aggregates"?: string, //CSV of aggregates
     *    "types"?: string, //CSV of event types
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventRead[]|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listEventsRawAlias(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListEventsRawAlias($queryParameters), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\EventResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getEvent(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetEvent($id), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\IdentityProviderListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listIdentityProviders(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListIdentityProviders(), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateIdentityProviderRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\IdentityProviderResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createIdentityProvider(?\FlowCatalyst\Generated\Model\CreateIdentityProviderRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateIdentityProvider($requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteIdentityProvider(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteIdentityProvider($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\IdentityProviderResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getIdentityProvider(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetIdentityProvider($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdateIdentityProviderRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\IdentityProviderResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updateIdentityProvider(string $id, ?\FlowCatalyst\Generated\Model\UpdateIdentityProviderRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdateIdentityProvider($id, $requestBody), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\IdpRoleMappingListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listIdpRoleMappings(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListIdpRoleMappings(), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateIdpRoleMappingRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createIdpRoleMapping(?\FlowCatalyst\Generated\Model\CreateIdpRoleMappingRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateIdpRoleMapping($requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteIdpRoleMapping(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteIdpRoleMapping($id), $fetch);
    }
    /**
     * @param array{
     *    "attemptType"?: string,
     *    "outcome"?: string,
     *    "identifier"?: string,
     *    "principalId"?: string,
     *    "dateFrom"?: string,
     *    "dateTo"?: string,
     *    "after"?: string,
     *    "pageSize"?: int,
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\LoginAttemptListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listLoginAttempts(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListLoginAttempts($queryParameters), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\OAuthClientListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listOAuthClients(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListOAuthClients(), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateOAuthClientRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreateOAuthClientResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createOAuthClient(?\FlowCatalyst\Generated\Model\CreateOAuthClientRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateOAuthClient($requestBody), $fetch);
    }
    /**
     * @param string $clientId
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\OAuthClientResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getOAuthClientByClientID(string $clientId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetOAuthClientByClientID($clientId), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteOAuthClient(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteOAuthClient($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\OAuthClientResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getOAuthClient(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetOAuthClient($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdateOAuthClientRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updateOAuthClient(string $id, ?\FlowCatalyst\Generated\Model\UpdateOAuthClientRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdateOAuthClient($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SuccessResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function activateOAuthClient(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ActivateOAuthClient($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SuccessResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deactivateOAuthClient(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeactivateOAuthClient($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\RotateOAuthClientSecretRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RotateOAuthClientSecretResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function regenerateOAuthClientSecret(string $id, ?\FlowCatalyst\Generated\Model\RotateOAuthClientSecretRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\RegenerateOAuthClientSecret($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SuccessResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function revokeOAuthClientPreviousSecret(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\RevokeOAuthClientPreviousSecret($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\RotateOAuthClientSecretRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RotateOAuthClientSecretResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function rotateOAuthClientSecret(string $id, ?\FlowCatalyst\Generated\Model\RotateOAuthClientSecretRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\RotateOAuthClientSecret($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function revokePlatformConfigAccess(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\RevokePlatformConfigAccess($id), $fetch);
    }
    /**
     * @param string $app
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ConfigListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listPlatformConfigProperties(string $app, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListPlatformConfigProperties($app), $fetch);
    }
    /**
     * @param string $app
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AccessListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listPlatformConfigAccess(string $app, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListPlatformConfigAccess($app), $fetch);
    }
    /**
     * @param string $app
     * @param null|\FlowCatalyst\Generated\Model\GrantAccessRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function grantPlatformConfigAccess(string $app, ?\FlowCatalyst\Generated\Model\GrantAccessRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GrantPlatformConfigAccess($app, $requestBody), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CorsOriginListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listCorsOrigins(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListCorsOrigins(), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\AddOriginRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function addCorsOrigin(?\FlowCatalyst\Generated\Model\AddOriginRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\AddCorsOrigin($requestBody), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PublicAllowedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function publicAllowedOrigins(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PublicAllowedOrigins(), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteCorsOrigin(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteCorsOrigin($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AllowedOriginResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getCorsOrigin(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetCorsOrigin($id), $fetch);
    }
    /**
     * @param array{
     *    "clientId"?: string, //Tenant client whose portal apps to list (anchors may omit it for every client's)
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PortalAppListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listPortalApps(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListPortalApps($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreatePortalAppRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatePortalAppResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createPortalApp(?\FlowCatalyst\Generated\Model\CreatePortalAppRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreatePortalApp($requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param array{
     *    "clientId"?: string, //Tenant client that owns the portal app
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deletePortalApp(string $id, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeletePortalApp($id, $queryParameters), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdatePortalAppRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PortalAppResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updatePortalApp(string $id, ?\FlowCatalyst\Generated\Model\UpdatePortalAppRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdatePortalApp($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\AssignUnassignedBody $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AssignUnassignedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function assignUnassignedPortalUsers(string $id, ?\FlowCatalyst\Generated\Model\AssignUnassignedBody $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\AssignUnassignedPortalUsers($id, $requestBody), $fetch);
    }
    /**
     * @param array{
     *    "clientId"?: string, //Tenant client whose portal identities to list
     *    "q"?: string, //Prefix (TERM%) matched case-insensitively against email and name
     *    "portalAppCode"?: string, //Only identities granted this portal app
     *    "unassigned"?: bool, //Only identities granted no portal app (cannot be combined with portalAppCode)
     *    "page"?: int, //0-based page index (default 0)
     *    "size"?: int, //Page size (default 100, max 1000)
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PortalUserListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listPortalUsers(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListPortalUsers($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\PortalUserRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PortalUserResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function ensurePortalUser(?\FlowCatalyst\Generated\Model\PortalUserRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\EnsurePortalUser($requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param array{
     *    "clientId"?: string, //Tenant client whose portal identity to delete
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deletePortalUser(string $id, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeletePortalUser($id, $queryParameters), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\PortalUserClientBody $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function activatePortalUser(string $id, ?\FlowCatalyst\Generated\Model\PortalUserClientBody $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ActivatePortalUser($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\PortalUserAppGrantBody $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function grantPortalUserApp(string $id, ?\FlowCatalyst\Generated\Model\PortalUserAppGrantBody $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GrantPortalUserApp($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $portalAppCode
     * @param array{
     *    "clientId"?: string, //Tenant client that owns the portal identity and app
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function revokePortalUserApp(string $id, string $portalAppCode, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\RevokePortalUserApp($id, $portalAppCode, $queryParameters), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\PortalUserClientBody $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deactivatePortalUser(string $id, ?\FlowCatalyst\Generated\Model\PortalUserClientBody $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeactivatePortalUser($id, $requestBody), $fetch);
    }
    /**
     * @param array{
     *    "type"?: string, //Filter by principal type (USER or SERVICE)
     *    "clientId"?: string, //Filter to principals homed at, or granted access to, this client
     *    "active"?: string, //Filter by active status (true/false); absent = both
     *    "q"?: string, //Case-insensitive substring search across name and email
     *    "roles"?: string, //CSV of role names; matches principals holding any of them
     *    "page"?: int, //0-based page index (default 0)
     *    "pageSize"?: int, //Page size; <=0 returns all matches (default: all)
     *    "sortField"?: string, //Sort key: name | email | createdAt (default createdAt)
     *    "sortOrder"?: string, //Sort direction: asc | desc (default asc)
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PrincipalListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listPrincipals(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListPrincipals($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreatePrincipalRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatePrincipalResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createPrincipal(?\FlowCatalyst\Generated\Model\CreatePrincipalRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreatePrincipal($requestBody), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\BulkImportRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\BulkImportResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function bulkImportUsers(?\FlowCatalyst\Generated\Model\BulkImportRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\BulkImportUsers($requestBody), $fetch);
    }
    /**
     * @param array{
     *    "email"?: string,
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CheckEmailDomainResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function checkPrincipalEmailDomain(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CheckPrincipalEmailDomain($queryParameters), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\DeveloperUserListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listDeveloperUsers(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListDeveloperUsers(), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\SyncUsersRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncUsersResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function syncUsers(?\FlowCatalyst\Generated\Model\SyncUsersRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SyncUsers($requestBody), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateUserRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PrincipalResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createUser(?\FlowCatalyst\Generated\Model\CreateUserRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateUser($requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deletePrincipal(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeletePrincipal($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PrincipalResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getPrincipal(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetPrincipal($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdatePrincipalRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PrincipalResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updatePrincipal(string $id, ?\FlowCatalyst\Generated\Model\UpdatePrincipalRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdatePrincipal($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function activatePrincipal(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ActivatePrincipal($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ApplicationAccessListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listPrincipalApplicationAccess(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListPrincipalApplicationAccess($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\AssignApplicationAccessRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SetApplicationAccessResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function assignPrincipalApplicationAccess(string $id, ?\FlowCatalyst\Generated\Model\AssignApplicationAccessRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\AssignPrincipalApplicationAccess($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PrincipalAvailableApplicationsResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listPrincipalAvailableApplications(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListPrincipalAvailableApplications($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientAccessGrantListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listPrincipalClientAccess(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListPrincipalClientAccess($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\GrantClientAccessRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ClientAccessGrantResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function grantPrincipalClientAccess(string $id, ?\FlowCatalyst\Generated\Model\GrantClientAccessRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GrantPrincipalClientAccess($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $clientId
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function revokePrincipalClientAccess(string $id, string $clientId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\RevokePrincipalClientAccess($id, $clientId), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\ClientAssociationRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PrincipalResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function setPrincipalClientAssociation(string $id, ?\FlowCatalyst\Generated\Model\ClientAssociationRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SetPrincipalClientAssociation($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deactivatePrincipal(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeactivatePrincipal($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function revokePrincipalDeveloperCredential(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\RevokePrincipalDeveloperCredential($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SetDeveloperCredentialResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function setPrincipalDeveloperCredential(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SetPrincipalDeveloperCredential($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function resetPrincipalTwoFactor(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ResetPrincipalTwoFactor($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\ResetPasswordRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function resetPrincipalPassword(string $id, ?\FlowCatalyst\Generated\Model\ResetPasswordRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ResetPrincipalPassword($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PrincipalRoleListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listPrincipalRoles(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListPrincipalRoles($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\AddRoleRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PrincipalResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function addPrincipalRole(string $id, ?\FlowCatalyst\Generated\Model\AddRoleRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\AddPrincipalRole($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\AssignPrincipalRolesRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RolesAssignedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function assignPrincipalRoles(string $id, ?\FlowCatalyst\Generated\Model\AssignPrincipalRolesRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\AssignPrincipalRoles($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $role
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PrincipalResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function removePrincipalRole(string $id, string $role, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\RemovePrincipalRole($id, $role), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\SendPasswordResetInputBody $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function sendPrincipalPasswordReset(string $id, ?\FlowCatalyst\Generated\Model\SendPasswordResetInputBody $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SendPrincipalPasswordReset($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PrincipalVersionResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getPrincipalVersion(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetPrincipalVersion($id), $fetch);
    }
    /**
     * @param array{
     *    "application"?: string,
     *    "subdomain"?: string,
     *    "status"?: string,
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ProcessListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listProcesses(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListProcesses($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateProcessRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createProcess(?\FlowCatalyst\Generated\Model\CreateProcessRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateProcess($requestBody), $fetch);
    }
    /**
     * @param string $code
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ProcessResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getProcessByCode(string $code, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetProcessByCode($code), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\SyncProcessesByBodyRequest $requestBody
     * @param array{
     *    "removeUnlisted"?: bool, //Remove API/CODE processes not in the list
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SyncResultResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function syncProcessesByBody(?\FlowCatalyst\Generated\Model\SyncProcessesByBodyRequest $requestBody = null, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\SyncProcessesByBody($requestBody, $queryParameters), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteProcess(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteProcess($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ProcessResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getProcess(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetProcess($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdateProcessRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updateProcess(string $id, ?\FlowCatalyst\Generated\Model\UpdateProcessRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdateProcess($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function archiveProcess(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ArchiveProcess($id), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ListOutputBody|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listResetApprovals(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListResetApprovals(), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function approveResetApproval(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ApproveResetApproval($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\StatusChangeResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function denyResetApproval(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DenyResetApproval($id), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RoleListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listRoles(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListRoles(), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateRoleRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createRole(?\FlowCatalyst\Generated\Model\CreateRoleRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateRole($requestBody), $fetch);
    }
    /**
     * @param string $applicationId
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RoleResponse[]|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getRolesByApplication(string $applicationId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetRolesByApplication($applicationId), $fetch);
    }
    /**
     * @param string $code
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RoleResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getRoleByCode(string $code, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetRoleByCode($code), $fetch);
    }
    /**
     * @param string $source
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RoleResponse[]|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getRolesBySource(string $source, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetRolesBySource($source), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ApplicationFilterListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getRoleApplicationFilters(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetRoleApplicationFilters(), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PermissionListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listPermissions(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListPermissions(), $fetch);
    }
    /**
     * @param string $permission
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deletePermission(string $permission, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeletePermission($permission), $fetch);
    }
    /**
     * @param string $permission
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\PermissionResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getPermission(string $permission, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetPermission($permission), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteRole(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteRole($id), $fetch);
    }
    /**
     * @param string $id Role id (TSID)
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RoleResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getRole(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetRole($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdateRoleRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updateRole(string $id, ?\FlowCatalyst\Generated\Model\UpdateRoleRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdateRole($id, $requestBody), $fetch);
    }
    /**
     * @param string $roleName
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RolePermissionListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listRolePermissions(string $roleName, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListRolePermissions($roleName), $fetch);
    }
    /**
     * @param string $roleName
     * @param null|\FlowCatalyst\Generated\Model\GrantPermissionRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RoleResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function grantRolePermissionByBody(string $roleName, ?\FlowCatalyst\Generated\Model\GrantPermissionRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GrantRolePermissionByBody($roleName, $requestBody), $fetch);
    }
    /**
     * @param string $roleName
     * @param string $permission
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RoleResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function revokeRolePermission(string $roleName, string $permission, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\RevokeRolePermission($roleName, $permission), $fetch);
    }
    /**
     * @param string $roleName
     * @param string $permission
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RoleResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function grantRolePermission(string $roleName, string $permission, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GrantRolePermission($roleName, $permission), $fetch);
    }
    /**
     * @param array{
     *    "status"?: string,
     *    "clientId"?: string,
     *    "search"?: string,
     *    "page"?: int,
     *    "size"?: int,
     *    "limit"?: int,
     *    "pageSize"?: int,
     *    "page_size"?: int,
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\OffsetPageScheduledJobResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listScheduledJobs(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListScheduledJobs($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateScheduledJobRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createScheduledJob(?\FlowCatalyst\Generated\Model\CreateScheduledJobRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateScheduledJob($requestBody), $fetch);
    }
    /**
     * @param string $code
     * @param array{
     *    "clientId"?: string, //Optional client scope; omit for platform-scoped lookup
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ScheduledJobResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getScheduledJobByCode(string $code, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetScheduledJobByCode($code, $queryParameters), $fetch);
    }
    /**
     * @param string $instanceId
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ScheduledJobInstanceResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getScheduledJobInstance(string $instanceId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetScheduledJobInstance($instanceId), $fetch);
    }
    /**
     * @param string $instanceId
     * @param null|\FlowCatalyst\Generated\Model\CompleteInstanceRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function completeScheduledJobInstance(string $instanceId, ?\FlowCatalyst\Generated\Model\CompleteInstanceRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CompleteScheduledJobInstance($instanceId, $requestBody), $fetch);
    }
    /**
     * @param string $instanceId
     * @param null|\FlowCatalyst\Generated\Model\WriteInstanceLogRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function writeScheduledJobInstanceLog(string $instanceId, ?\FlowCatalyst\Generated\Model\WriteInstanceLogRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\WriteScheduledJobInstanceLog($instanceId, $requestBody), $fetch);
    }
    /**
     * @param string $instanceId
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ScheduledJobInstanceLogResponse[]|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listScheduledJobInstanceLogs(string $instanceId, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListScheduledJobInstanceLogs($instanceId), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteScheduledJob(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteScheduledJob($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ScheduledJobResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getScheduledJob(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetScheduledJob($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdateScheduledJobRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updateScheduledJob(string $id, ?\FlowCatalyst\Generated\Model\UpdateScheduledJobRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdateScheduledJob($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function archiveScheduledJob(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ArchiveScheduledJob($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\FireNowRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\FireNowResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function fireScheduledJobNow(string $id, ?\FlowCatalyst\Generated\Model\FireNowRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\FireScheduledJobNow($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param array{
     *    "status"?: string,
     *    "page"?: int,
     *    "size"?: int,
     *    "limit"?: int,
     *    "pageSize"?: int,
     *    "page_size"?: int,
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\OffsetPageScheduledJobInstanceResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listScheduledJobInstances(string $id, array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListScheduledJobInstances($id, $queryParameters), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function pauseScheduledJob(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PauseScheduledJob($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function resumeScheduledJob(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ResumeScheduledJob($id), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ServiceAccountListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listServiceAccounts(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListServiceAccounts(), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateServiceAccountRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreateServiceAccountResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createServiceAccount(?\FlowCatalyst\Generated\Model\CreateServiceAccountRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateServiceAccount($requestBody), $fetch);
    }
    /**
     * @param string $code
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ServiceAccountResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getServiceAccountByCode(string $code, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetServiceAccountByCode($code), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteServiceAccount(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteServiceAccount($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ServiceAccountResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getServiceAccount(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetServiceAccount($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdateServiceAccountRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updateServiceAccount(string $id, ?\FlowCatalyst\Generated\Model\UpdateServiceAccountRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdateServiceAccount($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deactivateServiceAccount(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeactivateServiceAccount($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RegenerateAuthTokenResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function regenerateServiceAccountAuthTokenRegenerateAuthToken(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\RegenerateServiceAccountAuthTokenRegenerateAuthToken($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RegenerateSigningSecretResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function regenerateServiceAccountSigningSecretRegenerateSecret(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\RegenerateServiceAccountSigningSecretRegenerateSecret($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RegenerateSigningSecretResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function regenerateServiceAccountSigningSecretRegenerateSigningSecret(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\RegenerateServiceAccountSigningSecretRegenerateSigningSecret($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RegenerateAuthTokenResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function regenerateServiceAccountAuthTokenRegenerateToken(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\RegenerateServiceAccountAuthTokenRegenerateToken($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ServiceAccountRoleListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listServiceAccountRoles(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListServiceAccountRoles($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\AssignRolesRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ServiceAccountRolesAssignedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function assignServiceAccountRoles(string $id, ?\FlowCatalyst\Generated\Model\AssignRolesRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\AssignServiceAccountRoles($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ServiceAccountTokenResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function mintServiceAccountToken(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\MintServiceAccountToken($id), $fetch);
    }
    /**
     * @param array{
     *    "status"?: string,
     *    "clientId"?: string,
     * } $queryParameters
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SubscriptionListResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listSubscriptions(array $queryParameters = [], string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListSubscriptions($queryParameters), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\CreateSubscriptionRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\CreatedResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function createSubscription(?\FlowCatalyst\Generated\Model\CreateSubscriptionRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\CreateSubscription($requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteSubscription(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteSubscription($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\SubscriptionResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function getSubscription(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\GetSubscription($id), $fetch);
    }
    /**
     * @param string $id
     * @param null|\FlowCatalyst\Generated\Model\UpdateSubscriptionRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function updateSubscription(string $id, ?\FlowCatalyst\Generated\Model\UpdateSubscriptionRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\UpdateSubscription($id, $requestBody), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function pauseSubscription(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\PauseSubscription($id), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function resumeSubscription(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ResumeSubscription($id), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\AuthenticateBeginRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\AuthenticateBeginResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function webauthnAuthenticateBegin(?\FlowCatalyst\Generated\Model\AuthenticateBeginRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\WebauthnAuthenticateBegin($requestBody), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\AuthenticateCompleteRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\WebauthnAuthenticateCompleteResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function webauthnAuthenticateComplete(?\FlowCatalyst\Generated\Model\AuthenticateCompleteRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\WebauthnAuthenticateComplete($requestBody), $fetch);
    }
    /**
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\WebauthnCredentialSummary[]|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function listWebauthnCredentials(string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\ListWebauthnCredentials(), $fetch);
    }
    /**
     * @param string $id
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function deleteWebauthnCredential(string $id, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\DeleteWebauthnCredential($id), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\RegisterBeginRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RegisterBeginResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function webauthnRegisterBegin(?\FlowCatalyst\Generated\Model\RegisterBeginRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\WebauthnRegisterBegin($requestBody), $fetch);
    }
    /**
     * @param null|\FlowCatalyst\Generated\Model\RegisterCompleteRequest $requestBody
     * @param string $fetch Fetch mode to use (can be OBJECT or RESPONSE)
     *
     * @return ($fetch is 'object' ? null|\FlowCatalyst\Generated\Model\RegisterCompleteResponse|\FlowCatalyst\Generated\Model\ErrorModel : \Psr\Http\Message\ResponseInterface)
     */
    public function webauthnRegisterComplete(?\FlowCatalyst\Generated\Model\RegisterCompleteRequest $requestBody = null, string $fetch = self::FETCH_OBJECT)
    {
        return $this->executeEndpoint(new \FlowCatalyst\Generated\Endpoint\WebauthnRegisterComplete($requestBody), $fetch);
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