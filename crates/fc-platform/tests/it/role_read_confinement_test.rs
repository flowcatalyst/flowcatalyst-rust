//! Go's F21 (`bf81422f`, docs/platform-bug-fixes.md in flowcatalyst-go): the
//! nine `/api/roles` read routes (list, by id or name, by code, by source, by
//! application, application filters, a role's permissions, and the two
//! permission-catalogue reads) admit the application-service `role:view` /
//! `permission:view` next to the console's `platform:iam:role:view`, and
//! confine a caller admitted only by the former to the applications it is
//! bound to: a role is the application's when its `application_id` is one the
//! caller is bound to (a legacy row with no id is matched by
//! `application_code`; a row linked to another application's id is never
//! matched by its code); another application's role answers the same 404 as a
//! missing one; a foreign application id on by-application is a 403
//! `APPLICATION_ACCESS_REQUIRED`; a caller bound to no (resolvable)
//! application is a 403 on every route, never an empty page. The console
//! permission is unchanged, an all-applications caller is not narrowed, and
//! the two application-service permissions are not interchangeable. The BFF
//! admits neither. Requires Docker (or `FC_TEST_DATABASE_URL`).

use std::cell::Cell;

use axum::http::StatusCode;
use serde_json::Value;

use crate::support::{self, read_json, TestApp};
use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::{AuthRole, RoleSource};
use fc_platform::service_account::entity::RoleAssignment;

const APP_SVC_ROLE_VIEW: &str = "platform:application-service:role:view";
const APP_SVC_PERM_VIEW: &str = "platform:application-service:permission:view";
const IAM_ROLE_VIEW: &str = "platform:iam:role:view";

struct Fixture {
    app: TestApp,
    own: Application,
    other: Application,
    own_perm: String,
    other_perm: String,
    seq: Cell<u32>,
}

impl Fixture {
    async fn new() -> Self {
        support::set_app_key();
        let app = TestApp::setup().await;
        let own = Application::new("f21own", "F21 own");
        let other = Application::new("f21oth", "F21 other");
        for a in [&own, &other] {
            app.repos
                .application_repo
                .insert(a)
                .await
                .expect("insert application");
        }
        let own_perm = "f21own:ctx:agg:act".to_string();
        let other_perm = "f21oth:ctx:agg:act".to_string();
        let add = |application: &Application, id: Option<&Application>, short: &str, perm: &str| {
            let mut r = AuthRole::new(&application.code, short, short).with_permission(perm);
            r.source = RoleSource::Sdk;
            r.application_id = id.map(|a| a.id.clone());
            r.description = Some(format!("secret-description-{short}"));
            r
        };
        for role in [
            add(&own, Some(&own), "clerk", &own_perm),
            add(&other, Some(&other), "clerk", &other_perm),
            // Legacy rows: no application id, matched by code.
            add(&own, None, "legacy", &own_perm),
            add(&other, None, "legacy", &other_perm),
            // Linked to the other application's id but carrying the own code:
            // the id wins.
            add(&own, Some(&other), "mislinked", &other_perm),
        ] {
            app.repos
                .role_repo
                .insert(&role)
                .await
                .expect("insert role");
        }
        for (code, a) in [
            (&own_perm, "f21own"),
            (&other_perm, "f21oth"),
            (&"platform:ctx:agg:act".to_string(), "platform"),
        ] {
            sqlx::query(
                "INSERT INTO iam_permissions (id, code, subdomain, context, aggregate, action) \
                 VALUES ($1, $2, $3, 'ctx', 'agg', 'act') ON CONFLICT (code) DO NOTHING",
            )
            .bind(format!("prm_f21{a}"))
            .bind(code)
            .bind(a)
            .execute(&app.pool)
            .await
            .expect("insert permission");
        }
        Self {
            app,
            own,
            other,
            own_perm,
            other_perm,
            seq: Cell::new(0),
        }
    }

    /// A service account holding `perms`, bound to the applications whose ids
    /// are `bound` (raw, so a dangling id can be bound), `all_applications`
    /// as given.
    async fn caller(&self, perms: &[&str], bound: &[&str], all_applications: bool) -> String {
        let n = self.seq.get();
        self.seq.set(n + 1);
        let name = format!("caller{n}");
        let mut role = AuthRole::new("f21callers", &name, &name);
        for p in perms {
            role = role.with_permission(*p);
        }
        self.app.repos.role_repo.insert(&role).await.expect("role");
        let mut p = Principal::new_service(&name, &name, UserScope::Anchor);
        p.roles = vec![RoleAssignment::new(role.name.clone())];
        self.app
            .repos
            .principal_repo
            .insert(&p)
            .await
            .expect("principal");
        sqlx::query("UPDATE iam_principals SET all_applications = $2 WHERE id = $1")
            .bind(p.id.as_str())
            .bind(all_applications)
            .execute(&self.app.pool)
            .await
            .expect("flag");
        for a in bound {
            sqlx::query(
                "INSERT INTO iam_principal_application_access (principal_id, application_id) VALUES ($1, $2)",
            )
            .bind(p.id.as_str())
            .bind(a)
            .execute(&self.app.pool)
            .await
            .expect("grant");
        }
        self.app
            .auth_service
            .generate_access_token(&p)
            .expect("token")
    }

    async fn svc(&self, perms: &[&str]) -> String {
        self.caller(perms, &[self.own.id.as_str()], false).await
    }

    async fn get(&self, token: &str, path: &str) -> (StatusCode, Value) {
        read_json(self.app.get(path, token).await).await
    }
}

fn names(v: &Value) -> Vec<String> {
    let arr = v
        .as_array()
        .or_else(|| v["roles"].as_array())
        .expect("roles");
    let mut out: Vec<String> = arr
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_string())
        .collect();
    out.sort();
    out
}

fn sorted(v: &[&str]) -> Vec<String> {
    let mut o: Vec<String> = v.iter().map(|s| s.to_string()).collect();
    o.sort();
    o
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_bound_application_caller_reads_only_its_applications_roles() {
    let f = Fixture::new().await;
    let svc = f.svc(&[APP_SVC_ROLE_VIEW]).await;
    let own_roles = sorted(&["f21own:clerk", "f21own:legacy"]);

    // list
    let (status, body) = f.get(&svc, "/api/roles").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        names(&body),
        own_roles,
        "the bound application's roles (id-linked and legacy), nothing else"
    );
    assert_eq!(body["total"], 2, "{body}");

    // by id and by name: own ok, another's is the same 404 as a missing role
    let own_id: String = sqlx::query_scalar("SELECT id FROM iam_roles WHERE name = 'f21own:clerk'")
        .fetch_one(&f.app.pool)
        .await
        .unwrap();
    for key in [own_id.as_str(), "f21own:clerk", "f21own:legacy"] {
        let (status, body) = f.get(&svc, &format!("/api/roles/{key}")).await;
        assert_eq!(status, StatusCode::OK, "{key}: {body}");
    }
    let (_, missing) = f.get(&svc, "/api/roles/f21own:zz-missing").await;
    for key in ["f21oth:clerk", "f21oth:legacy", "f21own:mislinked"] {
        let (status, body) = f.get(&svc, &format!("/api/roles/{key}")).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{key}: {body}");
        assert_eq!(
            body["error"], missing["error"],
            "{key}: indistinguishable from a missing role"
        );
    }
    let other_id: String =
        sqlx::query_scalar("SELECT id FROM iam_roles WHERE name = 'f21oth:clerk'")
            .fetch_one(&f.app.pool)
            .await
            .unwrap();
    let (status, _) = f.get(&svc, &format!("/api/roles/{other_id}")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "another application's role by id"
    );

    // by code
    let (status, body) = f.get(&svc, "/api/roles/by-code/f21own:clerk").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for c in ["f21oth:clerk", "f21oth:legacy"] {
        let (status, _) = f.get(&svc, &format!("/api/roles/by-code/{c}")).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{c}");
    }

    // by source
    let (status, body) = f.get(&svc, "/api/roles/by-source/SDK").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(names(&body), own_roles);

    // by application: own ok (by application_id, so not the legacy row),
    // foreign and unknown ids are one 403
    let (status, body) = f
        .get(&svc, &format!("/api/roles/by-application/{}", f.own.id))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(names(&body), sorted(&["f21own:clerk"]));
    for id in [f.other.id.to_string(), "app_doesnotexist".to_string()] {
        let (status, body) = f
            .get(&svc, &format!("/api/roles/by-application/{id}"))
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{id}: {body}");
        assert_eq!(body["error"], "APPLICATION_ACCESS_REQUIRED", "{id}: {body}");
    }

    // filters/applications
    let (status, body) = f.get(&svc, "/api/roles/filters/applications").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["applicationCodes"], serde_json::json!(["f21own"]));

    // a role's permissions
    for r in ["f21own:clerk", "f21own:legacy"] {
        let (status, body) = f.get(&svc, &format!("/api/roles/{r}/permissions")).await;
        assert_eq!(status, StatusCode::OK, "{r}: {body}");
        assert_eq!(body["permissions"], serde_json::json!([f.own_perm]));
    }
    for r in ["f21oth:clerk", "f21oth:legacy"] {
        let (status, _) = f.get(&svc, &format!("/api/roles/{r}/permissions")).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{r}");
    }

    // the wire shape is unchanged
    let (_, body) = f.get(&svc, &format!("/api/roles/{own_id}")).await;
    let mut keys: Vec<&str> = body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "applicationCode",
            "applicationId",
            "clientManaged",
            "createdAt",
            "description",
            "displayName",
            "id",
            "name",
            "permissions",
            "source",
            "updatedAt"
        ]
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_permission_catalogue_is_limited_to_the_bound_applications() {
    let f = Fixture::new().await;
    let svc_perm = f.svc(&[APP_SVC_PERM_VIEW]).await;
    let svc_role = f.svc(&[APP_SVC_ROLE_VIEW]).await;

    let (status, body) = f.get(&svc_perm, "/api/roles/permissions").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let got: Vec<&str> = body["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["permission"].as_str().unwrap())
        .collect();
    assert_eq!(got, [f.own_perm.as_str()], "{body}");
    assert_eq!(body["total"], 1);

    let (status, body) = f
        .get(&svc_perm, &format!("/api/roles/permissions/{}", f.own_perm))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for p in [f.other_perm.as_str(), "platform:ctx:agg:act"] {
        let (status, _) = f
            .get(&svc_perm, &format!("/api/roles/permissions/{p}"))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{p}");
    }

    // role:view alone does not admit the catalogue, nor permission:view the roles.
    let (status, body) = f.get(&svc_role, "/api/roles/permissions").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body) = f.get(&svc_perm, "/api/roles").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_unbound_credential_is_refused_on_every_read_not_answered_empty() {
    let f = Fixture::new().await;
    let both = [APP_SVC_ROLE_VIEW, APP_SVC_PERM_VIEW];
    let unbound = f.caller(&both, &[], false).await;
    let dangling = f.caller(&both, &["app_doesnotexist"], false).await;
    let own_perm_path = format!("/api/roles/permissions/{}", f.own_perm);
    let by_app = format!("/api/roles/by-application/{}", f.own.id);
    for (label, token) in [("unbound", &unbound), ("dangling", &dangling)] {
        for path in [
            "/api/roles",
            "/api/roles/f21own:clerk",
            "/api/roles/by-code/f21own:clerk",
            "/api/roles/by-source/SDK",
            by_app.as_str(),
            "/api/roles/filters/applications",
            "/api/roles/f21own:clerk/permissions",
            "/api/roles/permissions",
            own_perm_path.as_str(),
        ] {
            let (status, body) = f.get(token, path).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{label} {path}: {body}");
        }
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn binding_decides_the_applications_a_caller_reads() {
    let f = Fixture::new().await;
    let (own, other) = (f.own.id.to_string(), f.other.id.to_string());

    // A dangling id beside a real one still confines to the real one.
    let t = f
        .caller(
            &[APP_SVC_ROLE_VIEW],
            &["app_doesnotexist", own.as_str()],
            false,
        )
        .await;
    let (_, body) = f.get(&t, "/api/roles").await;
    assert_eq!(names(&body), sorted(&["f21own:clerk", "f21own:legacy"]));

    // Bound to the other application: that one only (the mislinked row by id).
    let t = f
        .caller(&[APP_SVC_ROLE_VIEW], &[other.as_str()], false)
        .await;
    let (_, body) = f.get(&t, "/api/roles").await;
    assert_eq!(
        names(&body),
        sorted(&["f21oth:clerk", "f21oth:legacy", "f21own:mislinked"])
    );

    // Bound to both: both.
    let t = f
        .caller(&[APP_SVC_ROLE_VIEW], &[own.as_str(), other.as_str()], false)
        .await;
    let (_, body) = f.get(&t, "/api/roles").await;
    assert_eq!(
        names(&body),
        sorted(&[
            "f21own:clerk",
            "f21own:legacy",
            "f21oth:clerk",
            "f21oth:legacy",
            "f21own:mislinked"
        ])
    );

    // All-applications, holding only the application-service permission: not narrowed.
    let t = f.caller(&[APP_SVC_ROLE_VIEW], &[], true).await;
    let (status, body) = f.get(&t, "/api/roles").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let got = names(&body);
    for r in [
        "f21own:clerk",
        "f21own:legacy",
        "f21oth:clerk",
        "f21oth:legacy",
    ] {
        assert!(got.iter().any(|g| g == r), "{r} in {got:?}");
    }
    let (status, _) = f.get(&t, "/api/roles/f21oth:clerk").await;
    assert_eq!(status, StatusCode::OK);

    // Console permission: unchanged, sees everything even when application-bound.
    let t = f.caller(&[IAM_ROLE_VIEW], &[own.as_str()], false).await;
    let (_, body) = f.get(&t, "/api/roles").await;
    let got = names(&body);
    for r in [
        "f21own:clerk",
        "f21oth:clerk",
        "f21oth:legacy",
        "f21own:mislinked",
    ] {
        assert!(got.iter().any(|g| g == r), "{r} in {got:?}");
    }
    let (status, body) = f.get(&t, "/api/roles/permissions").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["total"].as_u64().unwrap() >= 3, "{body}");
    let (status, body) = f.get(&t, "/api/roles/filters/applications").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let codes = body["applicationCodes"].as_array().unwrap();
    assert!(codes.iter().any(|c| c == "f21own") && codes.iter().any(|c| c == "f21oth"));

    // No permission, no read.
    let t = f.svc(&["platform:application-service:event:create"]).await;
    let (status, _) = f.get(&t, "/api/roles").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_bff_roles_admit_the_console_permission_only() {
    let f = Fixture::new().await;
    let svc = f.svc(&[APP_SVC_ROLE_VIEW, APP_SVC_PERM_VIEW]).await;
    for path in [
        "/bff/roles",
        "/bff/roles/filters/applications",
        "/bff/roles/permissions",
    ] {
        let (status, body) = f.get(&svc, path).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
    }
    let console = f.caller(&[IAM_ROLE_VIEW], &[], true).await;
    let (status, body) = f.get(&console, "/bff/roles").await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
