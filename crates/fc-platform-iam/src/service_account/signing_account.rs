//! A service account as the signing-reach check sees it
//! (`service_account::signing_reach`, fc-platform-messaging): which clients
//! it reaches and the application it belongs to. The repository loads it.

/// Which clients a service account reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountReach {
    /// No client: an anchor-tier account.
    Anchor,
    /// These clients (never empty).
    Clients(Vec<String>),
}

impl AccountReach {
    /// An empty client list is no client: anchor-tier.
    pub fn of_clients(clients: Vec<String>) -> AccountReach {
        if clients.is_empty() {
            AccountReach::Anchor
        } else {
            AccountReach::Clients(clients)
        }
    }
}

/// A service account as the reach check sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningAccount {
    pub code: String,
    /// The application the account belongs to, if any.
    pub application_id: Option<String>,
    pub reach: AccountReach,
}
