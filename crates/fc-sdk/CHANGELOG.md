# Changelog — fc-sdk (Rust)

fc-sdk takes the workspace version and has never been published. The
platform API client (`fc_sdk::client`, feature `client`) now sends and reads
what Go's platform API expects (Go's `api/openapi.lock.json`, mirrored at
`frontend/openapi/openapi.json`). The Rust platform is being aligned to the
same shapes. Where Go answers `{ id }` or `204`, the methods now say so
instead of decoding a full entity; those calls failed with a decode error
before.

## Unreleased

### Added
- `Applications::get_client_config(id, client_id)`: Go's
  `GET /api/applications/{id}/clients/{clientId}`.
- `ApplicationProvisionServiceAccountResponse`,
  `ApplicationServiceAccountCredentials`, `ApplicationOAuthClientCredentials`:
  Go's nested provision response.
- `EventTypes::delete(id)`: `DELETE /api/event-types/{id}`.
- `SyncEventTypeItem`: the event-type sync item, carrying only what Go's strict
  `SyncEventTypeInputRequest` accepts (`code`, `name`, `description`).
- `DispatchPoolListResponse` (`{ pools, total }`), `NoteResponse`
  (`ClientResponse::notes`).
- Response members Go sends: `ServiceAccount::principal_id` and
  `oauth_client_id`; `EventTypeResponse::source`, `client_id` and
  `created_by`; `SpecVersionResponse::created_at`;
  `ConnectionResponse::source` and `application_code`;
  `DispatchPoolResponse::client_identifier`; `RoleResponse::application_id`;
  `AuditLogResponse::operation_json`; `ScheduledJobResponse::application_id`;
  `InstanceLogResponse::scheduled_job_id` and `client_id`;
  `FireResponse::id` and `scheduled_job_id`; top-level `pool_code` and
  `queue_id` on `InPipelineCheckResponse`.
- Request members Go accepts: `UpdateEventTypeRequest::client_scoped`,
  `UpdateConnectionRequest::application_code`, `UpdateRoleRequest::permissions`,
  `UpdatePrincipalRequest::email`.
- List query values are form-encoded (an email's `+` no longer becomes a
  space).
- Dispatch jobs carry a `descriptor` (what the job is, in words, e.g.
  "Notify Value of user logins", shown in the platform's dispatch-jobs grid;
  at most 255 characters, the platform answers 400 `VALIDATION` beyond) and a
  `queue` (the job's own dispatch priority, `DEFAULT` or `HIGH_PRIORITY`):
  `CreateDispatchJobDto::descriptor(..)` / `::queue(..)` and
  `DispatchJobPayload::descriptor` / `::queue`. Both travel in the outbox
  payload as `descriptor` / `queue` and are left out when unset. Both structs
  gained public fields, so a struct literal of either without
  `..Default::default()` (or `CreateDispatchJobDto::new`) needs the two
  fields added.
- `CreateEventTypeRequest::client_scoped` (Go's `clientScoped` on
  `POST /api/event-types`): events of the type are carried per client. Left
  out when `None`. The struct gained a public field, so a struct literal
  without `..Default::default()` needs it added.

### Changed
- `Applications::get_service_account` reads `GET /api/applications/{id}` and
  then `GET /api/service-accounts/{serviceAccountId}`, because Go has no
  `GET …/service-account`. It returns Go's `ServiceAccountResponse` (now an
  alias of `ServiceAccount`). An application without a service account
  yields `ClientError::Api { status: 404, .. }`.
- `Applications::provision_service_account` returns
  `ApplicationProvisionServiceAccountResponse`, so the one-time client secret
  (`service_account.oauth_client.client_secret`) is no longer lost.
- `Applications::update_client_config` is deprecated: only the Rust platform
  serves that PUT.
- `Applications::list(active, application_type)`: Go's `active` and `type`
  filters. The `page` / `page_size` parameters were removed; Go does not
  paginate this list.
- `ClientConfigResponse` is Go's shape: `config_json` (the older `config` is
  still read), `created_at`, `updated_at`. `client_name`,
  `client_identifier` and `effective_base_url` were removed; Go never sends
  them. `ClientConfigsResponse::items` still accepts `clientConfigs`.
- `Clients::list()` takes no parameters: Go's `GET /api/clients` has no
  filters or paging. Use `Clients::search`.
- `Connections::list(client_id, status)`: `service_account_id` was removed
  (Go has no such filter).
- `Processes::list(application, subdomain, status)`: `search` was removed (Go
  has no such filter).
- `PrincipalFilters::email` is replaced by `q` (Go's substring search over
  name and email). `Principals::find_by_email` sends `q` and keeps only the
  exact (case-insensitive) email matches.
- `UpdatePrincipalRequest` drops `first_name` / `last_name`, which Go ignores.
- `UpdateEventTypeRequest::name` and `UpdateConnectionRequest::name` are
  required `String`s and always sent: Go replaces the record and requires
  `name`.
- `AddSchemaVersionRequest` gains a required `version`; Go requires it.
- `SyncEventTypesRequest::event_types` is `Vec<SyncEventTypeItem>`, so a sync
  can no longer send `schema` / `clientId`, which Go's strict item rejects.
- `SyncDispatchPoolItem::name` is a required `String`; Go's strict item
  requires it.
- `EventTypeResponse::event_name` reads Go's `eventName` (the older `event`
  is still read).
- `PermissionResponse` is Go's shape: `permission`, `name`, `description`,
  `category`. The `application` / `context` / `aggregate` / `action` members
  were removed; Go never sends them.
- Audit logs page by cursor, as Go does. `AuditLogFilters` has `after`,
  `page_size`, `application_ids` and `client_ids` (sent as CSV
  `applicationIds` / `clientIds`); `client_id`, `from`, `to` and `page` were
  removed. `AuditLogListResponse` has `has_more` and `next_cursor` in place
  of `total` / `page` / `page_size`.
- Scheduled-job list pages read Go's `total_pages` (the older `totalPages`
  is still read). `InstanceFilters` keeps `status`, `page` and `size`;
  `trigger_kind`, `from` and `to` were removed.
- `DispatchPools::list` returns `DispatchPoolListResponse` (Go's `pools`).
- `EventTypes::list` sends `clientId` (it sent `client_id`, which Go ignores);
  `Subscriptions::list` likewise.
- `Roles::list_for_application` returns `Vec<RoleResponse>` (Go's bare
  array); `ScheduledJobs::list_instance_logs` returns
  `Vec<InstanceLogResponse>` (`InstanceLogListResponse` was removed).
- Creates that answer `{ id }` on Go return `CreatedResponse`:
  `EventTypes::create`, `DispatchPools::create`, `Processes::create`,
  `Subscriptions::create`.
- Calls that answer `204` on Go return `()`: `Applications::update`,
  `Clients::update`, `Clients::enable_application`,
  `Clients::disable_application`, `Clients::update_applications`,
  `Connections::update`, `DispatchPools::update`, `EventTypes::update`,
  `Roles::update`, `Subscriptions::update`, `ScheduledJobs::update`,
  `pause`, `resume`, `archive`, `log_for_instance` and `complete_instance`.
  The scheduled-job runner's log and completion callbacks no longer report
  a spurious `CallbackFailed`.

### Deprecated
- `EventTypes::archive`: Go has no archive route for event types. It sends
  `DELETE /api/event-types/{id}`, which Go treats as a delete; use
  `EventTypes::delete`.
- `Applications::update_client_config` (see above).
