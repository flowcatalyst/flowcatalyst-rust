# Changelog

## Unreleased

The `client` package now follows the Go platform's API contract
(`flowcatalyst-go` `api/openapi.lock.json`). The SDK has not been
published, so the breaking changes below affect no released version.

### Changed routes

- `Applications.GetServiceAccount` no longer calls
  `GET /api/applications/{id}/service-account`, which the platform lacks.
  It reads the application and then `GET /api/service-accounts/{serviceAccountId}`,
  and returns the platform's `ServiceAccountResponse`. It returns an
  `*APIError` with status 404 and code `SERVICE_ACCOUNT_NOT_FOUND` when the
  application has no service account.
- `Applications.UpdateClientConfig` (`PUT …/clients/{clientId}`) is
  deprecated: only the Rust platform serves it. New `Applications.GetClientConfig`
  (`GET …/clients/{clientId}`).
- `EventTypes.Archive` is removed. The platform has no archive route for event
  types, and `DELETE /api/event-types/{id}` is a hard delete. It is available
  as `EventTypes.Delete`. To retire event types, drop them from the
  definitions and sync with `removeUnlisted`.
- `Processes.Archive` and `DispatchPools.Archive` send `POST …/{id}/archive`
  (it was `DELETE`, a hard delete). New `DispatchPools.Delete` (hard delete).
  `Processes.Delete` no longer sends the ignored `?hard=true`.
- New `Applications.AttachServiceAccount` (`POST …/{id}/service-account`) and
  `ServiceAccounts.Get`.

### Request shapes

- `UpdateEventTypeRequest.Name` and `UpdateConnectionRequest.Name` are now
  `string` and always sent. The platform requires the name on every update.
  Added `ClientScoped` (event types) and `ApplicationCode` (connections).
- Processes model `Body`, `DiagramType` and `Tags` instead of `Steps` (create,
  update, response, sync item). In `sync`, `ProcessDefinition` has
  `WithBody`, `WithDiagramType` and `WithTags`, and `WithSteps` is removed.
  `UpdateProcessRequest.Tags` is `*[]string`, so an empty list clears the tags.
- `SyncEventTypesRequest.EventTypes` is `[]SyncEventTypeItem`
  (code/name/description). The platform's strict sync item rejects `schema` and
  `clientId`.
- `Principals.FindByEmail` sends `q` and keeps only exact (case-insensitive)
  email matches. `PrincipalFilters` drops `Email` and gains `Q`, `Roles`,
  `Page`, `PageSize`, `SortField` and `SortOrder`. `UpdatePrincipalRequest`
  drops `FirstName`/`LastName`, which the platform ignores, and gains `Email`.
- Audit logs use cursor paging. `AuditLogFilters` has `After`, `PageSize`,
  `ApplicationIDs` and `ClientIDs` (sent as CSV); `ClientID`, `From`, `To` and
  `Page` are removed. `AuditLogListResponse` has `HasMore` and `NextCursor`.
- List filters the platform does not support are removed:
  - `Applications.List(ctx, active, applicationType)`: page and page size are
    gone, and there is a new type filter.
  - `Clients.List(ctx)` takes no filters.
  - `Connections.List(ctx, clientID, status)`: `serviceAccountId` is gone.
  - `Processes.List(ctx, *ProcessFilters)` filters by application, subdomain
    and status. `clientId` is gone.
  - `InstanceFilters` drops `TriggerKind`, `From` and `To`.
- Added optional members the platform accepts: `rateLimit` (dispatch pools),
  `website`/`logo`/`logoMimeType` (applications), `applicationId` (service
  accounts, scheduled jobs) and `permissions` on `UpdateRoleRequest`.

### Response shapes

- Creates that answer `{id}` now return `*CreatedResponse`:
  `EventTypes.Create`, `Processes.Create`, `DispatchPools.Create` and
  `Subscriptions.Create`. `Connections.Create` returns the created
  `*ConnectionResponse`.
- Calls answered with 204 now return only `error`:
  - `Applications.Update`, `EnableForClient` and `DisableForClient`.
  - `Clients.Update`, `EnableApplication`, `DisableApplication` and
    `UpdateApplications`.
  - `EventTypes.Update`, `Processes.Update` and `Subscriptions.Update`.
  - `DispatchPools.Update`, `Suspend` and `Activate`.
  - `ScheduledJobs.Update`, `Pause`, `Resume`, `Archive`, `LogForInstance` and
    `CompleteInstance`.
- `ProvisionServiceAccount` returns `ApplicationProvisionServiceAccountResponse`,
  with the one-time secret in `ServiceAccount.OAuthClient.ClientSecret`.
- `Applications.ListRoles` returns `[]string` (role names from `{roles:[…]}`).
- `Applications.ListClients` returns `ClientConfigListResponse` (`items`).
  `ClientConfigResponse` has `ConfigJSON` (`configJson`), `CreatedAt` and
  `UpdatedAt`.
- `DispatchPoolListResponse` reads `pools` and `total`.
- Scheduled-job list responses read `total_pages`.
  `FireResponse` gains `ID` and `ScheduledJobID`.
- `PermissionResponse` is `permission`/`name`/`category`/`description`.
- `EventTypeResponse` reads `eventName` (it was `event`) and gains `Source`,
  `ClientID` and `CreatedBy`. `SpecVersionResponse` gains `CreatedAt`.
- `Router.InPipeline` reads `poolCode`/`queueId` at the top level. The nested
  `Detail` is deprecated, and it is still lifted to the top level when an
  older router sends it.
- Additive response members: `hasLoginClient`, `website`, `logo` and
  `logoMimeType` (applications); `notes` (clients); `source` and
  `applicationCode` (connections); `rateLimit` and `clientIdentifier`
  (dispatch pools); `applicationId` (roles, scheduled jobs); `operationJson`
  (audit logs); `principalId` and `oauthClientId` (service accounts).
