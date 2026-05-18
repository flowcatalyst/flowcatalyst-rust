<?php

namespace FlowCatalyst\Generated\Normalizer;

use FlowCatalyst\Generated\Runtime\Normalizer\CheckArray;
use FlowCatalyst\Generated\Runtime\Normalizer\ValidatorTrait;
use Symfony\Component\Serializer\Normalizer\DenormalizerAwareInterface;
use Symfony\Component\Serializer\Normalizer\DenormalizerAwareTrait;
use Symfony\Component\Serializer\Normalizer\DenormalizerInterface;
use Symfony\Component\Serializer\Normalizer\NormalizerAwareInterface;
use Symfony\Component\Serializer\Normalizer\NormalizerAwareTrait;
use Symfony\Component\Serializer\Normalizer\NormalizerInterface;
class JaneObjectNormalizer implements DenormalizerInterface, NormalizerInterface, DenormalizerAwareInterface, NormalizerAwareInterface
{
    use DenormalizerAwareTrait;
    use NormalizerAwareTrait;
    use CheckArray;
    use ValidatorTrait;
    protected $normalizers = [
        
        \FlowCatalyst\Generated\Model\AddNoteRequest::class => \FlowCatalyst\Generated\Normalizer\AddNoteRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AddNoteResponse::class => \FlowCatalyst\Generated\Normalizer\AddNoteResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AddSchemaVersionRequest::class => \FlowCatalyst\Generated\Normalizer\AddSchemaVersionRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AggregatesResponse::class => \FlowCatalyst\Generated\Normalizer\AggregatesResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AllFilterOptions::class => \FlowCatalyst\Generated\Normalizer\AllFilterOptionsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationAccessListResponse::class => \FlowCatalyst\Generated\Normalizer\ApplicationAccessListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationAccessResponse::class => \FlowCatalyst\Generated\Normalizer\ApplicationAccessResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationIdsResponse::class => \FlowCatalyst\Generated\Normalizer\ApplicationIdsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationOption::class => \FlowCatalyst\Generated\Normalizer\ApplicationOptionNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationOptionsResponse::class => \FlowCatalyst\Generated\Normalizer\ApplicationOptionsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationsResponse::class => \FlowCatalyst\Generated\Normalizer\ApplicationsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AssignRoleRequest::class => \FlowCatalyst\Generated\Normalizer\AssignRoleRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuditLogDetailResponse::class => \FlowCatalyst\Generated\Normalizer\AuditLogDetailResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuditLogListResponse::class => \FlowCatalyst\Generated\Normalizer\AuditLogListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuditLogResponse::class => \FlowCatalyst\Generated\Normalizer\AuditLogResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuthenticateBeginRequest::class => \FlowCatalyst\Generated\Normalizer\AuthenticateBeginRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuthenticateBeginResponse::class => \FlowCatalyst\Generated\Normalizer\AuthenticateBeginResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuthenticateCompleteRequest::class => \FlowCatalyst\Generated\Normalizer\AuthenticateCompleteRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuthenticateCompleteResponse::class => \FlowCatalyst\Generated\Normalizer\AuthenticateCompleteResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AvailableApplicationResponse::class => \FlowCatalyst\Generated\Normalizer\AvailableApplicationResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AvailableApplicationsResponse::class => \FlowCatalyst\Generated\Normalizer\AvailableApplicationsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\BatchAssignRolesRequest::class => \FlowCatalyst\Generated\Normalizer\BatchAssignRolesRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\BatchAssignRolesResponse::class => \FlowCatalyst\Generated\Normalizer\BatchAssignRolesResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\BatchCreateDispatchJobsRequest::class => \FlowCatalyst\Generated\Normalizer\BatchCreateDispatchJobsRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\BatchCreateDispatchJobsResponse::class => \FlowCatalyst\Generated\Normalizer\BatchCreateDispatchJobsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\BatchCreateEventsRequest::class => \FlowCatalyst\Generated\Normalizer\BatchCreateEventsRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\BatchCreateResponse::class => \FlowCatalyst\Generated\Normalizer\BatchCreateResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CheckEmailDomainResponse::class => \FlowCatalyst\Generated\Normalizer\CheckEmailDomainResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CircuitBreakerState::class => \FlowCatalyst\Generated\Normalizer\CircuitBreakerStateNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CircuitBreakersResponse::class => \FlowCatalyst\Generated\Normalizer\CircuitBreakersResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientAccessGrantResponse::class => \FlowCatalyst\Generated\Normalizer\ClientAccessGrantResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientAccessListResponse::class => \FlowCatalyst\Generated\Normalizer\ClientAccessListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientApplicationResponse::class => \FlowCatalyst\Generated\Normalizer\ClientApplicationResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientApplicationsResponse::class => \FlowCatalyst\Generated\Normalizer\ClientApplicationsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientFilterOptions::class => \FlowCatalyst\Generated\Normalizer\ClientFilterOptionsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientIdsResponse::class => \FlowCatalyst\Generated\Normalizer\ClientIdsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientListResponse::class => \FlowCatalyst\Generated\Normalizer\ClientListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientResponse::class => \FlowCatalyst\Generated\Normalizer\ClientResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClusterMember::class => \FlowCatalyst\Generated\Normalizer\ClusterMemberNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ConfigEntryResponse::class => \FlowCatalyst\Generated\Normalizer\ConfigEntryResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ContextDataDto::class => \FlowCatalyst\Generated\Normalizer\ContextDataDtoNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateClientRequest::class => \FlowCatalyst\Generated\Normalizer\CreateClientRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateDispatchJobRequest::class => \FlowCatalyst\Generated\Normalizer\CreateDispatchJobRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateEventRequest::class => \FlowCatalyst\Generated\Normalizer\CreateEventRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateEventResponse::class => \FlowCatalyst\Generated\Normalizer\CreateEventResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateEventTypeRequest::class => \FlowCatalyst\Generated\Normalizer\CreateEventTypeRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateOAuthClientRequest::class => \FlowCatalyst\Generated\Normalizer\CreateOAuthClientRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateOAuthClientResponse::class => \FlowCatalyst\Generated\Normalizer\CreateOAuthClientResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateProcessRequest::class => \FlowCatalyst\Generated\Normalizer\CreateProcessRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateRoleRequest::class => \FlowCatalyst\Generated\Normalizer\CreateRoleRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateScheduledJobRequest::class => \FlowCatalyst\Generated\Normalizer\CreateScheduledJobRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateSubscriptionRequest::class => \FlowCatalyst\Generated\Normalizer\CreateSubscriptionRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateUserRequest::class => \FlowCatalyst\Generated\Normalizer\CreateUserRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreatedResponse::class => \FlowCatalyst\Generated\Normalizer\CreatedResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CredentialSummary::class => \FlowCatalyst\Generated\Normalizer\CredentialSummaryNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CurrentUserResponse::class => \FlowCatalyst\Generated\Normalizer\CurrentUserResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DashboardMetrics::class => \FlowCatalyst\Generated\Normalizer\DashboardMetricsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DispatchAttemptResponse::class => \FlowCatalyst\Generated\Normalizer\DispatchAttemptResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DispatchJobFilterOptionsResponse::class => \FlowCatalyst\Generated\Normalizer\DispatchJobFilterOptionsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DispatchJobReadResponse::class => \FlowCatalyst\Generated\Normalizer\DispatchJobReadResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DispatchJobResponse::class => \FlowCatalyst\Generated\Normalizer\DispatchJobResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DispatchJobsFilterOptions::class => \FlowCatalyst\Generated\Normalizer\DispatchJobsFilterOptionsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DispatchPoolFilterOptions::class => \FlowCatalyst\Generated\Normalizer\DispatchPoolFilterOptionsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DomainCheckResponse::class => \FlowCatalyst\Generated\Normalizer\DomainCheckResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EnhancedPoolMetrics::class => \FlowCatalyst\Generated\Normalizer\EnhancedPoolMetricsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EntityAuditLogsResponse::class => \FlowCatalyst\Generated\Normalizer\EntityAuditLogsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EntityTypesResponse::class => \FlowCatalyst\Generated\Normalizer\EntityTypesResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ErrorResponse::class => \FlowCatalyst\Generated\Normalizer\ErrorResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventFilterOptions::class => \FlowCatalyst\Generated\Normalizer\EventFilterOptionsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventRead::class => \FlowCatalyst\Generated\Normalizer\EventReadNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventResponse::class => \FlowCatalyst\Generated\Normalizer\EventResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventSummaryResponse::class => \FlowCatalyst\Generated\Normalizer\EventSummaryResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventTypeBindingRequest::class => \FlowCatalyst\Generated\Normalizer\EventTypeBindingRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventTypeBindingResponse::class => \FlowCatalyst\Generated\Normalizer\EventTypeBindingResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventTypeFilterOptions::class => \FlowCatalyst\Generated\Normalizer\EventTypeFilterOptionsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventTypeListResponse::class => \FlowCatalyst\Generated\Normalizer\EventTypeListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventTypeResponse::class => \FlowCatalyst\Generated\Normalizer\EventTypeResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventsFilterOptions::class => \FlowCatalyst\Generated\Normalizer\EventsFilterOptionsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\FilterOption::class => \FlowCatalyst\Generated\Normalizer\FilterOptionNormalizer::class,
        
        \FlowCatalyst\Generated\Model\FireRequest::class => \FlowCatalyst\Generated\Normalizer\FireRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\GrantClientAccessRequest::class => \FlowCatalyst\Generated\Normalizer\GrantClientAccessRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\GrantPermissionRequest::class => \FlowCatalyst\Generated\Normalizer\GrantPermissionRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\InFlightMessage::class => \FlowCatalyst\Generated\Normalizer\InFlightMessageNormalizer::class,
        
        \FlowCatalyst\Generated\Model\InFlightMessagesResponse::class => \FlowCatalyst\Generated\Normalizer\InFlightMessagesResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\InstanceCompleteRequest::class => \FlowCatalyst\Generated\Normalizer\InstanceCompleteRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\InstanceLogRequest::class => \FlowCatalyst\Generated\Normalizer\InstanceLogRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\InstanceLogResponse::class => \FlowCatalyst\Generated\Normalizer\InstanceLogResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\LoginRequest::class => \FlowCatalyst\Generated\Normalizer\LoginRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\LoginResponse::class => \FlowCatalyst\Generated\Normalizer\LoginResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\OAuthClientListResponse::class => \FlowCatalyst\Generated\Normalizer\OAuthClientListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\OAuthClientResponse::class => \FlowCatalyst\Generated\Normalizer\OAuthClientResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\OperationsResponse::class => \FlowCatalyst\Generated\Normalizer\OperationsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PaginatedResponseScheduledJobInstanceResponse::class => \FlowCatalyst\Generated\Normalizer\PaginatedResponseScheduledJobInstanceResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PaginatedResponseScheduledJobInstanceResponseDataItem::class => \FlowCatalyst\Generated\Normalizer\PaginatedResponseScheduledJobInstanceResponseDataItemNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PaginatedResponseScheduledJobResponse::class => \FlowCatalyst\Generated\Normalizer\PaginatedResponseScheduledJobResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PaginatedResponseScheduledJobResponseDataItem::class => \FlowCatalyst\Generated\Normalizer\PaginatedResponseScheduledJobResponseDataItemNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PaginationParams::class => \FlowCatalyst\Generated\Normalizer\PaginationParamsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PermissionListResponse::class => \FlowCatalyst\Generated\Normalizer\PermissionListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PermissionResponse::class => \FlowCatalyst\Generated\Normalizer\PermissionResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PoolStats::class => \FlowCatalyst\Generated\Normalizer\PoolStatsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PoolStatsResponse::class => \FlowCatalyst\Generated\Normalizer\PoolStatsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PrincipalListResponse::class => \FlowCatalyst\Generated\Normalizer\PrincipalListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PrincipalResponse::class => \FlowCatalyst\Generated\Normalizer\PrincipalResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ProcessListResponse::class => \FlowCatalyst\Generated\Normalizer\ProcessListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ProcessResponse::class => \FlowCatalyst\Generated\Normalizer\ProcessResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ProcessingTimeMetrics::class => \FlowCatalyst\Generated\Normalizer\ProcessingTimeMetricsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RefreshTokenRequest::class => \FlowCatalyst\Generated\Normalizer\RefreshTokenRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RegenerateSecretResponse::class => \FlowCatalyst\Generated\Normalizer\RegenerateSecretResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RegisterBeginRequest::class => \FlowCatalyst\Generated\Normalizer\RegisterBeginRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RegisterBeginResponse::class => \FlowCatalyst\Generated\Normalizer\RegisterBeginResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RegisterCompleteRequest::class => \FlowCatalyst\Generated\Normalizer\RegisterCompleteRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RegisterCompleteResponse::class => \FlowCatalyst\Generated\Normalizer\RegisterCompleteResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ResetPasswordRequest::class => \FlowCatalyst\Generated\Normalizer\ResetPasswordRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RoleAssignmentDto::class => \FlowCatalyst\Generated\Normalizer\RoleAssignmentDtoNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RoleListResponse::class => \FlowCatalyst\Generated\Normalizer\RoleListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RoleResponse::class => \FlowCatalyst\Generated\Normalizer\RoleResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RolesListResponse::class => \FlowCatalyst\Generated\Normalizer\RolesListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ScheduledJobInstanceResponse::class => \FlowCatalyst\Generated\Normalizer\ScheduledJobInstanceResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ScheduledJobResponse::class => \FlowCatalyst\Generated\Normalizer\ScheduledJobResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SetApplicationAccessRequest::class => \FlowCatalyst\Generated\Normalizer\SetApplicationAccessRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SetApplicationAccessResponse::class => \FlowCatalyst\Generated\Normalizer\SetApplicationAccessResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SpecVersionResponse::class => \FlowCatalyst\Generated\Normalizer\SpecVersionResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\StandbyStatus::class => \FlowCatalyst\Generated\Normalizer\StandbyStatusNormalizer::class,
        
        \FlowCatalyst\Generated\Model\StatusChangeRequest::class => \FlowCatalyst\Generated\Normalizer\StatusChangeRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\StatusChangeResponse::class => \FlowCatalyst\Generated\Normalizer\StatusChangeResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SubdomainsResponse::class => \FlowCatalyst\Generated\Normalizer\SubdomainsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SubscriptionFilterOptions::class => \FlowCatalyst\Generated\Normalizer\SubscriptionFilterOptionsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SubscriptionListResponse::class => \FlowCatalyst\Generated\Normalizer\SubscriptionListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SubscriptionResponse::class => \FlowCatalyst\Generated\Normalizer\SubscriptionResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SuccessResponse::class => \FlowCatalyst\Generated\Normalizer\SuccessResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncDispatchPoolInputRequest::class => \FlowCatalyst\Generated\Normalizer\SyncDispatchPoolInputRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncDispatchPoolsRequest::class => \FlowCatalyst\Generated\Normalizer\SyncDispatchPoolsRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncEventTypeInputRequest::class => \FlowCatalyst\Generated\Normalizer\SyncEventTypeInputRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncEventTypesRequest::class => \FlowCatalyst\Generated\Normalizer\SyncEventTypesRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncOpenApiSpecRequest::class => \FlowCatalyst\Generated\Normalizer\SyncOpenApiSpecRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncOpenApiSpecResponse::class => \FlowCatalyst\Generated\Normalizer\SyncOpenApiSpecResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncPrincipalInputRequest::class => \FlowCatalyst\Generated\Normalizer\SyncPrincipalInputRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncPrincipalsRequest::class => \FlowCatalyst\Generated\Normalizer\SyncPrincipalsRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncProcessInputRequest::class => \FlowCatalyst\Generated\Normalizer\SyncProcessInputRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncProcessesRequest::class => \FlowCatalyst\Generated\Normalizer\SyncProcessesRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncResultResponse::class => \FlowCatalyst\Generated\Normalizer\SyncResultResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncRoleInputRequest::class => \FlowCatalyst\Generated\Normalizer\SyncRoleInputRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncRolesRequest::class => \FlowCatalyst\Generated\Normalizer\SyncRolesRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncScheduledJobInputRequest::class => \FlowCatalyst\Generated\Normalizer\SyncScheduledJobInputRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncScheduledJobsRequest::class => \FlowCatalyst\Generated\Normalizer\SyncScheduledJobsRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncScheduledJobsResultResponse::class => \FlowCatalyst\Generated\Normalizer\SyncScheduledJobsResultResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncSubscriptionEventTypeRequest::class => \FlowCatalyst\Generated\Normalizer\SyncSubscriptionEventTypeRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncSubscriptionInputRequest::class => \FlowCatalyst\Generated\Normalizer\SyncSubscriptionInputRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncSubscriptionsRequest::class => \FlowCatalyst\Generated\Normalizer\SyncSubscriptionsRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SystemHealth::class => \FlowCatalyst\Generated\Normalizer\SystemHealthNormalizer::class,
        
        \FlowCatalyst\Generated\Model\TokenRefreshResponse::class => \FlowCatalyst\Generated\Normalizer\TokenRefreshResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateClientApplicationsRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateClientApplicationsRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateClientRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateClientRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateEventTypeRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateEventTypeRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateOAuthClientRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateOAuthClientRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdatePrincipalRequest::class => \FlowCatalyst\Generated\Normalizer\UpdatePrincipalRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateProcessRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateProcessRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateRoleRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateRoleRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateScheduledJobRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateScheduledJobRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateSubscriptionRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateSubscriptionRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\WindowedMetrics::class => \FlowCatalyst\Generated\Normalizer\WindowedMetricsNormalizer::class,
        
        \Jane\Component\JsonSchemaRuntime\Reference::class => \FlowCatalyst\Generated\Runtime\Normalizer\ReferenceNormalizer::class,
    ], $normalizersCache = [];
    public function supportsDenormalization(mixed $data, string $type, ?string $format = null, array $context = []): bool
    {
        return array_key_exists($type, $this->normalizers);
    }
    public function supportsNormalization(mixed $data, ?string $format = null, array $context = []): bool
    {
        return is_object($data) && array_key_exists(get_class($data), $this->normalizers);
    }
    public function normalize(mixed $data, ?string $format = null, array $context = []): array|string|int|float|bool|\ArrayObject|null
    {
        $normalizerClass = $this->normalizers[get_class($data)];
        $normalizer = $this->getNormalizer($normalizerClass);
        return $normalizer->normalize($data, $format, $context);
    }
    public function denormalize(mixed $data, string $type, ?string $format = null, array $context = []): mixed
    {
        $denormalizerClass = $this->normalizers[$type];
        $denormalizer = $this->getNormalizer($denormalizerClass);
        return $denormalizer->denormalize($data, $type, $format, $context);
    }
    private function getNormalizer(string $normalizerClass)
    {
        return $this->normalizersCache[$normalizerClass] ?? $this->initNormalizer($normalizerClass);
    }
    private function initNormalizer(string $normalizerClass)
    {
        $normalizer = new $normalizerClass();
        $normalizer->setNormalizer($this->normalizer);
        $normalizer->setDenormalizer($this->denormalizer);
        $this->normalizersCache[$normalizerClass] = $normalizer;
        return $normalizer;
    }
    public function getSupportedTypes(?string $format = null): array
    {
        return [
            
            \FlowCatalyst\Generated\Model\AddNoteRequest::class => false,
            \FlowCatalyst\Generated\Model\AddNoteResponse::class => false,
            \FlowCatalyst\Generated\Model\AddSchemaVersionRequest::class => false,
            \FlowCatalyst\Generated\Model\AggregatesResponse::class => false,
            \FlowCatalyst\Generated\Model\AllFilterOptions::class => false,
            \FlowCatalyst\Generated\Model\ApplicationAccessListResponse::class => false,
            \FlowCatalyst\Generated\Model\ApplicationAccessResponse::class => false,
            \FlowCatalyst\Generated\Model\ApplicationIdsResponse::class => false,
            \FlowCatalyst\Generated\Model\ApplicationOption::class => false,
            \FlowCatalyst\Generated\Model\ApplicationOptionsResponse::class => false,
            \FlowCatalyst\Generated\Model\ApplicationsResponse::class => false,
            \FlowCatalyst\Generated\Model\AssignRoleRequest::class => false,
            \FlowCatalyst\Generated\Model\AuditLogDetailResponse::class => false,
            \FlowCatalyst\Generated\Model\AuditLogListResponse::class => false,
            \FlowCatalyst\Generated\Model\AuditLogResponse::class => false,
            \FlowCatalyst\Generated\Model\AuthenticateBeginRequest::class => false,
            \FlowCatalyst\Generated\Model\AuthenticateBeginResponse::class => false,
            \FlowCatalyst\Generated\Model\AuthenticateCompleteRequest::class => false,
            \FlowCatalyst\Generated\Model\AuthenticateCompleteResponse::class => false,
            \FlowCatalyst\Generated\Model\AvailableApplicationResponse::class => false,
            \FlowCatalyst\Generated\Model\AvailableApplicationsResponse::class => false,
            \FlowCatalyst\Generated\Model\BatchAssignRolesRequest::class => false,
            \FlowCatalyst\Generated\Model\BatchAssignRolesResponse::class => false,
            \FlowCatalyst\Generated\Model\BatchCreateDispatchJobsRequest::class => false,
            \FlowCatalyst\Generated\Model\BatchCreateDispatchJobsResponse::class => false,
            \FlowCatalyst\Generated\Model\BatchCreateEventsRequest::class => false,
            \FlowCatalyst\Generated\Model\BatchCreateResponse::class => false,
            \FlowCatalyst\Generated\Model\CheckEmailDomainResponse::class => false,
            \FlowCatalyst\Generated\Model\CircuitBreakerState::class => false,
            \FlowCatalyst\Generated\Model\CircuitBreakersResponse::class => false,
            \FlowCatalyst\Generated\Model\ClientAccessGrantResponse::class => false,
            \FlowCatalyst\Generated\Model\ClientAccessListResponse::class => false,
            \FlowCatalyst\Generated\Model\ClientApplicationResponse::class => false,
            \FlowCatalyst\Generated\Model\ClientApplicationsResponse::class => false,
            \FlowCatalyst\Generated\Model\ClientFilterOptions::class => false,
            \FlowCatalyst\Generated\Model\ClientIdsResponse::class => false,
            \FlowCatalyst\Generated\Model\ClientListResponse::class => false,
            \FlowCatalyst\Generated\Model\ClientResponse::class => false,
            \FlowCatalyst\Generated\Model\ClusterMember::class => false,
            \FlowCatalyst\Generated\Model\ConfigEntryResponse::class => false,
            \FlowCatalyst\Generated\Model\ContextDataDto::class => false,
            \FlowCatalyst\Generated\Model\CreateClientRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateDispatchJobRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateEventRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateEventResponse::class => false,
            \FlowCatalyst\Generated\Model\CreateEventTypeRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateOAuthClientRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateOAuthClientResponse::class => false,
            \FlowCatalyst\Generated\Model\CreateProcessRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateRoleRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateScheduledJobRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateSubscriptionRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateUserRequest::class => false,
            \FlowCatalyst\Generated\Model\CreatedResponse::class => false,
            \FlowCatalyst\Generated\Model\CredentialSummary::class => false,
            \FlowCatalyst\Generated\Model\CurrentUserResponse::class => false,
            \FlowCatalyst\Generated\Model\DashboardMetrics::class => false,
            \FlowCatalyst\Generated\Model\DispatchAttemptResponse::class => false,
            \FlowCatalyst\Generated\Model\DispatchJobFilterOptionsResponse::class => false,
            \FlowCatalyst\Generated\Model\DispatchJobReadResponse::class => false,
            \FlowCatalyst\Generated\Model\DispatchJobResponse::class => false,
            \FlowCatalyst\Generated\Model\DispatchJobsFilterOptions::class => false,
            \FlowCatalyst\Generated\Model\DispatchPoolFilterOptions::class => false,
            \FlowCatalyst\Generated\Model\DomainCheckResponse::class => false,
            \FlowCatalyst\Generated\Model\EnhancedPoolMetrics::class => false,
            \FlowCatalyst\Generated\Model\EntityAuditLogsResponse::class => false,
            \FlowCatalyst\Generated\Model\EntityTypesResponse::class => false,
            \FlowCatalyst\Generated\Model\ErrorResponse::class => false,
            \FlowCatalyst\Generated\Model\EventFilterOptions::class => false,
            \FlowCatalyst\Generated\Model\EventRead::class => false,
            \FlowCatalyst\Generated\Model\EventResponse::class => false,
            \FlowCatalyst\Generated\Model\EventSummaryResponse::class => false,
            \FlowCatalyst\Generated\Model\EventTypeBindingRequest::class => false,
            \FlowCatalyst\Generated\Model\EventTypeBindingResponse::class => false,
            \FlowCatalyst\Generated\Model\EventTypeFilterOptions::class => false,
            \FlowCatalyst\Generated\Model\EventTypeListResponse::class => false,
            \FlowCatalyst\Generated\Model\EventTypeResponse::class => false,
            \FlowCatalyst\Generated\Model\EventsFilterOptions::class => false,
            \FlowCatalyst\Generated\Model\FilterOption::class => false,
            \FlowCatalyst\Generated\Model\FireRequest::class => false,
            \FlowCatalyst\Generated\Model\GrantClientAccessRequest::class => false,
            \FlowCatalyst\Generated\Model\GrantPermissionRequest::class => false,
            \FlowCatalyst\Generated\Model\InFlightMessage::class => false,
            \FlowCatalyst\Generated\Model\InFlightMessagesResponse::class => false,
            \FlowCatalyst\Generated\Model\InstanceCompleteRequest::class => false,
            \FlowCatalyst\Generated\Model\InstanceLogRequest::class => false,
            \FlowCatalyst\Generated\Model\InstanceLogResponse::class => false,
            \FlowCatalyst\Generated\Model\LoginRequest::class => false,
            \FlowCatalyst\Generated\Model\LoginResponse::class => false,
            \FlowCatalyst\Generated\Model\OAuthClientListResponse::class => false,
            \FlowCatalyst\Generated\Model\OAuthClientResponse::class => false,
            \FlowCatalyst\Generated\Model\OperationsResponse::class => false,
            \FlowCatalyst\Generated\Model\PaginatedResponseScheduledJobInstanceResponse::class => false,
            \FlowCatalyst\Generated\Model\PaginatedResponseScheduledJobInstanceResponseDataItem::class => false,
            \FlowCatalyst\Generated\Model\PaginatedResponseScheduledJobResponse::class => false,
            \FlowCatalyst\Generated\Model\PaginatedResponseScheduledJobResponseDataItem::class => false,
            \FlowCatalyst\Generated\Model\PaginationParams::class => false,
            \FlowCatalyst\Generated\Model\PermissionListResponse::class => false,
            \FlowCatalyst\Generated\Model\PermissionResponse::class => false,
            \FlowCatalyst\Generated\Model\PoolStats::class => false,
            \FlowCatalyst\Generated\Model\PoolStatsResponse::class => false,
            \FlowCatalyst\Generated\Model\PrincipalListResponse::class => false,
            \FlowCatalyst\Generated\Model\PrincipalResponse::class => false,
            \FlowCatalyst\Generated\Model\ProcessListResponse::class => false,
            \FlowCatalyst\Generated\Model\ProcessResponse::class => false,
            \FlowCatalyst\Generated\Model\ProcessingTimeMetrics::class => false,
            \FlowCatalyst\Generated\Model\RefreshTokenRequest::class => false,
            \FlowCatalyst\Generated\Model\RegenerateSecretResponse::class => false,
            \FlowCatalyst\Generated\Model\RegisterBeginRequest::class => false,
            \FlowCatalyst\Generated\Model\RegisterBeginResponse::class => false,
            \FlowCatalyst\Generated\Model\RegisterCompleteRequest::class => false,
            \FlowCatalyst\Generated\Model\RegisterCompleteResponse::class => false,
            \FlowCatalyst\Generated\Model\ResetPasswordRequest::class => false,
            \FlowCatalyst\Generated\Model\RoleAssignmentDto::class => false,
            \FlowCatalyst\Generated\Model\RoleListResponse::class => false,
            \FlowCatalyst\Generated\Model\RoleResponse::class => false,
            \FlowCatalyst\Generated\Model\RolesListResponse::class => false,
            \FlowCatalyst\Generated\Model\ScheduledJobInstanceResponse::class => false,
            \FlowCatalyst\Generated\Model\ScheduledJobResponse::class => false,
            \FlowCatalyst\Generated\Model\SetApplicationAccessRequest::class => false,
            \FlowCatalyst\Generated\Model\SetApplicationAccessResponse::class => false,
            \FlowCatalyst\Generated\Model\SpecVersionResponse::class => false,
            \FlowCatalyst\Generated\Model\StandbyStatus::class => false,
            \FlowCatalyst\Generated\Model\StatusChangeRequest::class => false,
            \FlowCatalyst\Generated\Model\StatusChangeResponse::class => false,
            \FlowCatalyst\Generated\Model\SubdomainsResponse::class => false,
            \FlowCatalyst\Generated\Model\SubscriptionFilterOptions::class => false,
            \FlowCatalyst\Generated\Model\SubscriptionListResponse::class => false,
            \FlowCatalyst\Generated\Model\SubscriptionResponse::class => false,
            \FlowCatalyst\Generated\Model\SuccessResponse::class => false,
            \FlowCatalyst\Generated\Model\SyncDispatchPoolInputRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncDispatchPoolsRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncEventTypeInputRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncEventTypesRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncOpenApiSpecRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncOpenApiSpecResponse::class => false,
            \FlowCatalyst\Generated\Model\SyncPrincipalInputRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncPrincipalsRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncProcessInputRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncProcessesRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncResultResponse::class => false,
            \FlowCatalyst\Generated\Model\SyncRoleInputRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncRolesRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncScheduledJobInputRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncScheduledJobsRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncScheduledJobsResultResponse::class => false,
            \FlowCatalyst\Generated\Model\SyncSubscriptionEventTypeRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncSubscriptionInputRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncSubscriptionsRequest::class => false,
            \FlowCatalyst\Generated\Model\SystemHealth::class => false,
            \FlowCatalyst\Generated\Model\TokenRefreshResponse::class => false,
            \FlowCatalyst\Generated\Model\UpdateClientApplicationsRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateClientRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateEventTypeRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateOAuthClientRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdatePrincipalRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateProcessRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateRoleRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateScheduledJobRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateSubscriptionRequest::class => false,
            \FlowCatalyst\Generated\Model\WindowedMetrics::class => false,
            \Jane\Component\JsonSchemaRuntime\Reference::class => false,
        ];
    }
}