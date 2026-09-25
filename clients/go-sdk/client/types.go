package client

// SyncResult is the response body returned by every per-resource sync
// endpoint. Mirrors crates/fc-sdk/src/client::SyncResult.
type SyncResult struct {
	ApplicationCode string   `json:"applicationCode"`
	Created         uint32   `json:"created"`
	Updated         uint32   `json:"updated"`
	Deleted         uint32   `json:"deleted"`
	SyncedCodes     []string `json:"syncedCodes"`
	// PasswordHashIgnored — principal sync only: the emails whose
	// PasswordHash the platform ignored because the user already existed.
	// A sync uses a hash only to create a user, never to change an existing
	// one's password. Empty (omitted by the platform) otherwise.
	PasswordHashIgnored []string `json:"passwordHashIgnored,omitempty"`
}
