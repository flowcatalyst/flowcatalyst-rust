//! WebAuthn Operations — register, revoke, authenticate.

pub mod authenticate_with_passkey;
pub mod events;
pub mod register_passkey;
pub mod revoke_passkey;

pub use authenticate_with_passkey::{
    AuthenticatePasskeyCommand, AuthenticatePasskeyUseCase, AuthenticationOutcome,
};
pub use events::*;
pub use register_passkey::{RegisterPasskeyCommand, RegisterPasskeyUseCase};
pub use revoke_passkey::{RevokePasskeyCommand, RevokePasskeyUseCase};
