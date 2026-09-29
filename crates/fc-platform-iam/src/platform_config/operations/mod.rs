//! PlatformConfig Operations
//!
//! Use cases for platform config property management with atomic
//! event + audit log emission via UnitOfWork.

pub mod events;
pub mod grant_access;
pub mod revoke_access;
pub mod set_property;

pub use events::{
    PlatformConfigAccessGranted, PlatformConfigAccessRevoked, PlatformConfigPropertySet,
};
pub use grant_access::{GrantPlatformConfigAccessCommand, GrantPlatformConfigAccessUseCase};
pub use revoke_access::{RevokePlatformConfigAccessCommand, RevokePlatformConfigAccessUseCase};
pub use set_property::{SetPlatformConfigPropertyCommand, SetPlatformConfigPropertyUseCase};
