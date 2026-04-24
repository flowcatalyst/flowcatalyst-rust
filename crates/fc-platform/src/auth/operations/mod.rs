//! Auth Operations
//!
//! Use cases for anchor domain and auth config management.

pub mod events;
pub mod create_anchor_domain;
pub mod update_anchor_domain;
pub mod delete_anchor_domain;
pub mod create_auth_config;
pub mod update_auth_config;
pub mod delete_auth_config;
pub mod create_idp_role_mapping;
pub mod delete_idp_role_mapping;
pub mod create_oauth_client;
pub mod update_oauth_client;
pub mod delete_oauth_client;
pub mod activate_oauth_client;
pub mod deactivate_oauth_client;
pub mod rotate_oauth_client_secret;

pub use events::*;
pub use create_anchor_domain::{CreateAnchorDomainCommand, CreateAnchorDomainUseCase};
pub use update_anchor_domain::{UpdateAnchorDomainCommand, UpdateAnchorDomainUseCase};
pub use delete_anchor_domain::{DeleteAnchorDomainCommand, DeleteAnchorDomainUseCase};
pub use create_auth_config::{CreateAuthConfigCommand, CreateAuthConfigUseCase};
pub use update_auth_config::{UpdateAuthConfigCommand, UpdateAuthConfigUseCase};
pub use delete_auth_config::{DeleteAuthConfigCommand, DeleteAuthConfigUseCase};
pub use create_idp_role_mapping::{CreateIdpRoleMappingCommand, CreateIdpRoleMappingUseCase};
pub use delete_idp_role_mapping::{DeleteIdpRoleMappingCommand, DeleteIdpRoleMappingUseCase};
pub use create_oauth_client::{CreateOAuthClientCommand, CreateOAuthClientUseCase};
pub use update_oauth_client::{UpdateOAuthClientCommand, UpdateOAuthClientUseCase};
pub use delete_oauth_client::{DeleteOAuthClientCommand, DeleteOAuthClientUseCase};
pub use activate_oauth_client::{ActivateOAuthClientCommand, ActivateOAuthClientUseCase};
pub use deactivate_oauth_client::{DeactivateOAuthClientCommand, DeactivateOAuthClientUseCase};
pub use rotate_oauth_client_secret::{RotateOAuthClientSecretCommand, RotateOAuthClientSecretUseCase};
