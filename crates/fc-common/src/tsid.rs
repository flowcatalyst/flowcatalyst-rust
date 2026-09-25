//! TSID Generator
//!
//! Generates Time-Sorted IDs as Crockford Base32 strings.
//! The encoding matches the other FlowCatalyst implementations, so IDs are
//! interchangeable across them.
//!
//! Typed IDs follow the format `{prefix}_{tsid}` (e.g., `clt_0HZXEQ5Y8JY5Z`).

use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Crockford Base32 alphabet (excludes I, L, O, U)
const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Mixes the pseudo-random bits; not part of the id layout.
static COUNTER: AtomicU16 = AtomicU16::new(0);

/// The last `(millisecond << 12) | sequence` handed out (Go `nextMsSeq`).
static STATE: AtomicU64 = AtomicU64::new(0);

/// Well-known entity type prefixes matching the FlowCatalyst platform.
///
/// Use these with [`generate`] for platform-compatible typed IDs.
/// For custom entity types, use [`generate_with_prefix`].
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
    WebauthnCredential,
    ScheduledJob,
    ScheduledJobInstance,
    ScheduledJobInstanceLog,
    ApplicationOpenApiSpec,
    Process,
    // The function registry (Java `EntityType.java:56-59`).
    Function,
    FunctionVersion,
    FunctionDomain,
    FunctionRoute,
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
            EntityType::WebauthnCredential => "pkc",
            EntityType::ScheduledJob => "sjb",
            EntityType::ScheduledJobInstance => "sji",
            EntityType::ScheduledJobInstanceLog => "sjl",
            EntityType::ApplicationOpenApiSpec => "oas",
            EntityType::Process => "prc",
            EntityType::Function => "fnc",
            EntityType::FunctionVersion => "fnv",
            EntityType::FunctionDomain => "fnd",
            EntityType::FunctionRoute => "fnr",
        }
    }
}

/// Generate a raw TSID as a Crockford Base32 string (13 characters).
///
/// Layout, as Go's `pkg/fcsdk/tsid`: timestamp (42 bits, ms since the epoch)
/// | sequence (12 bits) | random (10 bits). The sequence starts at a random
/// value each millisecond and increments within it, so ids made in one
/// process are strictly increasing: rows inserted in creation order sort in
/// that order when ordered by id (the dispatch scheduler's final tie-break
/// within a message group).
fn generate_raw() -> String {
    let (ms, seq) = next_ms_seq();
    COUNTER.fetch_add(1, Ordering::Relaxed);
    let random = rand_u16() as u64 & 0x3FF;
    let tsid = ((ms & 0x3FF_FFFF_FFFF) << 22) | ((seq & 0xFFF) << 10) | random;
    encode_crockford(tsid)
}

/// The next `(millisecond, sequence)` pair (Go `nextMsSeq`): a new
/// millisecond starts at a random sequence; within one the sequence
/// increments; an exhausted sequence borrows the next millisecond.
fn next_ms_seq() -> (u64, u64) {
    loop {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;
        let old = STATE.load(Ordering::SeqCst);
        let last_ms = old >> 12;
        let last_seq = old & 0xFFF;
        let (ms, seq) = if now > last_ms {
            (now, rand_u16() as u64 & 0xFFF)
        } else if last_seq < 0xFFF {
            (last_ms, last_seq + 1)
        } else {
            (last_ms + 1, rand_u16() as u64 & 0xFFF)
        };
        if STATE
            .compare_exchange(old, (ms << 12) | seq, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return (ms, seq);
        }
    }
}

/// Generate a typed ID with a platform entity prefix: `{prefix}_{tsid}`.
///
/// ```
/// use fc_common::tsid::{self, EntityType};
///
/// let client_id = tsid::generate(EntityType::Client);
/// assert!(client_id.starts_with("clt_"));
/// ```
pub fn generate(entity_type: EntityType) -> String {
    format!("{}_{}", entity_type.prefix(), generate_raw())
}

/// Generate a typed ID with a custom prefix: `{prefix}_{tsid}`.
///
/// Use this for application-specific entity types not covered by [`EntityType`].
///
/// ```
/// let order_id = fc_common::tsid::generate_with_prefix("ord");
/// assert!(order_id.starts_with("ord_"));
/// ```
pub fn generate_with_prefix(prefix: &str) -> String {
    format!("{}_{}", prefix, generate_raw())
}

/// Generate an untyped ID for non-entity contexts (execution IDs, trace IDs, etc.)
///
/// ```
/// assert_eq!(fc_common::tsid::generate_untyped().len(), 13);
/// ```
pub fn generate_untyped() -> String {
    generate_raw()
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

    /// Ids from one process are strictly increasing, even many within one
    /// millisecond (Go parity; the scheduler orders a group's jobs by id last).
    #[test]
    fn ids_are_strictly_increasing() {
        let ids: Vec<String> = (0..10_000).map(|_| generate_untyped()).collect();
        for pair in ids.windows(2) {
            assert!(pair[0] < pair[1], "{} !< {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn test_generate_typed_id() {
        let id = generate(EntityType::Client);
        assert_eq!(id.len(), 17);
        assert!(id.starts_with("clt_"));
    }

    // Java `EntityType.java:56-59`.
    #[test]
    fn function_prefixes_match_java() {
        assert_eq!(EntityType::Function.prefix(), "fnc");
        assert_eq!(EntityType::FunctionVersion.prefix(), "fnv");
        assert_eq!(EntityType::FunctionDomain.prefix(), "fnd");
        assert_eq!(EntityType::FunctionRoute.prefix(), "fnr");
        let id = generate(EntityType::FunctionVersion);
        assert_eq!(id.len(), 17);
        assert!(id.starts_with("fnv_"));
    }

    #[test]
    fn test_generate_custom_prefix() {
        let id = generate_with_prefix("ord");
        assert_eq!(id.len(), 17);
        assert!(id.starts_with("ord_"));
    }

    #[test]
    fn test_generate_untyped_id() {
        let id = generate_untyped();
        assert_eq!(id.len(), 13);
    }

    #[test]
    fn test_uniqueness() {
        let mut ids = std::collections::HashSet::new();
        for _ in 0..1000 {
            let id = generate(EntityType::Client);
            assert!(ids.insert(id), "Duplicate TSID generated");
        }
    }

    #[test]
    fn test_round_trip_typed() {
        let id = generate(EntityType::Client);
        let num = to_long(&id).unwrap();
        let back = from_long(num);
        assert_eq!(&id[4..], back);
    }

    #[test]
    fn test_round_trip_raw() {
        let id = generate_untyped();
        let num = to_long(&id).unwrap();
        let back = from_long(num);
        assert_eq!(id, back);
    }

    #[test]
    fn test_sortability() {
        let id1 = generate(EntityType::Client);
        std::thread::sleep(std::time::Duration::from_millis(1));
        let id2 = generate(EntityType::Client);
        assert!(id1 < id2, "TSIDs should be lexicographically sortable");
    }
}
