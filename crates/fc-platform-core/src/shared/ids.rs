//! The id kinds: one marker per entity and its `XxxId` alias. Written out, one
//! after another, so a kind is a plain `enum` you can grep and jump to.

use crate::shared::id::{Id, IdKind};
use crate::shared::tsid::EntityType;

#[derive(Debug, Clone, Copy)]
pub enum ClientKind {}
impl IdKind for ClientKind {
    const ENTITY: EntityType = EntityType::Client;
}
pub type ClientId = Id<ClientKind>;

#[derive(Debug, Clone, Copy)]
pub enum PrincipalKind {}
impl IdKind for PrincipalKind {
    const ENTITY: EntityType = EntityType::Principal;
}
pub type PrincipalId = Id<PrincipalKind>;

#[derive(Debug, Clone, Copy)]
pub enum ServiceAccountKind {}
impl IdKind for ServiceAccountKind {
    const ENTITY: EntityType = EntityType::ServiceAccount;
}
pub type ServiceAccountId = Id<ServiceAccountKind>;

#[derive(Debug, Clone, Copy)]
pub enum ApplicationKind {}
impl IdKind for ApplicationKind {
    const ENTITY: EntityType = EntityType::Application;
}
pub type ApplicationId = Id<ApplicationKind>;

#[derive(Debug, Clone, Copy)]
pub enum AppClientConfigKind {}
impl IdKind for AppClientConfigKind {
    const ENTITY: EntityType = EntityType::AppClientConfig;
}
pub type AppClientConfigId = Id<AppClientConfigKind>;

#[derive(Debug, Clone, Copy)]
pub enum PlatformConfigKind {}
impl IdKind for PlatformConfigKind {
    const ENTITY: EntityType = EntityType::PlatformConfig;
}
pub type PlatformConfigId = Id<PlatformConfigKind>;

#[derive(Debug, Clone, Copy)]
pub enum ApplicationOpenApiSpecKind {}
impl IdKind for ApplicationOpenApiSpecKind {
    const ENTITY: EntityType = EntityType::ApplicationOpenApiSpec;
}
pub type ApplicationOpenApiSpecId = Id<ApplicationOpenApiSpecKind>;

#[derive(Debug, Clone, Copy)]
pub enum RoleKind {}
impl IdKind for RoleKind {
    const ENTITY: EntityType = EntityType::Role;
}
pub type RoleId = Id<RoleKind>;

#[derive(Debug, Clone, Copy)]
pub enum PermissionKind {}
impl IdKind for PermissionKind {
    const ENTITY: EntityType = EntityType::Permission;
}
pub type PermissionId = Id<PermissionKind>;

#[derive(Debug, Clone, Copy)]
pub enum ConnectionKind {}
impl IdKind for ConnectionKind {
    const ENTITY: EntityType = EntityType::Connection;
}
pub type ConnectionId = Id<ConnectionKind>;

#[derive(Debug, Clone, Copy)]
pub enum DispatchPoolKind {}
impl IdKind for DispatchPoolKind {
    const ENTITY: EntityType = EntityType::DispatchPool;
}
pub type DispatchPoolId = Id<DispatchPoolKind>;

#[derive(Debug, Clone, Copy)]
pub enum EventTypeKind {}
impl IdKind for EventTypeKind {
    const ENTITY: EntityType = EntityType::EventType;
}
pub type EventTypeId = Id<EventTypeKind>;

#[derive(Debug, Clone, Copy)]
pub enum SubscriptionKind {}
impl IdKind for SubscriptionKind {
    const ENTITY: EntityType = EntityType::Subscription;
}
pub type SubscriptionId = Id<SubscriptionKind>;

#[derive(Debug, Clone, Copy)]
pub enum ProcessKind {}
impl IdKind for ProcessKind {
    const ENTITY: EntityType = EntityType::Process;
}
pub type ProcessId = Id<ProcessKind>;

#[derive(Debug, Clone, Copy)]
pub enum ScheduledJobKind {}
impl IdKind for ScheduledJobKind {
    const ENTITY: EntityType = EntityType::ScheduledJob;
}
pub type ScheduledJobId = Id<ScheduledJobKind>;

#[derive(Debug, Clone, Copy)]
pub enum OAuthClientKind {}
impl IdKind for OAuthClientKind {
    const ENTITY: EntityType = EntityType::OAuthClient;
}
pub type OAuthClientId = Id<OAuthClientKind>;

#[derive(Debug, Clone, Copy)]
pub enum IdentityProviderKind {}
impl IdKind for IdentityProviderKind {
    const ENTITY: EntityType = EntityType::IdentityProvider;
}
pub type IdentityProviderId = Id<IdentityProviderKind>;

#[derive(Debug, Clone, Copy)]
pub enum EmailDomainMappingKind {}
impl IdKind for EmailDomainMappingKind {
    const ENTITY: EntityType = EntityType::EmailDomainMapping;
}
pub type EmailDomainMappingId = Id<EmailDomainMappingKind>;

#[derive(Debug, Clone, Copy)]
pub enum CorsOriginKind {}
impl IdKind for CorsOriginKind {
    const ENTITY: EntityType = EntityType::CorsOrigin;
}
pub type CorsOriginId = Id<CorsOriginKind>;

#[derive(Debug, Clone, Copy)]
pub enum AnchorDomainKind {}
impl IdKind for AnchorDomainKind {
    const ENTITY: EntityType = EntityType::AnchorDomain;
}
pub type AnchorDomainId = Id<AnchorDomainKind>;

#[derive(Debug, Clone, Copy)]
pub enum ClientAuthConfigKind {}
impl IdKind for ClientAuthConfigKind {
    const ENTITY: EntityType = EntityType::ClientAuthConfig;
}
pub type ClientAuthConfigId = Id<ClientAuthConfigKind>;

#[derive(Debug, Clone, Copy)]
pub enum IdpRoleMappingKind {}
impl IdKind for IdpRoleMappingKind {
    const ENTITY: EntityType = EntityType::IdpRoleMapping;
}
pub type IdpRoleMappingId = Id<IdpRoleMappingKind>;

#[derive(Debug, Clone, Copy)]
pub enum ScheduledJobInstanceKind {}
impl IdKind for ScheduledJobInstanceKind {
    const ENTITY: EntityType = EntityType::ScheduledJobInstance;
}
pub type ScheduledJobInstanceId = Id<ScheduledJobInstanceKind>;

#[derive(Debug, Clone, Copy)]
pub enum ScheduledJobInstanceLogKind {}
impl IdKind for ScheduledJobInstanceLogKind {
    const ENTITY: EntityType = EntityType::ScheduledJobInstanceLog;
}
pub type ScheduledJobInstanceLogId = Id<ScheduledJobInstanceLogKind>;

#[derive(Debug, Clone, Copy)]
pub enum ClientAccessGrantKind {}
impl IdKind for ClientAccessGrantKind {
    const ENTITY: EntityType = EntityType::ClientAccessGrant;
}
pub type ClientAccessGrantId = Id<ClientAccessGrantKind>;

#[derive(Debug, Clone, Copy)]
pub enum PasswordResetTokenKind {}
impl IdKind for PasswordResetTokenKind {
    const ENTITY: EntityType = EntityType::PasswordResetToken;
}
pub type PasswordResetTokenId = Id<PasswordResetTokenKind>;

#[derive(Debug, Clone, Copy)]
pub enum LoginAttemptKind {}
impl IdKind for LoginAttemptKind {
    const ENTITY: EntityType = EntityType::LoginAttempt;
}
pub type LoginAttemptId = Id<LoginAttemptKind>;

#[derive(Debug, Clone, Copy)]
pub enum WebauthnCredentialKind {}
impl IdKind for WebauthnCredentialKind {
    const ENTITY: EntityType = EntityType::WebauthnCredential;
}
pub type WebauthnCredentialId = Id<WebauthnCredentialKind>;

#[derive(Debug, Clone, Copy)]
pub enum PlatformConfigAccessKind {}
impl IdKind for PlatformConfigAccessKind {
    const ENTITY: EntityType = EntityType::ConfigAccess;
}
pub type PlatformConfigAccessId = Id<PlatformConfigAccessKind>;

#[derive(Debug, Clone, Copy)]
pub enum SpecVersionKind {}
impl IdKind for SpecVersionKind {
    const ENTITY: EntityType = EntityType::Schema;
}
pub type SpecVersionId = Id<SpecVersionKind>;

#[derive(Debug, Clone, Copy)]
pub enum FunctionKind {}
impl IdKind for FunctionKind {
    const ENTITY: EntityType = EntityType::Function;
}
pub type FunctionId = Id<FunctionKind>;

#[derive(Debug, Clone, Copy)]
pub enum FunctionVersionKind {}
impl IdKind for FunctionVersionKind {
    const ENTITY: EntityType = EntityType::FunctionVersion;
}
pub type FunctionVersionId = Id<FunctionVersionKind>;

#[derive(Debug, Clone, Copy)]
pub enum FunctionDomainKind {}
impl IdKind for FunctionDomainKind {
    const ENTITY: EntityType = EntityType::FunctionDomain;
}
pub type FunctionDomainId = Id<FunctionDomainKind>;

#[derive(Debug, Clone, Copy)]
pub enum FunctionRouteKind {}
impl IdKind for FunctionRouteKind {
    const ENTITY: EntityType = EntityType::FunctionRoute;
}
pub type FunctionRouteId = Id<FunctionRouteKind>;

#[derive(Debug, Clone, Copy)]
pub enum PortalIdentityKind {}
impl IdKind for PortalIdentityKind {
    const ENTITY: EntityType = EntityType::PortalIdentity;
}
pub type PortalIdentityId = Id<PortalIdentityKind>;

#[derive(Debug, Clone, Copy)]
pub enum PortalAppKind {}
impl IdKind for PortalAppKind {
    const ENTITY: EntityType = EntityType::PortalApp;
}
pub type PortalAppId = Id<PortalAppKind>;

#[derive(Debug, Clone, Copy)]
pub enum MfaMethodKind {}
impl IdKind for MfaMethodKind {
    const ENTITY: EntityType = EntityType::MfaMethod;
}
pub type MfaMethodId = Id<MfaMethodKind>;

#[derive(Debug, Clone, Copy)]
pub enum MfaRecoveryCodeKind {}
impl IdKind for MfaRecoveryCodeKind {
    const ENTITY: EntityType = EntityType::MfaRecoveryCode;
}
pub type MfaRecoveryCodeId = Id<MfaRecoveryCodeKind>;

#[derive(Debug, Clone, Copy)]
pub enum MfaEmailPinKind {}
impl IdKind for MfaEmailPinKind {
    const ENTITY: EntityType = EntityType::MfaEmailPin;
}
pub type MfaEmailPinId = Id<MfaEmailPinKind>;

#[derive(Debug, Clone, Copy)]
pub enum MfaTrustedDeviceKind {}
impl IdKind for MfaTrustedDeviceKind {
    const ENTITY: EntityType = EntityType::MfaTrustedDevice;
}
pub type MfaTrustedDeviceId = Id<MfaTrustedDeviceKind>;
