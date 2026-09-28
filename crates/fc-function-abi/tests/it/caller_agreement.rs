//! The guest-side authorisation rules against the Rust platform's own,
//! mirroring Java's `server/src/test/java/io/flowcatalyst/platform/function/CallerClaimsAgreeWithPlatformTest.java`
//! (`docs/spec/function-caller-claims.md` §2 in the Java repo): every row is
//! run through `fc-platform`'s `AuthContext` / `matches_pattern` and through
//! `Principal`'s methods, built from the same claims, and the answers must
//! agree.
//!
//! The guest follows **Java's** rule, because a function must answer as the
//! Java platform would. Where the Rust platform's rule is narrower (it only
//! understands four-segment codes), `known_divergences` pins the difference so
//! it cannot change silently on either side.

use std::collections::{BTreeSet, HashSet};

use fc_function_abi::{permission_matches, Principal};
use fc_platform::role::entity::matches_pattern;
use fc_platform::{AuthContext, PrincipalType, UserScope};

fn principal(
    tier: Option<&str>,
    clients: &[&str],
    applications: &[&str],
    all_applications: bool,
    permissions: &[&str],
) -> Principal {
    Principal {
        id: "prn_1".into(),
        principal_type: "user".into(),
        tier: tier.map(str::to_string),
        clients: clients.iter().map(|s| s.to_string()).collect(),
        roles: vec![],
        applications: applications.iter().map(|s| s.to_string()).collect(),
        all_applications,
        permissions: permissions
            .iter()
            .map(|s| s.to_string())
            .collect::<BTreeSet<_>>(),
    }
}

fn scope(tier: Option<&str>) -> UserScope {
    match tier {
        Some("ANCHOR") => UserScope::Anchor,
        Some("PARTNER") => UserScope::Partner,
        // The Rust platform has no "no tier"; its default scope is Client.
        _ => UserScope::Client,
    }
}

fn auth_context(tier: Option<&str>, clients: &[&str], permissions: &[&str]) -> AuthContext {
    AuthContext {
        principal_id: "prn_1".into(),
        principal_type: PrincipalType::User,
        scope: scope(tier),
        email: None,
        name: "n".into(),
        accessible_clients: clients.iter().map(|s| s.to_string()).collect(),
        permissions: permissions
            .iter()
            .map(|s| s.to_string())
            .collect::<HashSet<_>>(),
        roles: vec![],
        credential: fc_platform::shared::authorization_service::Credential::BearerToken,
    }
}

/// Java's `hasPermissionAgreesWithPermissionGrants` table, verbatim.
const PERMISSION_ROWS: &[(&str, &str)] = &[
    // exact match
    ("a:b:c:d", "a:b:c:d"),
    // wildcard segment (leading, middle, trailing)
    ("*:b:c:d", "a:b:c:d"),
    ("a:*:c:d", "a:b:c:d"),
    ("a:b:c:*", "a:b:c:d"),
    // wildcard in the middle of a real code
    (
        "platform:*:event-type:view",
        "platform:messaging:event-type:view",
    ),
    // all-wildcard (super-admin)
    ("*:*:*:*", "platform:messaging:event-type:view"),
    // a wildcard in the REQUIRED code grants nothing
    ("a:b:c:d", "a:*:c:d"),
    ("a:b:c:d", "*:*:*:*"),
    // mismatch: a non-wildcard segment differs
    ("a:b:c:d", "a:b:c:e"),
    // different segment counts (fewer / more)
    ("a:b:c", "a:b:c:d"),
    ("a:b:c:d:e", "a:b:c:d"),
    // empty strings
    ("", ""),
    ("", "a:b:c:d"),
];

#[test]
fn has_permission_agrees_with_the_platform() {
    for &(held, required) in PERMISSION_ROWS {
        let via_platform = auth_context(Some("CLIENT"), &[], &[held]).has_permission(required);
        let via_guest =
            principal(Some("CLIENT"), &[], &[], false, &[held]).has_permission(required);
        assert_eq!(
            via_guest, via_platform,
            "held={held:?} required={required:?}"
        );
    }
}

#[test]
fn the_matcher_agrees_with_matches_pattern_on_four_segment_codes() {
    for &(held, required) in PERMISSION_ROWS {
        if held.split(':').count() == 4 && required.split(':').count() == 4 {
            // matches_pattern(permission, pattern): the held code is the pattern.
            assert_eq!(
                permission_matches(held, required),
                matches_pattern(required, held),
                "held={held:?} required={required:?}"
            );
        }
    }
}

/// Where the guest (Java's rule) and the Rust platform answer differently.
/// Java's `Permission.matches` accepts wildcards at any segment count; the
/// Rust platform's `matches_pattern` only at four. No permission the platform
/// issues has another length, so no real token is affected.
#[test]
fn known_divergences() {
    let rows = [("a:*:c", "a:b:c"), ("*", "x"), ("a:b:c:d:*", "a:b:c:d:e")];
    for (held, required) in rows {
        let via_platform = auth_context(Some("CLIENT"), &[], &[held]).has_permission(required);
        let via_guest =
            principal(Some("CLIENT"), &[], &[], false, &[held]).has_permission(required);
        assert!(via_guest, "Java grants held={held:?} required={required:?}");
        assert!(
            !via_platform,
            "the Rust platform refuses held={held:?} required={required:?}"
        );
    }
}

#[test]
fn is_anchor_agrees_with_the_platform() {
    for tier in [Some("ANCHOR"), Some("PARTNER"), Some("CLIENT"), None] {
        assert_eq!(
            principal(tier, &[], &[], false, &[]).is_anchor(),
            auth_context(tier, &[], &[]).is_anchor(),
            "tier={tier:?}"
        );
    }
}

/// Java's `canAccessClientAgreesWithAuthContext` rows. The Rust platform
/// grants an anchor through the `*` its tokens carry in `clients`
/// (`auth_service.rs`: `UserScope::Anchor => vec!["*"]`), Java through the
/// tier; for the claims each platform actually mints they agree.
#[test]
fn can_access_client_agrees_with_the_platform() {
    let rows: [(&str, &[&str], &str); 5] = [
        ("ANCHOR", &["*"], "some-client"),
        ("CLIENT", &["clt_1"], "clt_1"),
        ("CLIENT", &["clt_1"], "clt_2"),
        ("CLIENT", &["clt_1", "clt_2"], "clt_2"),
        ("PARTNER", &[], "clt_1"),
    ];
    for (tier, clients, asked) in rows {
        assert_eq!(
            principal(Some(tier), clients, &[], false, &[]).can_access_client(asked),
            auth_context(Some(tier), clients, &[]).can_access_client(asked),
            "tier={tier} clients={clients:?} asked={asked}"
        );
    }
    // Java's own anchor row: no clients listed, still granted by the tier.
    assert!(principal(Some("ANCHOR"), &[], &[], false, &[]).can_access_client("some-client"));
}

/// Java's `canAccessApplicationAgreesWithAuthContext` rows. The Rust
/// `AuthContext` has no application claims, so these are pinned against
/// Java's answers directly.
#[test]
fn can_access_application_matches_javas_rows() {
    let rows: [(bool, &[&str], &str, bool); 5] = [
        (true, &[], "app_1", true),
        (false, &["app_1"], "app_1", true),
        (false, &["app_1"], "app_2", false),
        (false, &["app_1", "app_2"], "app_2", true),
        (false, &[], "app_1", false),
    ];
    for (all, applications, asked, expected) in rows {
        assert_eq!(
            principal(Some("CLIENT"), &[], applications, all, &[]).can_access_application(asked),
            expected,
            "all={all} applications={applications:?} asked={asked}"
        );
    }
}
