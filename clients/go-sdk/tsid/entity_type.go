// Package tsid generates Time-Sorted IDs as Crockford Base32 strings.
// Compatible with the Rust crate fc_common::tsid, the TypeScript SDK's
// tsid module, and the Laravel SDK — all four SDKs produce the same
// wire format.
//
// Layout (64-bit): 42 bits timestamp (ms since Unix epoch) | 10 bits
// random | 12 bits process-local counter. Encoded as 13 Crockford
// Base32 characters. Typed IDs prepend a 3-char prefix and underscore,
// e.g. "clt_0HZXEQ5Y8JY5Z".
package tsid

// EntityType enumerates the well-known prefixes used across the
// FlowCatalyst platform. New variants MUST be added to all four SDKs
// (Rust crates/fc-common-types/src/tsid.rs, TypeScript, Laravel, here) so
// the wire format stays consistent — see project_go_sdk_location memo.
type EntityType int

const (
	Client EntityType = iota
	Principal
	Application
	ServiceAccount
	Role
	Permission
	OAuthClient
	AuthCode
	LoginAttempt
	ClientAuthConfig
	AppClientConfig
	IdpRoleMapping
	CorsOrigin
	AnchorDomain
	IdentityProvider
	EmailDomainMapping
	ClientAccessGrant
	EventType
	Event
	EventRead
	Connection
	Subscription
	DispatchPool
	DispatchJob
	DispatchJobRead
	Schema
	AuditLog
	PlatformConfig
	ConfigAccess
	PasswordResetToken
	WebauthnCredential
	ScheduledJob
	ScheduledJobInstance
	ScheduledJobInstanceLog
	ApplicationOpenApiSpec
	Process
)

// Prefix returns the 3-character platform prefix for the entity type.
// Mirrors crates/fc-common-types/src/tsid.rs::EntityType::prefix.
func (e EntityType) Prefix() string {
	switch e {
	case Client:
		return "clt"
	case Principal:
		return "prn"
	case Application:
		return "app"
	case ServiceAccount:
		return "sac"
	case Role:
		return "rol"
	case Permission:
		return "prm"
	case OAuthClient:
		return "oac"
	case AuthCode:
		return "acd"
	case LoginAttempt:
		return "lat"
	case ClientAuthConfig:
		return "cac"
	case AppClientConfig:
		return "apc"
	case IdpRoleMapping:
		return "irm"
	case CorsOrigin:
		return "cor"
	case AnchorDomain:
		return "anc"
	case IdentityProvider:
		return "idp"
	case EmailDomainMapping:
		return "edm"
	case ClientAccessGrant:
		return "gnt"
	case EventType:
		return "evt"
	case Event:
		return "evn"
	case EventRead:
		return "evr"
	case Connection:
		return "con"
	case Subscription:
		return "sub"
	case DispatchPool:
		return "dpl"
	case DispatchJob:
		return "djb"
	case DispatchJobRead:
		return "djr"
	case Schema:
		return "sch"
	case AuditLog:
		return "aud"
	case PlatformConfig:
		return "pcf"
	case ConfigAccess:
		return "cfa"
	case PasswordResetToken:
		return "prt"
	case WebauthnCredential:
		return "pkc"
	case ScheduledJob:
		return "sjb"
	case ScheduledJobInstance:
		return "sji"
	case ScheduledJobInstanceLog:
		return "sjl"
	case ApplicationOpenApiSpec:
		return "oas"
	case Process:
		return "prc"
	}
	return "unk"
}
