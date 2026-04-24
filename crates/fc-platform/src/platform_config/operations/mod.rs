//! PlatformConfig Operations
//!
//! Use cases for platform config property management with atomic
//! event + audit log emission via UnitOfWork.

pub mod events;
pub mod set_property;
pub mod grant_access;
pub mod revoke_access;

pub use events::{
    PlatformConfigPropertySet,
    PlatformConfigAccessGranted,
    PlatformConfigAccessRevoked,
};
pub use set_property::{SetPlatformConfigPropertyCommand, SetPlatformConfigPropertyUseCase};
pub use grant_access::{GrantPlatformConfigAccessCommand, GrantPlatformConfigAccessUseCase};
pub use revoke_access::{RevokePlatformConfigAccessCommand, RevokePlatformConfigAccessUseCase};
