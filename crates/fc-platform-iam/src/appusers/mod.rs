//! The user and invitation administration an application performs for its own
//! users, as its own service account (Go F20): list and read the application's
//! users, invite one, and list, resend and revoke pending invitations.

pub mod api;
pub mod repository;

pub use api::AppUsersState;
pub use repository::AppUsersRepository;
