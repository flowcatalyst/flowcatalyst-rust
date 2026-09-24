//! The function permissions and roles match Java's catalogue
//! (`../flowcatalyst-javalin` at `0118cdca`): `shared/auth/Permission.java:271-293`,
//! `seed/Permissions.java:230-238` and `seed/PlatformRoles.java`. The expected
//! strings are Java's, written out here. Where the Java checkout sits next to
//! this repo, the permission strings are also read from its source.

use std::collections::BTreeSet;
use std::path::Path;

use fc_platform::role::entity::{permissions, roles, AuthRole};

/// Java's nine `platform:function:*` permissions, in declaration order.
const JAVA_FUNCTION_PERMISSIONS: [&str; 9] = [
    "platform:function:function:view",
    "platform:function:function:manage",
    "platform:function:version:publish",
    "platform:function:alias:promote",
    "platform:function:policy:manage",
    "platform:function:host:control",
    "platform:function:version:invoke",
    "platform:function:secret:manage",
    "platform:function:domain:manage",
];

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn perms(role: &AuthRole) -> BTreeSet<String> {
    role.permissions.iter().cloned().collect()
}

fn role(name: &str) -> AuthRole {
    roles::all()
        .into_iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no built-in role {name}"))
}

#[test]
fn function_permissions_are_javas_nine_in_order() {
    assert_eq!(permissions::function::ALL, JAVA_FUNCTION_PERMISSIONS);
}

// PlatformRoles.java:211-217.
#[test]
fn function_publisher_holds_exactly_javas_grants() {
    let publisher = role("platform:function-publisher");
    assert_eq!(publisher.display_name, "Function Publisher");
    assert_eq!(
        publisher.description.as_deref(),
        Some("Publishes and promotes function versions")
    );
    assert_eq!(
        perms(&publisher),
        set(&[
            "platform:function:function:view",
            "platform:function:version:publish",
            "platform:function:alias:promote",
            "platform:function:version:invoke",
            "platform:function:secret:manage",
        ])
    );
}

// PlatformRoles.java:219-224.
#[test]
fn function_host_holds_only_host_control() {
    let host = role("platform:function-host");
    assert_eq!(host.display_name, "Function Host");
    assert_eq!(
        host.description.as_deref(),
        Some("Fetches desired state and reports heartbeats for the function host")
    );
    assert_eq!(perms(&host), set(&["platform:function:host:control"]));
}

// PlatformRoles.java:149-150: messaging-admin gains every function grant
// except host control.
#[test]
fn messaging_admin_function_grants_are_javas() {
    let admin = role("platform:messaging-admin");
    let function_grants: BTreeSet<String> = perms(&admin)
        .into_iter()
        .filter(|p| p.starts_with("platform:function:"))
        .collect();
    assert_eq!(
        function_grants,
        set(&[
            "platform:function:function:view",
            "platform:function:function:manage",
            "platform:function:version:publish",
            "platform:function:alias:promote",
            "platform:function:policy:manage",
            "platform:function:version:invoke",
            "platform:function:secret:manage",
            "platform:function:domain:manage",
        ])
    );
}

/// No other built-in role holds a function permission (super-admin's
/// wildcard aside), as in Java.
#[test]
fn no_other_role_holds_a_function_permission() {
    for r in roles::all() {
        if matches!(
            r.name.as_str(),
            "platform:function-publisher" | "platform:function-host" | "platform:messaging-admin"
        ) {
            continue;
        }
        let held: Vec<_> = r
            .permissions
            .iter()
            .filter(|p| p.starts_with("platform:function:"))
            .collect();
        assert!(held.is_empty(), "{} holds {held:?}", r.name);
    }
}

/// Reads `seed/Permissions.java`'s `FUNCTION_*` constants from the Java
/// checkout, when it is there, and compares them with Rust's.
#[test]
fn function_permissions_match_the_java_source_when_present() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../../flowcatalyst-javalin/server/src/main/java/io/flowcatalyst/platform/seed/Permissions.java",
    );
    let Ok(source) = std::fs::read_to_string(&path) else {
        eprintln!("skipped: {} not found", path.display());
        return;
    };
    let java: Vec<String> = source
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("public static final String FUNCTION_"))
        .filter_map(|l| l.split('"').nth(1).map(str::to_string))
        .collect();
    let rust: Vec<String> = permissions::function::ALL
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(java, rust);
}
