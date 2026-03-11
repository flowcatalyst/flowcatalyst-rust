//! TSID Generator
//!
//! Generates Time-Sorted IDs as Crockford Base32 strings.
//! Compatible with Java's TsidGenerator for cross-platform ID consistency.
//!
//! Typed IDs follow the format `{prefix}_{tsid}` (e.g., `clt_0HZXEQ5Y8JY5Z`).

use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Crockford Base32 alphabet (excludes I, L, O, U)
const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

static COUNTER: AtomicU16 = AtomicU16::new(0);

/// Well-known entity type prefixes matching the FlowCatalyst platform.
///
/// Use these with [`TsidGenerator::generate`] for platform-compatible typed IDs.
/// For custom entity types, use [`TsidGenerator::generate_with_prefix`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityType {
    Client,
    Principal,
    Application,
    ServiceAccount,
    Role,
    Permission,
    OAuthClient,
    AuthCode,
    LoginAttempt,
    ClientAuthConfig,
    AppClientConfig,
    IdpRoleMapping,
    CorsOrigin,
    AnchorDomain,
    IdentityProvider,
    EmailDomainMapping,
    ClientAccessGrant,
    EventType,
    Event,
    EventRead,
    Connection,
    Subscription,
    DispatchPool,
    DispatchJob,
    DispatchJobRead,
    Schema,
    AuditLog,
    PlatformConfig,
    ConfigAccess,
    PasswordResetToken,
}

impl EntityType {
    /// Returns the 3-character prefix for this entity type.
    pub fn prefix(&self) -> &'static str {
        match self {
            EntityType::Client => "clt",
            EntityType::Principal => "prn",
            EntityType::Application => "app",
            EntityType::ServiceAccount => "sac",
            EntityType::Role => "rol",
            EntityType::Permission => "prm",
            EntityType::OAuthClient => "oac",
            EntityType::AuthCode => "acd",
            EntityType::LoginAttempt => "lat",
            EntityType::ClientAuthConfig => "cac",
            EntityType::AppClientConfig => "apc",
            EntityType::IdpRoleMapping => "irm",
            EntityType::CorsOrigin => "cor",
            EntityType::AnchorDomain => "anc",
            EntityType::IdentityProvider => "idp",
            EntityType::EmailDomainMapping => "edm",
            EntityType::ClientAccessGrant => "gnt",
            EntityType::EventType => "evt",
            EntityType::Event => "evn",
            EntityType::EventRead => "evr",
            EntityType::Connection => "con",
            EntityType::Subscription => "sub",
            EntityType::DispatchPool => "dpl",
            EntityType::DispatchJob => "djb",
            EntityType::DispatchJobRead => "djr",
            EntityType::Schema => "sch",
            EntityType::AuditLog => "aud",
            EntityType::PlatformConfig => "pcf",
            EntityType::ConfigAccess => "cfa",
            EntityType::PasswordResetToken => "prt",
        }
    }
}

/// TSID Generator for creating unique, time-sorted identifiers.
///
/// # Examples
///
/// ```
/// use fc_sdk::tsid::{TsidGenerator, EntityType};
///
/// // Generate a typed ID for a known entity type
/// let event_id = TsidGenerator::generate(EntityType::Event);
/// assert!(event_id.starts_with("evn_"));
///
/// // Generate with a custom prefix for your own entity types
/// let order_id = TsidGenerator::generate_with_prefix("ord");
/// assert!(order_id.starts_with("ord_"));
///
/// // Generate an untyped ID (e.g., for execution IDs, trace IDs)
/// let trace_id = TsidGenerator::generate_untyped();
/// assert_eq!(trace_id.len(), 13);
/// ```
pub struct TsidGenerator;

impl TsidGenerator {
    /// Generate a raw TSID as a Crockford Base32 string (13 characters).
    fn generate_raw() -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;

        let counter = COUNTER.fetch_add(1, Ordering::SeqCst) as u64;
        let random: u64 = rand_u16() as u64 & 0x3FF;

        // Combine: timestamp (42 bits) | random (10 bits) | counter (12 bits)
        let tsid = ((now & 0x3FFFFFFFFFF) << 22) | (random << 12) | (counter & 0xFFF);

        encode_crockford(tsid)
    }

    /// Generate a typed ID with a platform entity prefix: `{prefix}_{tsid}`.
    ///
    /// Example: `generate(EntityType::Client)` → `"clt_0HZXEQ5Y8JY5Z"`
    pub fn generate(entity_type: EntityType) -> String {
        format!("{}_{}", entity_type.prefix(), Self::generate_raw())
    }

    /// Generate a typed ID with a custom prefix: `{prefix}_{tsid}`.
    ///
    /// Use this for application-specific entity types not covered by [`EntityType`].
    ///
    /// Example: `generate_with_prefix("ord")` → `"ord_0HZXEQ5Y8JY5Z"`
    pub fn generate_with_prefix(prefix: &str) -> String {
        format!("{}_{}", prefix, Self::generate_raw())
    }

    /// Generate an untyped ID for non-entity contexts (execution IDs, trace IDs, etc.)
    pub fn generate_untyped() -> String {
        Self::generate_raw()
    }

    /// Convert a TSID string to its numeric representation.
    /// Handles both typed (`clt_0HZXEQ5Y8JY5Z`) and raw (`0HZXEQ5Y8JY5Z`) formats.
    pub fn to_long(tsid_str: &str) -> Option<i64> {
        let raw = if tsid_str.len() > 14 && tsid_str.contains('_') {
            tsid_str.split('_').nth(1)?
        } else {
            tsid_str
        };
        decode_crockford(raw).map(|v| v as i64)
    }

    /// Convert a numeric TSID to its string representation (raw, no prefix).
    pub fn from_long(value: i64) -> String {
        encode_crockford(value as u64)
    }
}

/// Encode a 64-bit value to Crockford Base32 (13 characters).
fn encode_crockford(mut value: u64) -> String {
    let mut result = [b'0'; 13];

    for i in (0..13).rev() {
        result[i] = ALPHABET[(value & 0x1F) as usize];
        value >>= 5;
    }

    String::from_utf8(result.to_vec()).unwrap()
}

/// Decode a Crockford Base32 string to 64-bit value.
fn decode_crockford(s: &str) -> Option<u64> {
    if s.len() != 13 {
        return None;
    }

    let mut result: u64 = 0;
    for c in s.chars() {
        let c = c.to_ascii_uppercase();
        let val = match c {
            '0'..='9' => c as u64 - '0' as u64,
            'A'..='H' => c as u64 - 'A' as u64 + 10,
            'J'..='K' => c as u64 - 'J' as u64 + 18,
            'M'..='N' => c as u64 - 'M' as u64 + 20,
            'P'..='T' => c as u64 - 'P' as u64 + 22,
            'V'..='Z' => c as u64 - 'V' as u64 + 27,
            _ => return None,
        };
        result = (result << 5) | val;
    }

    Some(result)
}

/// Simple random u16 using system time and counter.
fn rand_u16() -> u16 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let counter = COUNTER.load(Ordering::Relaxed) as u64;
    ((now ^ (counter.wrapping_mul(0x5851F42D4C957F2D))) & 0xFFFF) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_typed_id() {
        let id = TsidGenerator::generate(EntityType::Client);
        assert_eq!(id.len(), 17);
        assert!(id.starts_with("clt_"));
    }

    #[test]
    fn test_generate_custom_prefix() {
        let id = TsidGenerator::generate_with_prefix("ord");
        assert_eq!(id.len(), 17);
        assert!(id.starts_with("ord_"));
    }

    #[test]
    fn test_generate_untyped_id() {
        let id = TsidGenerator::generate_untyped();
        assert_eq!(id.len(), 13);
    }

    #[test]
    fn test_uniqueness() {
        let mut ids = std::collections::HashSet::new();
        for _ in 0..1000 {
            let id = TsidGenerator::generate(EntityType::Client);
            assert!(ids.insert(id), "Duplicate TSID generated");
        }
    }

    #[test]
    fn test_round_trip_typed() {
        let id = TsidGenerator::generate(EntityType::Client);
        let num = TsidGenerator::to_long(&id).unwrap();
        let back = TsidGenerator::from_long(num);
        assert_eq!(&id[4..], back);
    }

    #[test]
    fn test_sortability() {
        let id1 = TsidGenerator::generate(EntityType::Client);
        std::thread::sleep(std::time::Duration::from_millis(1));
        let id2 = TsidGenerator::generate(EntityType::Client);
        assert!(id1 < id2, "TSIDs should be lexicographically sortable");
    }
}
