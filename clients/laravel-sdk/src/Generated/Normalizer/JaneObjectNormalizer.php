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
        
        \FlowCatalyst\Generated\Model\AccessListResponse::class => \FlowCatalyst\Generated\Normalizer\AccessListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AccessResponse::class => \FlowCatalyst\Generated\Normalizer\AccessResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AddNoteRequest::class => \FlowCatalyst\Generated\Normalizer\AddNoteRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AddOriginRequest::class => \FlowCatalyst\Generated\Normalizer\AddOriginRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AddRoleRequest::class => \FlowCatalyst\Generated\Normalizer\AddRoleRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AddSchemaRequest::class => \FlowCatalyst\Generated\Normalizer\AddSchemaRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AllowedOriginResponse::class => \FlowCatalyst\Generated\Normalizer\AllowedOriginResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AnchorDomainListResponse::class => \FlowCatalyst\Generated\Normalizer\AnchorDomainListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AnchorDomainResponse::class => \FlowCatalyst\Generated\Normalizer\AnchorDomainResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AppDocsGroup::class => \FlowCatalyst\Generated\Normalizer\AppDocsGroupNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationAccessListResponse::class => \FlowCatalyst\Generated\Normalizer\ApplicationAccessListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationAccessResponse::class => \FlowCatalyst\Generated\Normalizer\ApplicationAccessResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationFilterListResponse::class => \FlowCatalyst\Generated\Normalizer\ApplicationFilterListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationListResponse::class => \FlowCatalyst\Generated\Normalizer\ApplicationListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationLoginClientCredentials::class => \FlowCatalyst\Generated\Normalizer\ApplicationLoginClientCredentialsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationOAuthClientCredentials::class => \FlowCatalyst\Generated\Normalizer\ApplicationOAuthClientCredentialsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationProvisionLoginClientResponse::class => \FlowCatalyst\Generated\Normalizer\ApplicationProvisionLoginClientResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationProvisionServiceAccountResponse::class => \FlowCatalyst\Generated\Normalizer\ApplicationProvisionServiceAccountResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationResponse::class => \FlowCatalyst\Generated\Normalizer\ApplicationResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationRolesResponse::class => \FlowCatalyst\Generated\Normalizer\ApplicationRolesResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ApplicationServiceAccountCredentials::class => \FlowCatalyst\Generated\Normalizer\ApplicationServiceAccountCredentialsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AssignApplicationAccessRequest::class => \FlowCatalyst\Generated\Normalizer\AssignApplicationAccessRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AssignPrincipalRolesRequest::class => \FlowCatalyst\Generated\Normalizer\AssignPrincipalRolesRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AssignRolesRequest::class => \FlowCatalyst\Generated\Normalizer\AssignRolesRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AssignUnassignedBody::class => \FlowCatalyst\Generated\Normalizer\AssignUnassignedBodyNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AssignUnassignedResponse::class => \FlowCatalyst\Generated\Normalizer\AssignUnassignedResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AttachServiceAccountRequest::class => \FlowCatalyst\Generated\Normalizer\AttachServiceAccountRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AttemptDTO::class => \FlowCatalyst\Generated\Normalizer\AttemptDTONormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuditLogApplicationIDsResponse::class => \FlowCatalyst\Generated\Normalizer\AuditLogApplicationIDsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuditLogClientIDsResponse::class => \FlowCatalyst\Generated\Normalizer\AuditLogClientIDsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuditLogEntityTypesResponse::class => \FlowCatalyst\Generated\Normalizer\AuditLogEntityTypesResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuditLogListResponse::class => \FlowCatalyst\Generated\Normalizer\AuditLogListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuditLogOperationsResponse::class => \FlowCatalyst\Generated\Normalizer\AuditLogOperationsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuditLogResponse::class => \FlowCatalyst\Generated\Normalizer\AuditLogResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuthConfigListResponse::class => \FlowCatalyst\Generated\Normalizer\AuthConfigListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuthConfigResponse::class => \FlowCatalyst\Generated\Normalizer\AuthConfigResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuthenticateBeginRequest::class => \FlowCatalyst\Generated\Normalizer\AuthenticateBeginRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuthenticateBeginResponse::class => \FlowCatalyst\Generated\Normalizer\AuthenticateBeginResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\AuthenticateCompleteRequest::class => \FlowCatalyst\Generated\Normalizer\AuthenticateCompleteRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\BatchEventItem::class => \FlowCatalyst\Generated\Normalizer\BatchEventItemNormalizer::class,
        
        \FlowCatalyst\Generated\Model\BatchRequest::class => \FlowCatalyst\Generated\Normalizer\BatchRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\BatchResponse::class => \FlowCatalyst\Generated\Normalizer\BatchResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\BatchResultItem::class => \FlowCatalyst\Generated\Normalizer\BatchResultItemNormalizer::class,
        
        \FlowCatalyst\Generated\Model\BulkImportRequest::class => \FlowCatalyst\Generated\Normalizer\BulkImportRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\BulkImportResponse::class => \FlowCatalyst\Generated\Normalizer\BulkImportResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\BulkImportResult::class => \FlowCatalyst\Generated\Normalizer\BulkImportResultNormalizer::class,
        
        \FlowCatalyst\Generated\Model\BulkImportUser::class => \FlowCatalyst\Generated\Normalizer\BulkImportUserNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CheckEmailDomainResponse::class => \FlowCatalyst\Generated\Normalizer\CheckEmailDomainResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientAccessGrantListResponse::class => \FlowCatalyst\Generated\Normalizer\ClientAccessGrantListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientAccessGrantResponse::class => \FlowCatalyst\Generated\Normalizer\ClientAccessGrantResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientApplicationResponse::class => \FlowCatalyst\Generated\Normalizer\ClientApplicationResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientApplicationsResponse::class => \FlowCatalyst\Generated\Normalizer\ClientApplicationsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientAssociationRequest::class => \FlowCatalyst\Generated\Normalizer\ClientAssociationRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientConfigListResponse::class => \FlowCatalyst\Generated\Normalizer\ClientConfigListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientConfigResponse::class => \FlowCatalyst\Generated\Normalizer\ClientConfigResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientListResponse::class => \FlowCatalyst\Generated\Normalizer\ClientListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ClientResponse::class => \FlowCatalyst\Generated\Normalizer\ClientResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CompleteInstanceRequest::class => \FlowCatalyst\Generated\Normalizer\CompleteInstanceRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ConfigEntryDTO::class => \FlowCatalyst\Generated\Normalizer\ConfigEntryDTONormalizer::class,
        
        \FlowCatalyst\Generated\Model\ConfigListResponse::class => \FlowCatalyst\Generated\Normalizer\ConfigListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ConfigResponse::class => \FlowCatalyst\Generated\Normalizer\ConfigResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ConnectionListResponse::class => \FlowCatalyst\Generated\Normalizer\ConnectionListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ConnectionResponse::class => \FlowCatalyst\Generated\Normalizer\ConnectionResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ContextEntryDTO::class => \FlowCatalyst\Generated\Normalizer\ContextEntryDTONormalizer::class,
        
        \FlowCatalyst\Generated\Model\CorsOriginListResponse::class => \FlowCatalyst\Generated\Normalizer\CorsOriginListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateAnchorDomainRequest::class => \FlowCatalyst\Generated\Normalizer\CreateAnchorDomainRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateApplicationRequest::class => \FlowCatalyst\Generated\Normalizer\CreateApplicationRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateAuthConfigRequest::class => \FlowCatalyst\Generated\Normalizer\CreateAuthConfigRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateClientRequest::class => \FlowCatalyst\Generated\Normalizer\CreateClientRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateConnectionRequest::class => \FlowCatalyst\Generated\Normalizer\CreateConnectionRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateDispatchPoolRequest::class => \FlowCatalyst\Generated\Normalizer\CreateDispatchPoolRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateEventRequest::class => \FlowCatalyst\Generated\Normalizer\CreateEventRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateEventResponse::class => \FlowCatalyst\Generated\Normalizer\CreateEventResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateEventTypeRequest::class => \FlowCatalyst\Generated\Normalizer\CreateEventTypeRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateIdentityProviderRequest::class => \FlowCatalyst\Generated\Normalizer\CreateIdentityProviderRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateIdpRoleMappingRequest::class => \FlowCatalyst\Generated\Normalizer\CreateIdpRoleMappingRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateMappingRequest::class => \FlowCatalyst\Generated\Normalizer\CreateMappingRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateOAuthClientRequest::class => \FlowCatalyst\Generated\Normalizer\CreateOAuthClientRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateOAuthClientResponse::class => \FlowCatalyst\Generated\Normalizer\CreateOAuthClientResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreatePortalAppRequest::class => \FlowCatalyst\Generated\Normalizer\CreatePortalAppRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreatePortalAppResponse::class => \FlowCatalyst\Generated\Normalizer\CreatePortalAppResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreatePrincipalRequest::class => \FlowCatalyst\Generated\Normalizer\CreatePrincipalRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreatePrincipalResponse::class => \FlowCatalyst\Generated\Normalizer\CreatePrincipalResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateProcessRequest::class => \FlowCatalyst\Generated\Normalizer\CreateProcessRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateRoleRequest::class => \FlowCatalyst\Generated\Normalizer\CreateRoleRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateScheduledJobRequest::class => \FlowCatalyst\Generated\Normalizer\CreateScheduledJobRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateServiceAccountRequest::class => \FlowCatalyst\Generated\Normalizer\CreateServiceAccountRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateServiceAccountResponse::class => \FlowCatalyst\Generated\Normalizer\CreateServiceAccountResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateSubscriptionRequest::class => \FlowCatalyst\Generated\Normalizer\CreateSubscriptionRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreateUserRequest::class => \FlowCatalyst\Generated\Normalizer\CreateUserRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreatedEvent::class => \FlowCatalyst\Generated\Normalizer\CreatedEventNormalizer::class,
        
        \FlowCatalyst\Generated\Model\CreatedResponse::class => \FlowCatalyst\Generated\Normalizer\CreatedResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DeveloperUserListResponse::class => \FlowCatalyst\Generated\Normalizer\DeveloperUserListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DispatchJobFilterOptionsResponse::class => \FlowCatalyst\Generated\Normalizer\DispatchJobFilterOptionsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DispatchJobRead::class => \FlowCatalyst\Generated\Normalizer\DispatchJobReadNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DispatchJobResponse::class => \FlowCatalyst\Generated\Normalizer\DispatchJobResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DispatchPoolListResponse::class => \FlowCatalyst\Generated\Normalizer\DispatchPoolListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DispatchPoolResponse::class => \FlowCatalyst\Generated\Normalizer\DispatchPoolResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DocListResponse::class => \FlowCatalyst\Generated\Normalizer\DocListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DocResponse::class => \FlowCatalyst\Generated\Normalizer\DocResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\DocSummary::class => \FlowCatalyst\Generated\Normalizer\DocSummaryNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ErrorModel::class => \FlowCatalyst\Generated\Normalizer\ErrorModelNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventFilterOption::class => \FlowCatalyst\Generated\Normalizer\EventFilterOptionNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventFilterOptionsResponse::class => \FlowCatalyst\Generated\Normalizer\EventFilterOptionsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventRead::class => \FlowCatalyst\Generated\Normalizer\EventReadNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventResponse::class => \FlowCatalyst\Generated\Normalizer\EventResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventTypeBindingDTO::class => \FlowCatalyst\Generated\Normalizer\EventTypeBindingDTONormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventTypeListResponse::class => \FlowCatalyst\Generated\Normalizer\EventTypeListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\EventTypeResponse::class => \FlowCatalyst\Generated\Normalizer\EventTypeResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\FireNowRequest::class => \FlowCatalyst\Generated\Normalizer\FireNowRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\FireNowResponse::class => \FlowCatalyst\Generated\Normalizer\FireNowResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\GrantAccessRequest::class => \FlowCatalyst\Generated\Normalizer\GrantAccessRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\GrantClientAccessRequest::class => \FlowCatalyst\Generated\Normalizer\GrantClientAccessRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\GrantPermissionRequest::class => \FlowCatalyst\Generated\Normalizer\GrantPermissionRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\IdentityProviderListResponse::class => \FlowCatalyst\Generated\Normalizer\IdentityProviderListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\IdentityProviderResponse::class => \FlowCatalyst\Generated\Normalizer\IdentityProviderResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\IdpRoleMappingListResponse::class => \FlowCatalyst\Generated\Normalizer\IdpRoleMappingListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\IdpRoleMappingResponse::class => \FlowCatalyst\Generated\Normalizer\IdpRoleMappingResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\LinkedOAuthClient::class => \FlowCatalyst\Generated\Normalizer\LinkedOAuthClientNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ListOutputBody::class => \FlowCatalyst\Generated\Normalizer\ListOutputBodyNormalizer::class,
        
        \FlowCatalyst\Generated\Model\LoginAttemptListResponse::class => \FlowCatalyst\Generated\Normalizer\LoginAttemptListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\LoginAttemptResponse::class => \FlowCatalyst\Generated\Normalizer\LoginAttemptResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\MappingListResponse::class => \FlowCatalyst\Generated\Normalizer\MappingListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\MappingResponse::class => \FlowCatalyst\Generated\Normalizer\MappingResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\MetadataDTO::class => \FlowCatalyst\Generated\Normalizer\MetadataDTONormalizer::class,
        
        \FlowCatalyst\Generated\Model\MoveProviderRequest::class => \FlowCatalyst\Generated\Normalizer\MoveProviderRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\MoveProviderResponse::class => \FlowCatalyst\Generated\Normalizer\MoveProviderResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\NoteResponse::class => \FlowCatalyst\Generated\Normalizer\NoteResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\OAuthClientApplicationRef::class => \FlowCatalyst\Generated\Normalizer\OAuthClientApplicationRefNormalizer::class,
        
        \FlowCatalyst\Generated\Model\OAuthClientListResponse::class => \FlowCatalyst\Generated\Normalizer\OAuthClientListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\OAuthClientResponse::class => \FlowCatalyst\Generated\Normalizer\OAuthClientResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\OffsetPageScheduledJobInstanceResponse::class => \FlowCatalyst\Generated\Normalizer\OffsetPageScheduledJobInstanceResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\OffsetPageScheduledJobResponse::class => \FlowCatalyst\Generated\Normalizer\OffsetPageScheduledJobResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PermissionListResponse::class => \FlowCatalyst\Generated\Normalizer\PermissionListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PermissionResponse::class => \FlowCatalyst\Generated\Normalizer\PermissionResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PoolConfig::class => \FlowCatalyst\Generated\Normalizer\PoolConfigNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PortalAppListResponse::class => \FlowCatalyst\Generated\Normalizer\PortalAppListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PortalAppResponse::class => \FlowCatalyst\Generated\Normalizer\PortalAppResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PortalUserAppGrantBody::class => \FlowCatalyst\Generated\Normalizer\PortalUserAppGrantBodyNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PortalUserAppRef::class => \FlowCatalyst\Generated\Normalizer\PortalUserAppRefNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PortalUserClientBody::class => \FlowCatalyst\Generated\Normalizer\PortalUserClientBodyNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PortalUserListItem::class => \FlowCatalyst\Generated\Normalizer\PortalUserListItemNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PortalUserListResponse::class => \FlowCatalyst\Generated\Normalizer\PortalUserListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PortalUserRequest::class => \FlowCatalyst\Generated\Normalizer\PortalUserRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PortalUserResponse::class => \FlowCatalyst\Generated\Normalizer\PortalUserResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PrincipalAvailableApplication::class => \FlowCatalyst\Generated\Normalizer\PrincipalAvailableApplicationNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PrincipalAvailableApplicationsResponse::class => \FlowCatalyst\Generated\Normalizer\PrincipalAvailableApplicationsResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PrincipalListResponse::class => \FlowCatalyst\Generated\Normalizer\PrincipalListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PrincipalResponse::class => \FlowCatalyst\Generated\Normalizer\PrincipalResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PrincipalRoleAssignmentDTO::class => \FlowCatalyst\Generated\Normalizer\PrincipalRoleAssignmentDTONormalizer::class,
        
        \FlowCatalyst\Generated\Model\PrincipalRoleListResponse::class => \FlowCatalyst\Generated\Normalizer\PrincipalRoleListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PrincipalVersionResponse::class => \FlowCatalyst\Generated\Normalizer\PrincipalVersionResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ProcessListResponse::class => \FlowCatalyst\Generated\Normalizer\ProcessListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ProcessResponse::class => \FlowCatalyst\Generated\Normalizer\ProcessResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ProvisionLoginClientRequest::class => \FlowCatalyst\Generated\Normalizer\ProvisionLoginClientRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\PublicAllowedResponse::class => \FlowCatalyst\Generated\Normalizer\PublicAllowedResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\QueueConfig::class => \FlowCatalyst\Generated\Normalizer\QueueConfigNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RawDispatchJobResponse::class => \FlowCatalyst\Generated\Normalizer\RawDispatchJobResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RawEventResponse::class => \FlowCatalyst\Generated\Normalizer\RawEventResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RegenerateAuthTokenResponse::class => \FlowCatalyst\Generated\Normalizer\RegenerateAuthTokenResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RegenerateSigningSecretResponse::class => \FlowCatalyst\Generated\Normalizer\RegenerateSigningSecretResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RegisterBeginRequest::class => \FlowCatalyst\Generated\Normalizer\RegisterBeginRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RegisterBeginResponse::class => \FlowCatalyst\Generated\Normalizer\RegisterBeginResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RegisterCompleteRequest::class => \FlowCatalyst\Generated\Normalizer\RegisterCompleteRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RegisterCompleteResponse::class => \FlowCatalyst\Generated\Normalizer\RegisterCompleteResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RequestDTO::class => \FlowCatalyst\Generated\Normalizer\RequestDTONormalizer::class,
        
        \FlowCatalyst\Generated\Model\RequeueRequest::class => \FlowCatalyst\Generated\Normalizer\RequeueRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RequeueResponse::class => \FlowCatalyst\Generated\Normalizer\RequeueResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ResetPasswordRequest::class => \FlowCatalyst\Generated\Normalizer\ResetPasswordRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RoleAssignmentDTO::class => \FlowCatalyst\Generated\Normalizer\RoleAssignmentDTONormalizer::class,
        
        \FlowCatalyst\Generated\Model\RoleListResponse::class => \FlowCatalyst\Generated\Normalizer\RoleListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RolePermissionListResponse::class => \FlowCatalyst\Generated\Normalizer\RolePermissionListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RoleResponse::class => \FlowCatalyst\Generated\Normalizer\RoleResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RolesAssignedResponse::class => \FlowCatalyst\Generated\Normalizer\RolesAssignedResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RotateOAuthClientSecretRequest::class => \FlowCatalyst\Generated\Normalizer\RotateOAuthClientSecretRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RotateOAuthClientSecretResponse::class => \FlowCatalyst\Generated\Normalizer\RotateOAuthClientSecretResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\RouterConfig::class => \FlowCatalyst\Generated\Normalizer\RouterConfigNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ScheduledJobInstanceLogResponse::class => \FlowCatalyst\Generated\Normalizer\ScheduledJobInstanceLogResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ScheduledJobInstanceResponse::class => \FlowCatalyst\Generated\Normalizer\ScheduledJobInstanceResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ScheduledJobResponse::class => \FlowCatalyst\Generated\Normalizer\ScheduledJobResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SearchClientRequest::class => \FlowCatalyst\Generated\Normalizer\SearchClientRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SendPasswordResetInputBody::class => \FlowCatalyst\Generated\Normalizer\SendPasswordResetInputBodyNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ServiceAccountListResponse::class => \FlowCatalyst\Generated\Normalizer\ServiceAccountListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ServiceAccountOAuthSecrets::class => \FlowCatalyst\Generated\Normalizer\ServiceAccountOAuthSecretsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ServiceAccountResponse::class => \FlowCatalyst\Generated\Normalizer\ServiceAccountResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ServiceAccountRoleListResponse::class => \FlowCatalyst\Generated\Normalizer\ServiceAccountRoleListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ServiceAccountRolesAssignedResponse::class => \FlowCatalyst\Generated\Normalizer\ServiceAccountRolesAssignedResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ServiceAccountTokenResponse::class => \FlowCatalyst\Generated\Normalizer\ServiceAccountTokenResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\ServiceAccountWebhookSecrets::class => \FlowCatalyst\Generated\Normalizer\ServiceAccountWebhookSecretsNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SetApplicationAccessResponse::class => \FlowCatalyst\Generated\Normalizer\SetApplicationAccessResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SetDeveloperCredentialResponse::class => \FlowCatalyst\Generated\Normalizer\SetDeveloperCredentialResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SetPropertyRequest::class => \FlowCatalyst\Generated\Normalizer\SetPropertyRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SpecVersionResponse::class => \FlowCatalyst\Generated\Normalizer\SpecVersionResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\StatusChangeRequest::class => \FlowCatalyst\Generated\Normalizer\StatusChangeRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\StatusChangeResponse::class => \FlowCatalyst\Generated\Normalizer\StatusChangeResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SubscriptionListResponse::class => \FlowCatalyst\Generated\Normalizer\SubscriptionListResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SubscriptionResponse::class => \FlowCatalyst\Generated\Normalizer\SubscriptionResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SuccessResponse::class => \FlowCatalyst\Generated\Normalizer\SuccessResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SuspendClientRequest::class => \FlowCatalyst\Generated\Normalizer\SuspendClientRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncDispatchPoolInputRequest::class => \FlowCatalyst\Generated\Normalizer\SyncDispatchPoolInputRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncDispatchPoolsRequest::class => \FlowCatalyst\Generated\Normalizer\SyncDispatchPoolsRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncDocInputRequest::class => \FlowCatalyst\Generated\Normalizer\SyncDocInputRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncDocsRequest::class => \FlowCatalyst\Generated\Normalizer\SyncDocsRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncEventTypeInputRequest::class => \FlowCatalyst\Generated\Normalizer\SyncEventTypeInputRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncEventTypesRequest::class => \FlowCatalyst\Generated\Normalizer\SyncEventTypesRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncOpenApiSpecResponse::class => \FlowCatalyst\Generated\Normalizer\SyncOpenApiSpecResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncOpenapiRequest::class => \FlowCatalyst\Generated\Normalizer\SyncOpenapiRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncPrincipalInputRequest::class => \FlowCatalyst\Generated\Normalizer\SyncPrincipalInputRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncPrincipalsRequest::class => \FlowCatalyst\Generated\Normalizer\SyncPrincipalsRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncProcessInputRequest::class => \FlowCatalyst\Generated\Normalizer\SyncProcessInputRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncProcessesByBodyRequest::class => \FlowCatalyst\Generated\Normalizer\SyncProcessesByBodyRequestNormalizer::class,
        
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
        
        \FlowCatalyst\Generated\Model\SyncUserInput::class => \FlowCatalyst\Generated\Normalizer\SyncUserInputNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncUsersRequest::class => \FlowCatalyst\Generated\Normalizer\SyncUsersRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\SyncUsersResponse::class => \FlowCatalyst\Generated\Normalizer\SyncUsersResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateAnchorDomainRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateAnchorDomainRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateApplicationRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateApplicationRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateAuthConfigRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateAuthConfigRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateClientApplicationsRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateClientApplicationsRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateClientRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateClientRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateConnectionRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateConnectionRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateDispatchPoolRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateDispatchPoolRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateEventTypeRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateEventTypeRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateIdentityProviderRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateIdentityProviderRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateMappingRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateMappingRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateOAuthClientRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateOAuthClientRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdatePortalAppRequest::class => \FlowCatalyst\Generated\Normalizer\UpdatePortalAppRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdatePrincipalRequest::class => \FlowCatalyst\Generated\Normalizer\UpdatePrincipalRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateProcessRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateProcessRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateRoleRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateRoleRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateScheduledJobRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateScheduledJobRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateServiceAccountRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateServiceAccountRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\UpdateSubscriptionRequest::class => \FlowCatalyst\Generated\Normalizer\UpdateSubscriptionRequestNormalizer::class,
        
        \FlowCatalyst\Generated\Model\WebauthnAuthenticateCompleteResponse::class => \FlowCatalyst\Generated\Normalizer\WebauthnAuthenticateCompleteResponseNormalizer::class,
        
        \FlowCatalyst\Generated\Model\WebauthnCredentialSummary::class => \FlowCatalyst\Generated\Normalizer\WebauthnCredentialSummaryNormalizer::class,
        
        \FlowCatalyst\Generated\Model\WebhookCredentialsDTO::class => \FlowCatalyst\Generated\Normalizer\WebhookCredentialsDTONormalizer::class,
        
        \FlowCatalyst\Generated\Model\WriteInstanceLogRequest::class => \FlowCatalyst\Generated\Normalizer\WriteInstanceLogRequestNormalizer::class,
        
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
            
            \FlowCatalyst\Generated\Model\AccessListResponse::class => false,
            \FlowCatalyst\Generated\Model\AccessResponse::class => false,
            \FlowCatalyst\Generated\Model\AddNoteRequest::class => false,
            \FlowCatalyst\Generated\Model\AddOriginRequest::class => false,
            \FlowCatalyst\Generated\Model\AddRoleRequest::class => false,
            \FlowCatalyst\Generated\Model\AddSchemaRequest::class => false,
            \FlowCatalyst\Generated\Model\AllowedOriginResponse::class => false,
            \FlowCatalyst\Generated\Model\AnchorDomainListResponse::class => false,
            \FlowCatalyst\Generated\Model\AnchorDomainResponse::class => false,
            \FlowCatalyst\Generated\Model\AppDocsGroup::class => false,
            \FlowCatalyst\Generated\Model\ApplicationAccessListResponse::class => false,
            \FlowCatalyst\Generated\Model\ApplicationAccessResponse::class => false,
            \FlowCatalyst\Generated\Model\ApplicationFilterListResponse::class => false,
            \FlowCatalyst\Generated\Model\ApplicationListResponse::class => false,
            \FlowCatalyst\Generated\Model\ApplicationLoginClientCredentials::class => false,
            \FlowCatalyst\Generated\Model\ApplicationOAuthClientCredentials::class => false,
            \FlowCatalyst\Generated\Model\ApplicationProvisionLoginClientResponse::class => false,
            \FlowCatalyst\Generated\Model\ApplicationProvisionServiceAccountResponse::class => false,
            \FlowCatalyst\Generated\Model\ApplicationResponse::class => false,
            \FlowCatalyst\Generated\Model\ApplicationRolesResponse::class => false,
            \FlowCatalyst\Generated\Model\ApplicationServiceAccountCredentials::class => false,
            \FlowCatalyst\Generated\Model\AssignApplicationAccessRequest::class => false,
            \FlowCatalyst\Generated\Model\AssignPrincipalRolesRequest::class => false,
            \FlowCatalyst\Generated\Model\AssignRolesRequest::class => false,
            \FlowCatalyst\Generated\Model\AssignUnassignedBody::class => false,
            \FlowCatalyst\Generated\Model\AssignUnassignedResponse::class => false,
            \FlowCatalyst\Generated\Model\AttachServiceAccountRequest::class => false,
            \FlowCatalyst\Generated\Model\AttemptDTO::class => false,
            \FlowCatalyst\Generated\Model\AuditLogApplicationIDsResponse::class => false,
            \FlowCatalyst\Generated\Model\AuditLogClientIDsResponse::class => false,
            \FlowCatalyst\Generated\Model\AuditLogEntityTypesResponse::class => false,
            \FlowCatalyst\Generated\Model\AuditLogListResponse::class => false,
            \FlowCatalyst\Generated\Model\AuditLogOperationsResponse::class => false,
            \FlowCatalyst\Generated\Model\AuditLogResponse::class => false,
            \FlowCatalyst\Generated\Model\AuthConfigListResponse::class => false,
            \FlowCatalyst\Generated\Model\AuthConfigResponse::class => false,
            \FlowCatalyst\Generated\Model\AuthenticateBeginRequest::class => false,
            \FlowCatalyst\Generated\Model\AuthenticateBeginResponse::class => false,
            \FlowCatalyst\Generated\Model\AuthenticateCompleteRequest::class => false,
            \FlowCatalyst\Generated\Model\BatchEventItem::class => false,
            \FlowCatalyst\Generated\Model\BatchRequest::class => false,
            \FlowCatalyst\Generated\Model\BatchResponse::class => false,
            \FlowCatalyst\Generated\Model\BatchResultItem::class => false,
            \FlowCatalyst\Generated\Model\BulkImportRequest::class => false,
            \FlowCatalyst\Generated\Model\BulkImportResponse::class => false,
            \FlowCatalyst\Generated\Model\BulkImportResult::class => false,
            \FlowCatalyst\Generated\Model\BulkImportUser::class => false,
            \FlowCatalyst\Generated\Model\CheckEmailDomainResponse::class => false,
            \FlowCatalyst\Generated\Model\ClientAccessGrantListResponse::class => false,
            \FlowCatalyst\Generated\Model\ClientAccessGrantResponse::class => false,
            \FlowCatalyst\Generated\Model\ClientApplicationResponse::class => false,
            \FlowCatalyst\Generated\Model\ClientApplicationsResponse::class => false,
            \FlowCatalyst\Generated\Model\ClientAssociationRequest::class => false,
            \FlowCatalyst\Generated\Model\ClientConfigListResponse::class => false,
            \FlowCatalyst\Generated\Model\ClientConfigResponse::class => false,
            \FlowCatalyst\Generated\Model\ClientListResponse::class => false,
            \FlowCatalyst\Generated\Model\ClientResponse::class => false,
            \FlowCatalyst\Generated\Model\CompleteInstanceRequest::class => false,
            \FlowCatalyst\Generated\Model\ConfigEntryDTO::class => false,
            \FlowCatalyst\Generated\Model\ConfigListResponse::class => false,
            \FlowCatalyst\Generated\Model\ConfigResponse::class => false,
            \FlowCatalyst\Generated\Model\ConnectionListResponse::class => false,
            \FlowCatalyst\Generated\Model\ConnectionResponse::class => false,
            \FlowCatalyst\Generated\Model\ContextEntryDTO::class => false,
            \FlowCatalyst\Generated\Model\CorsOriginListResponse::class => false,
            \FlowCatalyst\Generated\Model\CreateAnchorDomainRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateApplicationRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateAuthConfigRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateClientRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateConnectionRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateDispatchPoolRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateEventRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateEventResponse::class => false,
            \FlowCatalyst\Generated\Model\CreateEventTypeRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateIdentityProviderRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateIdpRoleMappingRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateMappingRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateOAuthClientRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateOAuthClientResponse::class => false,
            \FlowCatalyst\Generated\Model\CreatePortalAppRequest::class => false,
            \FlowCatalyst\Generated\Model\CreatePortalAppResponse::class => false,
            \FlowCatalyst\Generated\Model\CreatePrincipalRequest::class => false,
            \FlowCatalyst\Generated\Model\CreatePrincipalResponse::class => false,
            \FlowCatalyst\Generated\Model\CreateProcessRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateRoleRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateScheduledJobRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateServiceAccountRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateServiceAccountResponse::class => false,
            \FlowCatalyst\Generated\Model\CreateSubscriptionRequest::class => false,
            \FlowCatalyst\Generated\Model\CreateUserRequest::class => false,
            \FlowCatalyst\Generated\Model\CreatedEvent::class => false,
            \FlowCatalyst\Generated\Model\CreatedResponse::class => false,
            \FlowCatalyst\Generated\Model\DeveloperUserListResponse::class => false,
            \FlowCatalyst\Generated\Model\DispatchJobFilterOptionsResponse::class => false,
            \FlowCatalyst\Generated\Model\DispatchJobRead::class => false,
            \FlowCatalyst\Generated\Model\DispatchJobResponse::class => false,
            \FlowCatalyst\Generated\Model\DispatchPoolListResponse::class => false,
            \FlowCatalyst\Generated\Model\DispatchPoolResponse::class => false,
            \FlowCatalyst\Generated\Model\DocListResponse::class => false,
            \FlowCatalyst\Generated\Model\DocResponse::class => false,
            \FlowCatalyst\Generated\Model\DocSummary::class => false,
            \FlowCatalyst\Generated\Model\ErrorModel::class => false,
            \FlowCatalyst\Generated\Model\EventFilterOption::class => false,
            \FlowCatalyst\Generated\Model\EventFilterOptionsResponse::class => false,
            \FlowCatalyst\Generated\Model\EventRead::class => false,
            \FlowCatalyst\Generated\Model\EventResponse::class => false,
            \FlowCatalyst\Generated\Model\EventTypeBindingDTO::class => false,
            \FlowCatalyst\Generated\Model\EventTypeListResponse::class => false,
            \FlowCatalyst\Generated\Model\EventTypeResponse::class => false,
            \FlowCatalyst\Generated\Model\FireNowRequest::class => false,
            \FlowCatalyst\Generated\Model\FireNowResponse::class => false,
            \FlowCatalyst\Generated\Model\GrantAccessRequest::class => false,
            \FlowCatalyst\Generated\Model\GrantClientAccessRequest::class => false,
            \FlowCatalyst\Generated\Model\GrantPermissionRequest::class => false,
            \FlowCatalyst\Generated\Model\IdentityProviderListResponse::class => false,
            \FlowCatalyst\Generated\Model\IdentityProviderResponse::class => false,
            \FlowCatalyst\Generated\Model\IdpRoleMappingListResponse::class => false,
            \FlowCatalyst\Generated\Model\IdpRoleMappingResponse::class => false,
            \FlowCatalyst\Generated\Model\LinkedOAuthClient::class => false,
            \FlowCatalyst\Generated\Model\ListOutputBody::class => false,
            \FlowCatalyst\Generated\Model\LoginAttemptListResponse::class => false,
            \FlowCatalyst\Generated\Model\LoginAttemptResponse::class => false,
            \FlowCatalyst\Generated\Model\MappingListResponse::class => false,
            \FlowCatalyst\Generated\Model\MappingResponse::class => false,
            \FlowCatalyst\Generated\Model\MetadataDTO::class => false,
            \FlowCatalyst\Generated\Model\MoveProviderRequest::class => false,
            \FlowCatalyst\Generated\Model\MoveProviderResponse::class => false,
            \FlowCatalyst\Generated\Model\NoteResponse::class => false,
            \FlowCatalyst\Generated\Model\OAuthClientApplicationRef::class => false,
            \FlowCatalyst\Generated\Model\OAuthClientListResponse::class => false,
            \FlowCatalyst\Generated\Model\OAuthClientResponse::class => false,
            \FlowCatalyst\Generated\Model\OffsetPageScheduledJobInstanceResponse::class => false,
            \FlowCatalyst\Generated\Model\OffsetPageScheduledJobResponse::class => false,
            \FlowCatalyst\Generated\Model\PermissionListResponse::class => false,
            \FlowCatalyst\Generated\Model\PermissionResponse::class => false,
            \FlowCatalyst\Generated\Model\PoolConfig::class => false,
            \FlowCatalyst\Generated\Model\PortalAppListResponse::class => false,
            \FlowCatalyst\Generated\Model\PortalAppResponse::class => false,
            \FlowCatalyst\Generated\Model\PortalUserAppGrantBody::class => false,
            \FlowCatalyst\Generated\Model\PortalUserAppRef::class => false,
            \FlowCatalyst\Generated\Model\PortalUserClientBody::class => false,
            \FlowCatalyst\Generated\Model\PortalUserListItem::class => false,
            \FlowCatalyst\Generated\Model\PortalUserListResponse::class => false,
            \FlowCatalyst\Generated\Model\PortalUserRequest::class => false,
            \FlowCatalyst\Generated\Model\PortalUserResponse::class => false,
            \FlowCatalyst\Generated\Model\PrincipalAvailableApplication::class => false,
            \FlowCatalyst\Generated\Model\PrincipalAvailableApplicationsResponse::class => false,
            \FlowCatalyst\Generated\Model\PrincipalListResponse::class => false,
            \FlowCatalyst\Generated\Model\PrincipalResponse::class => false,
            \FlowCatalyst\Generated\Model\PrincipalRoleAssignmentDTO::class => false,
            \FlowCatalyst\Generated\Model\PrincipalRoleListResponse::class => false,
            \FlowCatalyst\Generated\Model\PrincipalVersionResponse::class => false,
            \FlowCatalyst\Generated\Model\ProcessListResponse::class => false,
            \FlowCatalyst\Generated\Model\ProcessResponse::class => false,
            \FlowCatalyst\Generated\Model\ProvisionLoginClientRequest::class => false,
            \FlowCatalyst\Generated\Model\PublicAllowedResponse::class => false,
            \FlowCatalyst\Generated\Model\QueueConfig::class => false,
            \FlowCatalyst\Generated\Model\RawDispatchJobResponse::class => false,
            \FlowCatalyst\Generated\Model\RawEventResponse::class => false,
            \FlowCatalyst\Generated\Model\RegenerateAuthTokenResponse::class => false,
            \FlowCatalyst\Generated\Model\RegenerateSigningSecretResponse::class => false,
            \FlowCatalyst\Generated\Model\RegisterBeginRequest::class => false,
            \FlowCatalyst\Generated\Model\RegisterBeginResponse::class => false,
            \FlowCatalyst\Generated\Model\RegisterCompleteRequest::class => false,
            \FlowCatalyst\Generated\Model\RegisterCompleteResponse::class => false,
            \FlowCatalyst\Generated\Model\RequestDTO::class => false,
            \FlowCatalyst\Generated\Model\RequeueRequest::class => false,
            \FlowCatalyst\Generated\Model\RequeueResponse::class => false,
            \FlowCatalyst\Generated\Model\ResetPasswordRequest::class => false,
            \FlowCatalyst\Generated\Model\RoleAssignmentDTO::class => false,
            \FlowCatalyst\Generated\Model\RoleListResponse::class => false,
            \FlowCatalyst\Generated\Model\RolePermissionListResponse::class => false,
            \FlowCatalyst\Generated\Model\RoleResponse::class => false,
            \FlowCatalyst\Generated\Model\RolesAssignedResponse::class => false,
            \FlowCatalyst\Generated\Model\RotateOAuthClientSecretRequest::class => false,
            \FlowCatalyst\Generated\Model\RotateOAuthClientSecretResponse::class => false,
            \FlowCatalyst\Generated\Model\RouterConfig::class => false,
            \FlowCatalyst\Generated\Model\ScheduledJobInstanceLogResponse::class => false,
            \FlowCatalyst\Generated\Model\ScheduledJobInstanceResponse::class => false,
            \FlowCatalyst\Generated\Model\ScheduledJobResponse::class => false,
            \FlowCatalyst\Generated\Model\SearchClientRequest::class => false,
            \FlowCatalyst\Generated\Model\SendPasswordResetInputBody::class => false,
            \FlowCatalyst\Generated\Model\ServiceAccountListResponse::class => false,
            \FlowCatalyst\Generated\Model\ServiceAccountOAuthSecrets::class => false,
            \FlowCatalyst\Generated\Model\ServiceAccountResponse::class => false,
            \FlowCatalyst\Generated\Model\ServiceAccountRoleListResponse::class => false,
            \FlowCatalyst\Generated\Model\ServiceAccountRolesAssignedResponse::class => false,
            \FlowCatalyst\Generated\Model\ServiceAccountTokenResponse::class => false,
            \FlowCatalyst\Generated\Model\ServiceAccountWebhookSecrets::class => false,
            \FlowCatalyst\Generated\Model\SetApplicationAccessResponse::class => false,
            \FlowCatalyst\Generated\Model\SetDeveloperCredentialResponse::class => false,
            \FlowCatalyst\Generated\Model\SetPropertyRequest::class => false,
            \FlowCatalyst\Generated\Model\SpecVersionResponse::class => false,
            \FlowCatalyst\Generated\Model\StatusChangeRequest::class => false,
            \FlowCatalyst\Generated\Model\StatusChangeResponse::class => false,
            \FlowCatalyst\Generated\Model\SubscriptionListResponse::class => false,
            \FlowCatalyst\Generated\Model\SubscriptionResponse::class => false,
            \FlowCatalyst\Generated\Model\SuccessResponse::class => false,
            \FlowCatalyst\Generated\Model\SuspendClientRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncDispatchPoolInputRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncDispatchPoolsRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncDocInputRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncDocsRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncEventTypeInputRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncEventTypesRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncOpenApiSpecResponse::class => false,
            \FlowCatalyst\Generated\Model\SyncOpenapiRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncPrincipalInputRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncPrincipalsRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncProcessInputRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncProcessesByBodyRequest::class => false,
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
            \FlowCatalyst\Generated\Model\SyncUserInput::class => false,
            \FlowCatalyst\Generated\Model\SyncUsersRequest::class => false,
            \FlowCatalyst\Generated\Model\SyncUsersResponse::class => false,
            \FlowCatalyst\Generated\Model\UpdateAnchorDomainRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateApplicationRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateAuthConfigRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateClientApplicationsRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateClientRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateConnectionRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateDispatchPoolRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateEventTypeRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateIdentityProviderRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateMappingRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateOAuthClientRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdatePortalAppRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdatePrincipalRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateProcessRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateRoleRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateScheduledJobRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateServiceAccountRequest::class => false,
            \FlowCatalyst\Generated\Model\UpdateSubscriptionRequest::class => false,
            \FlowCatalyst\Generated\Model\WebauthnAuthenticateCompleteResponse::class => false,
            \FlowCatalyst\Generated\Model\WebauthnCredentialSummary::class => false,
            \FlowCatalyst\Generated\Model\WebhookCredentialsDTO::class => false,
            \FlowCatalyst\Generated\Model\WriteInstanceLogRequest::class => false,
            \Jane\Component\JsonSchemaRuntime\Reference::class => false,
        ];
    }
}