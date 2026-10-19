//! Go F20: the application's users and invitations, the six routes under
//! `/api/applications/{appCode}/users` and `/invitations` that an
//! application's own admin pages call as its service account (Go
//! `appusers/api_pg_test.go`, ported through the HTTP stack).
//!
//! An application's users are the CLIENT-scope users homed at a client the
//! application is enabled for who hold at least one of its roles; everything
//! is confined to the applications the credential is bound to. Where Go
//! answers an out-of-scope `{appCode}` with 403 "Not authorised for
//! application", this platform keeps its owner ruling (the same 404 as a
//! missing application, `require_application_access`). Requires Docker (or
//! `FC_TEST_PG_BIN`).

use std::collections::{BTreeSet, HashMap};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::support::{self, read_json, TestApp};
use fc_platform::application::entity::Application;
use fc_platform::application::ApplicationClientConfig;
use fc_platform::client::entity::Client;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::email_domain_mapping::entity::ScopeType;
use fc_platform::identity_provider::entity::{IdentityProvider, IdentityProviderType};
use fc_platform::role::entity::AuthRole;
use fc_platform::service_account::entity::RoleAssignment;
use fc_platform::shared::id::{PasswordResetTokenId, PrincipalId};
use fc_platform::EmailDomainMapping;
use fc_platform::PlatformError;

const PERM_USER_VIEW: &str = "platform:application-service:user:view";
const PERM_USER_CREATE: &str = "platform:application-service:user:create";
const PERM_INV_VIEW: &str = "platform:application-service:invitation:view";
const PERM_INV_MANAGE: &str = "platform:application-service:invitation:manage";
const ALL_PERMS: [&str; 4] = [
    PERM_USER_VIEW,
    PERM_USER_CREATE,
    PERM_INV_VIEW,
    PERM_INV_MANAGE,
];
const HASH: &str = "$argon2id$v=19$m=65536,t=3,p=4$SECRETSALT$SECRETHASHVALUE";

struct Fixture {
    app: TestApp,
    own: Application,
    other: Application,
    /// c1 and c2 are enabled for `own`; c3 only for `other`; c4's config for
    /// `own` is disabled.
    c1: Client,
    c2: Client,
    c3: Client,
    c4: Client,
    /// key -> principal id
    u: HashMap<&'static str, String>,
    email_of: HashMap<&'static str, String>,
    platform_role: String,
}

async fn put_client(app: &TestApp, tag: &str, config_for: Option<(&Application, bool)>) -> Client {
    let client = Client::new(format!("Client {tag}"), format!("uac-{tag}"));
    app.repos.client_repo.insert(&client).await.unwrap();
    if let Some((application, enabled)) = config_for {
        let mut cfg = ApplicationClientConfig::new(application.id.clone(), client.id.clone());
        cfg.enabled = enabled;
        app.repos
            .application_client_config_repo
            .insert(&cfg)
            .await
            .unwrap();
    }
    client
}

async fn put_role(app: &TestApp, application: &str, name: &str) -> String {
    let role =
        AuthRole::new(application, name, name).with_permission(format!("{application}:thing:view"));
    app.repos.role_repo.insert(&role).await.unwrap();
    role.name
}

#[expect(clippy::too_many_arguments, reason = "a fixture row, spelled out")]
async fn put_user(
    app: &TestApp,
    client: Option<&Client>,
    scope: &str,
    roles: &[&str],
    active: bool,
    hash: Option<&str>,
    idp: &str,
    email: &str,
    name: &str,
) -> String {
    let id = PrincipalId::generate();
    sqlx::query(
        "INSERT INTO iam_principals (id, type, scope, client_id, name, active, email, email_domain, idp_type, password_hash) \
         VALUES ($1, 'USER', $2, $3, $4, $5, $6, 'example.test', $7, $8)",
    )
    .bind(id.as_str())
    .bind(scope)
    .bind(client.map(|c| c.id.as_str().to_string()))
    .bind(name)
    .bind(active)
    .bind(email)
    .bind(idp)
    .bind(hash)
    .execute(&app.pool)
    .await
    .unwrap();
    for r in roles {
        sqlx::query(
            "INSERT INTO iam_principal_roles (principal_id, role_name, assignment_source) \
             VALUES ($1, $2, 'ADMIN_ASSIGNED')",
        )
        .bind(id.as_str())
        .bind(*r)
        .execute(&app.pool)
        .await
        .unwrap();
    }
    id.as_str().to_string()
}

async fn put_token(user: &str, hours: i64, purpose: &str, app: &TestApp, hash: &str) {
    sqlx::query(
        "INSERT INTO iam_password_reset_tokens (id, principal_id, token_hash, purpose, expires_at, created_at) \
         VALUES ($1, $2, $3, $4, NOW() + ($5 || ' hours')::interval, NOW())",
    )
    .bind(PasswordResetTokenId::generate().as_str())
    .bind(user)
    .bind(hash)
    .bind(purpose)
    .bind(hours.to_string())
    .execute(&app.pool)
    .await
    .unwrap();
}

async fn setup() -> Fixture {
    support::set_app_key();
    let app = TestApp::setup().await;
    let own = Application::new("uaown", "UA Own");
    let other = Application::new("uaoth", "UA Other");
    for a in [&own, &other] {
        app.repos.application_repo.insert(a).await.unwrap();
    }
    let clerk = put_role(&app, "uaown", "clerk").await;
    let admin_role = put_role(&app, "uaown", "admin").await;
    let other_clerk = put_role(&app, "uaoth", "clerk").await;
    let platform_role = put_role(&app, "platform", "uatest").await;

    let c1 = put_client(&app, "c1", Some((&own, true))).await;
    let c2 = put_client(&app, "c2", Some((&own, true))).await;
    let c3 = put_client(&app, "c3", Some((&other, true))).await;
    let c4 = put_client(&app, "c4", Some((&own, false))).await;

    let mut u = HashMap::new();
    let mut email_of = HashMap::new();
    macro_rules! user {
        ($key:expr, $client:expr, $scope:expr, $roles:expr, $active:expr, $hash:expr, $idp:expr) => {{
            let email = format!("{}@example.test", $key);
            let id = put_user(
                &app,
                $client,
                $scope,
                $roles,
                $active,
                $hash,
                $idp,
                &email,
                &format!("User {}", $key),
            )
            .await;
            u.insert($key, id);
            email_of.insert($key, email);
        }};
    }
    // The application's users:
    // invited, never set a password; also holds the other app's role
    user!(
        "pend",
        Some(&c1),
        "CLIENT",
        &[&clerk, &other_clerk],
        true,
        None,
        "INTERNAL"
    );
    user!(
        "done",
        Some(&c1),
        "CLIENT",
        &[&admin_role],
        true,
        Some(HASH),
        "INTERNAL"
    );
    user!(
        "off",
        Some(&c2),
        "CLIENT",
        &[&clerk],
        false,
        None,
        "INTERNAL"
    );
    user!("fed", Some(&c2), "CLIENT", &[&clerk], true, None, "OIDC");
    user!(
        "c2u",
        Some(&c2),
        "CLIENT",
        &[&clerk],
        true,
        Some(HASH),
        "INTERNAL"
    );
    // Not the application's:
    user!(
        "c3u",
        Some(&c3),
        "CLIENT",
        &[&clerk],
        true,
        Some(HASH),
        "INTERNAL"
    );
    user!(
        "c4u",
        Some(&c4),
        "CLIENT",
        &[&clerk],
        true,
        Some(HASH),
        "INTERNAL"
    );
    user!(
        "noRole",
        Some(&c1),
        "CLIENT",
        &[&other_clerk],
        true,
        Some(HASH),
        "INTERNAL"
    );
    user!(
        "anchor",
        None,
        "ANCHOR",
        &[&clerk],
        true,
        Some(HASH),
        "INTERNAL"
    );
    user!(
        "partner",
        Some(&c1),
        "PARTNER",
        &[&clerk],
        true,
        Some(HASH),
        "INTERNAL"
    );

    // "pend" has a live invite token; "c2u" an ordinary reset token, which
    // must not matter.
    put_token(&u["pend"], 24, "invite", &app, "hpend").await;
    put_token(&u["c2u"], 1, "reset", &app, "hreset").await;

    Fixture {
        app,
        own,
        other,
        c1,
        c2,
        c3,
        c4,
        u,
        email_of,
        platform_role,
    }
}

/// One route under test: its name and the request it makes for an application code.
type Call = (
    &'static str,
    Box<dyn Fn(&str) -> (Method, String, Option<Value>)>,
);

/// How a test caller reaches applications.
enum Reach<'a> {
    Bound(&'a [&'a Application]),
    All,
}

impl Fixture {
    /// A token for a stored service principal holding exactly `perms`, with
    /// the given application reach; `client` makes it a client-scoped caller.
    async fn token(
        &self,
        name: &str,
        reach: Reach<'_>,
        perms: &[&str],
        client: Option<&Client>,
    ) -> String {
        let role_name = {
            let mut role = AuthRole::new("uacaller", name, name);
            for p in perms {
                role = role.with_permission(*p);
            }
            self.app.repos.role_repo.insert(&role).await.unwrap();
            role.name.clone()
        };
        let scope = if client.is_some() {
            UserScope::Client
        } else {
            UserScope::Anchor
        };
        let mut principal = Principal::new_service(name, name, scope);
        if let Some(c) = client {
            principal = principal.with_client_id(c.id.clone());
        }
        principal.roles = vec![RoleAssignment::new(role_name)];
        self.app
            .repos
            .principal_repo
            .insert(&principal)
            .await
            .unwrap();
        match reach {
            Reach::All => {
                principal.all_applications = true;
            }
            Reach::Bound(apps) => {
                principal.accessible_application_ids = apps.iter().map(|a| a.id.clone()).collect();
            }
        }
        self.app
            .repos
            .principal_repo
            .update(&principal)
            .await
            .unwrap();
        self.app
            .auth_service
            .generate_access_token(&principal)
            .unwrap()
    }

    async fn svc(&self, name: &str, perms: &[&str]) -> String {
        self.token(name, Reach::Bound(&[&self.own]), perms, None)
            .await
    }

    fn base(&self) -> String {
        format!("/api/applications/{}", self.own.code)
    }

    fn ids_of(&self, keys: &[&str]) -> BTreeSet<String> {
        keys.iter().map(|k| self.u[k].clone()).collect()
    }

    async fn count_users(&self, email: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM iam_principals WHERE email = $1")
            .bind(email)
            .fetch_one(&self.app.pool)
            .await
            .unwrap()
    }

    async fn tokens_of(&self, user: &str) -> Vec<(String, String)> {
        sqlx::query_as(
            "SELECT token_hash, purpose FROM iam_password_reset_tokens WHERE principal_id = $1",
        )
        .bind(user)
        .fetch_all(&self.app.pool)
        .await
        .unwrap()
    }

    async fn active(&self, user: &str) -> bool {
        sqlx::query_scalar("SELECT active FROM iam_principals WHERE id = $1")
            .bind(user)
            .fetch_one(&self.app.pool)
            .await
            .unwrap()
    }
}

fn visible() -> [&'static str; 5] {
    ["pend", "done", "off", "fed", "c2u"]
}

fn user_ids(body: &Value) -> BTreeSet<String> {
    body["users"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["id"].as_str().unwrap().to_string())
        .collect()
}

fn invitation_ids(body: &Value) -> BTreeSet<String> {
    body["invitations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["userId"].as_str().unwrap().to_string())
        .collect()
}

fn code(body: &Value) -> &str {
    body["error"].as_str().unwrap_or_default()
}

async fn get(f: &Fixture, token: &str, path: &str) -> (StatusCode, Value) {
    read_json(f.app.get(path, token).await).await
}

async fn post(f: &Fixture, token: &str, path: &str, body: Value) -> (StatusCode, Value) {
    read_json(f.app.post(path, token, body).await).await
}

// The users visible to an application: CLIENT users at its enabled clients
// holding its roles; nobody else; and only its own roles on them.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_list_is_the_applications_users_and_nobody_else() {
    let f = setup().await;
    let svc = f.svc("svc-list", &[PERM_USER_VIEW]).await;
    let list = |query: String| {
        let (f, svc) = (&f, svc.clone());
        async move { get(f, &svc, &format!("{}/users?{query}", f.base())).await }
    };

    let (status, body) = list("pageSize=200".into()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(user_ids(&body), f.ids_of(&visible()), "{body}");
    assert_eq!(body["total"], visible().len());
    for u in body["users"].as_array().unwrap() {
        for r in u["roles"].as_array().unwrap() {
            assert!(
                r.as_str().unwrap().starts_with("uaown:"),
                "only the application's own roles are shown, got {r}"
            );
        }
        if u["id"] == f.u["pend"] {
            assert_eq!(
                u["roles"],
                json!(["uaown:clerk"]),
                "the other application's role is not shown"
            );
            assert_eq!(u["invitation"], "PENDING");
        }
        if u["id"] == f.u["done"] {
            assert!(u.get("invitation").is_none(), "{u}");
        }
    }

    // filters
    let ids = |q: String| {
        let list = &list;
        async move { user_ids(&list(q).await.1) }
    };
    assert_eq!(
        ids(format!("pageSize=200&clientId={}", f.c1.id)).await,
        f.ids_of(&["done", "pend"])
    );
    assert_eq!(
        ids("pageSize=200&active=false".into()).await,
        f.ids_of(&["off"])
    );
    assert_eq!(
        ids("pageSize=200&roles=uaown:admin".into()).await,
        f.ids_of(&["done"])
    );
    assert_eq!(
        ids("pageSize=200&roles=admin".into()).await,
        f.ids_of(&["done"]),
        "short role names are the application's"
    );
    assert!(
        ids("pageSize=200&roles=uaoth:clerk".into())
            .await
            .is_empty(),
        "another application's role matches nothing"
    );
    assert_eq!(ids("pageSize=200&q=PEND".into()).await, f.ids_of(&["pend"]));
    assert!(
        ids("pageSize=200&q=%25".into()).await.is_empty(),
        "LIKE metacharacters are literal"
    );
    assert!(
        ids(format!("pageSize=200&clientId={}", f.c3.id))
            .await
            .is_empty(),
        "client c3's user cannot be reached by naming the client"
    );

    // pagination tiles the set without overlap
    let page = |p: i64, size: i64| {
        let list = &list;
        async move {
            list(format!("page={p}&pageSize={size}&sortField=email"))
                .await
                .1
        }
    };
    let (first, second, third) = (page(0, 2).await, page(1, 2).await, page(2, 2).await);
    assert_eq!(first["users"].as_array().unwrap().len(), 2);
    assert_eq!(first["total"], 5);
    assert_eq!(second["users"].as_array().unwrap().len(), 2);
    assert_eq!(third["users"].as_array().unwrap().len(), 1);
    let mut all = user_ids(&first);
    all.extend(user_ids(&second));
    all.extend(user_ids(&third));
    assert_eq!(all, f.ids_of(&visible()));
    assert_eq!(
        page(0, 100_000).await["pageSize"],
        200,
        "the page size is clamped"
    );
    assert_eq!(page(0, 0).await["pageSize"], 50, "and defaults");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_user_outside_the_application_is_not_found_like_a_missing_id() {
    let f = setup().await;
    let svc = f.svc("svc-get", &[PERM_USER_VIEW]).await;
    let (status, body) = get(&f, &svc, &format!("{}/users/{}", f.base(), f.u["pend"])).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["email"], f.email_of["pend"]);

    let (missing_status, missing) =
        get(&f, &svc, &format!("{}/users/prn_doesnotexist", f.base())).await;
    assert_eq!(missing_status, StatusCode::NOT_FOUND, "{missing}");
    for hidden in ["c3u", "c4u", "noRole", "anchor", "partner"] {
        let (status, body) = get(&f, &svc, &format!("{}/users/{}", f.base(), f.u[hidden])).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{hidden}: {body}");
        assert_eq!(code(&body), code(&missing), "{hidden}");
        assert_eq!(
            body["message"].as_str().unwrap().replace(&f.u[hidden], "X"),
            missing["message"]
                .as_str()
                .unwrap()
                .replace("prn_doesnotexist", "X"),
            "{hidden}: not distinguishable from a missing id"
        );
    }
}

// The application binding: another application's code, an unbound credential
// and (for any application-scoped credential) a code that names no
// application are one and the same refusal; nothing is changed by any of
// them. A credential reaching every application gets the 404 for an unknown
// code. (Go answers 403 here; this platform's owner ruling is the 404.)
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_application_binding_is_enforced_on_all_six_routes() {
    let f = setup().await;
    let other_svc = f
        .token("svc-other", Reach::Bound(&[&f.other]), &ALL_PERMS, None)
        .await;
    let unbound = f
        .token("svc-unbound", Reach::Bound(&[]), &ALL_PERMS, None)
        .await;
    let admin = f.token("svc-admin", Reach::All, &ALL_PERMS, None).await;
    let invite_body = json!({
        "email": "x@example.test", "name": "X", "clientId": f.c1.id.as_str(), "roles": ["uaown:clerk"]
    });
    let pend = f.u["pend"].clone();

    let calls: Vec<Call> = vec![
        (
            "list",
            Box::new(|a| (Method::GET, format!("/api/applications/{a}/users"), None)),
        ),
        (
            "get",
            Box::new({
                let pend = pend.clone();
                move |a| {
                    (
                        Method::GET,
                        format!("/api/applications/{a}/users/{pend}"),
                        None,
                    )
                }
            }),
        ),
        (
            "invite",
            Box::new({
                let body = invite_body.clone();
                move |a| {
                    (
                        Method::POST,
                        format!("/api/applications/{a}/users"),
                        Some(body.clone()),
                    )
                }
            }),
        ),
        (
            "invitations",
            Box::new(|a| {
                (
                    Method::GET,
                    format!("/api/applications/{a}/invitations"),
                    None,
                )
            }),
        ),
        (
            "resend",
            Box::new({
                let pend = pend.clone();
                move |a| {
                    (
                        Method::POST,
                        format!("/api/applications/{a}/invitations/{pend}/resend"),
                        Some(json!({})),
                    )
                }
            }),
        ),
        (
            "revoke",
            Box::new({
                let pend = pend.clone();
                move |a| {
                    (
                        Method::DELETE,
                        format!("/api/applications/{a}/invitations/{pend}"),
                        None,
                    )
                }
            }),
        ),
    ];
    for (name, call) in &calls {
        let run = |token: &str, app: &str| {
            let (method, path, body) = call(app);
            let (f, token) = (&f, token.to_string());
            async move {
                match method {
                    Method::GET => get(f, &token, &path).await,
                    Method::POST => post(f, &token, &path, body.unwrap()).await,
                    _ => read_json(f.app.delete(&path, &token).await).await,
                }
            }
        };
        let (own_s, own_b) = run(&other_svc, "uaown").await;
        let (unb_s, unb_b) = run(&unbound, "uaown").await;
        let (ghost_s, ghost_b) = run(&other_svc, "nosuchapp").await;
        let (admin_s, admin_b) = run(&admin, "nosuchapp").await;
        for (what, status, body) in [
            ("other-bound", own_s, &own_b),
            ("unbound", unb_s, &unb_b),
            ("unknown for a scoped caller", ghost_s, &ghost_b),
            ("unknown for an all-applications caller", admin_s, &admin_b),
        ] {
            assert_eq!(status, StatusCode::NOT_FOUND, "{name} {what}: {body}");
        }
    }
    // nothing was changed by any of the refused calls
    assert!(f.active(&f.u["pend"]).await);
    assert_eq!(f.count_users("x@example.test").await, 0);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_all_applications_admin_sees_any_application() {
    let f = setup().await;
    let admin = f
        .token("svc-all", Reach::All, &["platform:iam:user:view"], None)
        .await;
    let (status, body) = get(&f, &admin, &format!("{}/users?pageSize=200", f.base())).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(user_ids(&body), f.ids_of(&visible()));
    let (status, body) = get(
        &f,
        &admin,
        &format!("/api/applications/{}/users?pageSize=200", f.other.code),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        user_ids(&body).is_empty(),
        "the other application has no users of its own at its enabled clients: {body}"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_client_scoped_caller_keeps_its_client_reach() {
    let f = setup().await;
    let scoped = f
        .token("svc-cs", Reach::Bound(&[&f.own]), &ALL_PERMS, Some(&f.c1))
        .await;
    let (status, body) = get(&f, &scoped, &format!("{}/users?pageSize=200", f.base())).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        user_ids(&body),
        f.ids_of(&["pend", "done"]),
        "only client c1's users"
    );

    let (status, _) = get(&f, &scoped, &format!("{}/users/{}", f.base(), f.u["c2u"])).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // inviting into a client outside its reach, or one the application is not
    // enabled for, is one and the same refusal
    for c in [
        f.c2.id.as_str(),
        f.c3.id.as_str(),
        f.c4.id.as_str(),
        "clt_nosuchclient",
        "nosuchidentifier",
    ] {
        let (status, body) = post(
            &f,
            &scoped,
            &format!("{}/users", f.base()),
            json!({
                "email": format!("scoped-{c}@example.test"), "name": "S",
                "clientId": c, "roles": ["uaown:clerk"]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{c}: {body}");
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("client is not available to this application"),
            "{c}: {body}"
        );
    }
    // resend / revoke of a user at another client: not found
    let (status, body) = post(
        &f,
        &scoped,
        &format!("{}/invitations/{}/resend", f.base(), f.u["off"]),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn each_route_asks_its_own_permission() {
    let f = setup().await;
    let deny = |status: StatusCode, body: &Value, what: &str| {
        assert_eq!(status, StatusCode::FORBIDDEN, "{what}: {body}");
        assert_eq!(code(body), "PERMISSION_REQUIRED", "{what}: {body}");
    };
    let pend = f.u["pend"].clone();
    let invite = json!({"email": "a@example.test", "name": "A", "clientId": f.c1.id.as_str(), "roles": ["uaown:clerk"]});

    // only invitation:view: lists invitations, nothing else
    let view = f.svc("svc-invview", &[PERM_INV_VIEW]).await;
    let (s, b) = get(&f, &view, &format!("{}/invitations", f.base())).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (s, b) = get(&f, &view, &format!("{}/users", f.base())).await;
    deny(s, &b, "users");
    let (s, b) = post(
        &f,
        &view,
        &format!("{}/invitations/{pend}/resend", f.base()),
        json!({}),
    )
    .await;
    deny(s, &b, "resend");
    let (s, b) = read_json(
        f.app
            .delete(&format!("{}/invitations/{pend}", f.base()), &view)
            .await,
    )
    .await;
    deny(s, &b, "revoke");

    // user:view alone: reads users, cannot invite
    let uv = f.svc("svc-userview", &[PERM_USER_VIEW]).await;
    let (s, b) = get(&f, &uv, &format!("{}/users", f.base())).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (s, b) = post(&f, &uv, &format!("{}/users", f.base()), invite.clone()).await;
    deny(s, &b, "invite");
    let (s, b) = get(&f, &uv, &format!("{}/invitations", f.base())).await;
    deny(s, &b, "invitations");

    // invitation:manage alone can manage but not read users
    let mg = f.svc("svc-manage", &[PERM_INV_MANAGE]).await;
    let (s, b) = get(&f, &mg, &format!("{}/users", f.base())).await;
    deny(s, &b, "manage reads users");
    let (s, b) = get(&f, &mg, &format!("{}/invitations", f.base())).await;
    assert_eq!(s, StatusCode::OK, "manage lists invitations too: {b}");

    // nothing at all
    let none = f
        .svc("svc-none", &["platform:application-service:event:view"])
        .await;
    let (s, b) = get(&f, &none, &format!("{}/users", f.base())).await;
    deny(s, &b, "none");

    // the IAM permissions are admitted too, confined the same way
    let iam_view = f.svc("svc-iamview", &["platform:iam:user:view"]).await;
    let (s, b) = get(&f, &iam_view, &format!("{}/users", f.base())).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (s, b) = get(&f, &iam_view, &format!("{}/invitations", f.base())).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (s, b) = get(
        &f,
        &iam_view,
        &format!("/api/applications/{}/users", f.other.code),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::NOT_FOUND,
        "IAM view stays confined to the bound application: {b}"
    );
    let iam_create = f.svc("svc-iamcreate", &["platform:iam:user:create"]).await;
    let (s, b) = post(
        &f,
        &iam_create,
        &format!("{}/invitations/{pend}/resend", f.base()),
        json!({"returnInviteLink": true}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");

    // no credential at all
    let resp = f.app.get_unauth(&format!("{}/users", f.base())).await;
    assert!(
        matches!(
            resp.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ),
        "{}",
        resp.status()
    );
}

// No password hash, token or link ever appears in a read.
#[tokio::test]
#[ignore = "requires Docker"]
async fn reads_carry_no_sensitive_fields() {
    let f = setup().await;
    let svc = f.svc("svc-sens", &ALL_PERMS).await;
    let (_, users) = get(&f, &svc, &format!("{}/users?pageSize=200", f.base())).await;
    let (_, one) = get(&f, &svc, &format!("{}/users/{}", f.base(), f.u["done"])).await;
    let (_, invs) = get(&f, &svc, &format!("{}/invitations", f.base())).await;
    for v in [&users, &one, &invs] {
        let raw = v.to_string().to_lowercase();
        for banned in [
            "password",
            "hash",
            "secret",
            "token",
            "invitelink",
            "argon2",
            "mfa",
            "externalidp",
        ] {
            assert!(!raw.contains(banned), "{banned} in {raw}");
        }
    }
    // and the shape is exactly the documented one
    let keys: BTreeSet<&str> = one
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "id",
            "email",
            "name",
            "active",
            "clientId",
            "idpType",
            "roles",
            "createdAt"
        ])
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn inviting_creates_a_visible_client_user_with_the_applications_role() {
    let f = setup().await;
    let svc = f.svc("svc-invite", &ALL_PERMS).await;
    let users = format!("{}/users", f.base());

    // creates a visible CLIENT user with the application's role and sends the invitation
    let (status, body) = post(
        &f,
        &svc,
        &users,
        json!({"email": "Fresh@Example.test", "name": "New Person", "clientId": f.c1.id.as_str(), "roles": ["clerk"]}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["invitationSent"], true);
    assert!(body.get("inviteLink").is_none(), "{body}");
    let fresh = body["id"].as_str().unwrap().to_string();
    let tokens = f.tokens_of(&fresh).await;
    assert_eq!(tokens.len(), 1, "one invite minted: {tokens:?}");
    assert_eq!(tokens[0].1, "invite");
    let (status, got) = get(&f, &svc, &format!("{users}/{fresh}")).await;
    assert_eq!(status, StatusCode::OK, "{got}");
    assert_eq!(got["email"], "fresh@example.test");
    assert_eq!(got["roles"], json!(["uaown:clerk"]));
    assert_eq!(got["clientId"], f.c1.id.as_str());
    assert_eq!(got["invitation"], "PENDING");
    let (scope, src): (String, String) = sqlx::query_as(
        "SELECT p.scope, r.assignment_source FROM iam_principals p \
         JOIN iam_principal_roles r ON r.principal_id = p.id WHERE p.id = $1",
    )
    .bind(&fresh)
    .fetch_one(&f.app.pool)
    .await
    .unwrap();
    assert_eq!(scope, "CLIENT");
    assert_eq!(src, "ADMIN_ASSIGNED", "an SDK sync sweep must not strip it");
    assert_eq!(
        f.app.event_count_by_type("platform:iam:user:created").await,
        1
    );

    // returnInviteLink returns the link and sends nothing
    let (status, body) = post(
        &f,
        &svc,
        &users,
        json!({"email": "link@example.test", "name": "L", "clientId": f.c2.id.as_str(),
               "roles": ["uaown:admin"], "returnInviteLink": true,
               "inviteRedirectUri": "https://app.example/welcome"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(
        body["inviteLink"]
            .as_str()
            .unwrap()
            .contains("set-password?token="),
        "{body}"
    );
    assert_eq!(body["invitationSent"], false);
    let redirect: Option<String> = sqlx::query_scalar(
        "SELECT redirect_uri FROM iam_password_reset_tokens WHERE principal_id = $1",
    )
    .bind(body["id"].as_str().unwrap())
    .fetch_one(&f.app.pool)
    .await
    .unwrap();
    assert_eq!(
        redirect.as_deref(),
        Some("https://app.example/welcome"),
        "the redirect rides on the token"
    );

    // sendInvitation false delivers nothing
    let (status, body) = post(
        &f,
        &svc,
        &users,
        json!({"email": "quiet@example.test", "name": "Q", "clientId": f.c1.id.as_str(),
               "roles": ["clerk"], "sendInvitation": false}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["invitationSent"], false);
    assert!(body.get("inviteLink").is_none());
    assert!(f.tokens_of(body["id"].as_str().unwrap()).await.is_empty());

    // a federated user is created without a password and never invited
    let mut idp = IdentityProvider::new("uaoidc", "UA SSO", IdentityProviderType::Oidc);
    idp.oidc_issuer_url = Some("https://idp.fed.test".into());
    idp.oidc_client_id = Some("ua-client".into());
    f.app.repos.idp_repo.insert(&idp).await.unwrap();
    let mapping = EmailDomainMapping::new("fed.test", idp.id.clone(), ScopeType::Client);
    f.app.repos.edm_repo.insert(&mapping).await.unwrap();
    let (status, body) = post(
        &f,
        &svc,
        &users,
        json!({"email": "sam@fed.test", "name": "Sam", "clientId": f.c1.id.as_str(), "roles": ["clerk"]}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["invitationSent"], false);
    assert!(body.get("inviteLink").is_none(), "{body}");
    let federated = body["id"].as_str().unwrap();
    assert!(
        f.tokens_of(federated).await.is_empty(),
        "no invite for a federated user"
    );
    let (status, got) = get(&f, &svc, &format!("{users}/{federated}")).await;
    assert_eq!(status, StatusCode::OK, "{got}");
    assert_eq!(got["idpType"], "OIDC");
    assert!(got.get("invitation").is_none(), "{got}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_invitation_that_is_refused_writes_nothing() {
    let f = setup().await;
    let svc = f.svc("svc-refuse", &ALL_PERMS).await;
    let users = format!("{}/users", f.base());
    let invite = |email: &str, client: &str, roles: Value, extra: Value| {
        let mut b = json!({"email": email, "name": "N", "clientId": client, "roles": roles});
        if let Value::Object(map) = extra {
            b.as_object_mut().unwrap().extend(map);
        }
        b
    };
    let c1 = f.c1.id.as_str().to_string();

    // a bad redirect writes nothing
    let (s, b) = post(
        &f,
        &svc,
        &users,
        invite(
            "badredirect@example.test",
            &c1,
            json!(["clerk"]),
            json!({"inviteRedirectUri": "javascript:alert(1)"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{b}");
    assert_eq!(code(&b), "INVITE_REDIRECT_URI_INVALID");
    assert_eq!(f.count_users("badredirect@example.test").await, 0);
    // any http(s) URL is accepted as a redirect (owner ruling 2026-09-16)
    let (s, b) = post(
        &f,
        &svc,
        &users,
        invite(
            "anyurl@example.test",
            &c1,
            json!(["clerk"]),
            json!({"inviteRedirectUri": "http://localhost:3000/home?x=1"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{b}");

    // roles: another application's, the platform's, unknown, none
    let (s, b) = post(
        &f,
        &svc,
        &users,
        invite("r1@example.test", &c1, json!(["uaoth:clerk"]), json!({})),
    )
    .await;
    assert_eq!(
        (s, code(&b)),
        (StatusCode::FORBIDDEN, "ROLE_APP_FORBIDDEN"),
        "{b}"
    );
    let (s, b) = post(
        &f,
        &svc,
        &users,
        invite(
            "r2@example.test",
            &c1,
            json!(["uaown:clerk", f.platform_role]),
            json!({}),
        ),
    )
    .await;
    assert_eq!(
        (s, code(&b)),
        (StatusCode::FORBIDDEN, "ROLE_APP_FORBIDDEN"),
        "{b}"
    );
    let (s, b) = post(
        &f,
        &svc,
        &users,
        invite("r3@example.test", &c1, json!(["nosuchrole"]), json!({})),
    )
    .await;
    assert_eq!(
        (s, code(&b)),
        (StatusCode::BAD_REQUEST, "UNKNOWN_ROLE"),
        "{b}"
    );
    let (s, b) = post(
        &f,
        &svc,
        &users,
        invite("r4@example.test", &c1, json!([]), json!({})),
    )
    .await;
    assert_eq!(
        (s, code(&b)),
        (StatusCode::BAD_REQUEST, "ROLES_REQUIRED"),
        "{b}"
    );
    for e in ["r1", "r2", "r3", "r4"] {
        assert_eq!(f.count_users(&format!("{e}@example.test")).await, 0, "{e}");
    }

    // clients: not enabled, disabled config, unknown: one answer; the identifier works
    for c in [
        f.c3.id.as_str().to_string(),
        f.c4.id.as_str().to_string(),
        "clt_nosuchclient".to_string(),
    ] {
        let (s, b) = post(
            &f,
            &svc,
            &users,
            invite(
                &format!("c-{c}@example.test"),
                &c,
                json!(["clerk"]),
                json!({}),
            ),
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{c}: {b}");
        assert!(
            b["message"]
                .as_str()
                .unwrap()
                .contains("client is not available to this application"),
            "{c}: {b}"
        );
    }
    let (s, b) = post(
        &f,
        &svc,
        &users,
        invite(
            "byident@example.test",
            &f.c1.identifier.to_uppercase(),
            json!(["clerk"]),
            json!({}),
        ),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::CREATED,
        "the identifier, case-insensitive: {b}"
    );
    let (s, b) = post(
        &f,
        &svc,
        &users,
        invite("noclient@example.test", "  ", json!(["clerk"]), json!({})),
    )
    .await;
    assert_eq!(
        (s, code(&b)),
        (StatusCode::BAD_REQUEST, "CLIENT_REQUIRED"),
        "{b}"
    );

    // an existing email conflicts
    let (s, b) = post(
        &f,
        &svc,
        &users,
        invite(&f.email_of["c3u"], &c1, json!(["clerk"]), json!({})),
    )
    .await;
    assert_eq!((s, code(&b)), (StatusCode::CONFLICT, "EMAIL_EXISTS"), "{b}");

    // a caller bound to the other application cannot invite here
    let other_svc = f
        .token("svc-oth2", Reach::Bound(&[&f.other]), &ALL_PERMS, None)
        .await;
    let (s, b) = post(
        &f,
        &other_svc,
        &users,
        invite("x2@example.test", &c1, json!(["uaown:clerk"]), json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{b}");
    assert_eq!(f.count_users("x2@example.test").await, 0);
}

// The role ceiling bounds an invitation's roles as it does every role change.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_role_ceiling_bounds_an_invitations_roles() {
    let f = setup().await;
    // An own-application role that carries a platform permission the caller lacks.
    let powerful = AuthRole::new("uaown", "powerful", "Powerful")
        .with_permission("platform:admin:client:create");
    f.app.repos.role_repo.insert(&powerful).await.unwrap();
    let svc = f.svc("svc-ceiling", &ALL_PERMS).await;
    let (s, b) = post(
        &f,
        &svc,
        &format!("{}/users", f.base()),
        json!({"email": "ceil@example.test", "name": "C", "clientId": f.c1.id.as_str(), "roles": ["powerful"]}),
    )
    .await;
    assert_eq!(
        (s, code(&b)),
        (StatusCode::FORBIDDEN, "ROLE_ABOVE_CALLER"),
        "{b}"
    );
    assert_eq!(f.count_users("ceil@example.test").await, 0);
    // a caller that holds the permission may grant it
    let holder = f
        .svc(
            "svc-ceiling-ok",
            &[PERM_USER_CREATE, "platform:admin:client:create"],
        )
        .await;
    let (s, b) = post(
        &f,
        &holder,
        &format!("{}/users", f.base()),
        json!({"email": "ceil@example.test", "name": "C", "clientId": f.c1.id.as_str(), "roles": ["powerful"]}),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn invitations_are_the_passwordless_active_internal_users() {
    let f = setup().await;
    let svc = f.svc("svc-invs", &ALL_PERMS).await;
    let list = |q: &str| {
        let path = format!("{}/invitations?pageSize=200{q}", f.base());
        let (f, svc) = (&f, svc.clone());
        async move { get(f, &svc, &path).await }
    };
    // "pend" (live token) is PENDING; the inactive, the password-holding and
    // the federated users are not invitations; the other app's and unlinked
    // users never appear.
    let (s, b) = list("").await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(invitation_ids(&b), f.ids_of(&["pend"]));
    assert_eq!(
        invitation_ids(&list("&status=PENDING").await.1),
        f.ids_of(&["pend"])
    );
    assert!(invitation_ids(&list("&status=EXPIRED").await.1).is_empty());
    let p = &list("").await.1["invitations"][0];
    assert_eq!(p["status"], "PENDING");
    assert!(p["expiresAt"].is_string());
    assert_eq!(p["roles"], json!(["uaown:clerk"]));

    // the token lapses: EXPIRED, still listed, so it can be resent
    sqlx::query("UPDATE iam_password_reset_tokens SET expires_at = NOW() - interval '1 hour' WHERE principal_id = $1")
        .bind(&f.u["pend"])
        .execute(&f.app.pool)
        .await
        .unwrap();
    assert_eq!(
        invitation_ids(&list("&status=EXPIRED").await.1),
        f.ids_of(&["pend"])
    );
    assert!(invitation_ids(&list("&status=PENDING").await.1).is_empty());
    // the janitor purges it: still an invitation (no token, no password)
    sqlx::query("DELETE FROM iam_password_reset_tokens WHERE principal_id = $1")
        .bind(&f.u["pend"])
        .execute(&f.app.pool)
        .await
        .unwrap();
    let (_, b) = list("&status=EXPIRED").await;
    assert_eq!(b["invitations"].as_array().unwrap().len(), 1);
    assert!(b["invitations"][0].get("expiresAt").is_none(), "{b}");

    let (s, b) = list("&status=bogus").await;
    assert_eq!(
        (s, code(&b)),
        (StatusCode::BAD_REQUEST, "INVALID_STATUS"),
        "{b}"
    );

    // pagination
    for i in 1..=3 {
        let (s, b) = post(
            &f,
            &svc,
            &format!("{}/users", f.base()),
            json!({"email": format!("{}@example.test", "p".repeat(i)), "name": "P",
                   "clientId": f.c1.id.as_str(), "roles": ["clerk"]}),
        )
        .await;
        assert_eq!(s, StatusCode::CREATED, "{b}");
    }
    let (_, all) = list("").await;
    assert_eq!(all["total"], 4);
    let page = |p: i64| {
        let path = format!("{}/invitations?page={p}&pageSize=3", f.base());
        let (f, svc) = (&f, svc.clone());
        async move { get(f, &svc, &path).await.1 }
    };
    let (p1, p2) = (page(0).await, page(1).await);
    assert_eq!(p1["invitations"].as_array().unwrap().len(), 3);
    assert_eq!(p2["invitations"].as_array().unwrap().len(), 1);
    assert_eq!(p2["total"], 4);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn resend_replaces_the_link_and_revoke_deactivates() {
    let f = setup().await;
    let svc = f.svc("svc-resend", &ALL_PERMS).await;
    let inv = |id: &str| format!("{}/invitations/{id}", f.base());
    let pend = f.u["pend"].clone();

    // resend emails a fresh invitation and replaces the old token
    let old = f.tokens_of(&pend).await;
    assert_eq!(old.len(), 1);
    let (s, b) = post(&f, &svc, &format!("{}/resend", inv(&pend)), json!({})).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert!(b.get("inviteLink").is_none(), "{b}");
    assert!(b["expiresAt"].is_string(), "{b}");
    let fresh = f.tokens_of(&pend).await;
    assert_eq!(fresh.len(), 1);
    assert_ne!(fresh[0].0, old[0].0, "the earlier link no longer works");

    // resend with no body at all is the defaults
    let resp = f
        .app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{}/resend", inv(&pend)))
                .header("authorization", format!("Bearer {svc}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // resend can return the link instead
    let (s, b) = post(
        &f,
        &svc,
        &format!("{}/resend", inv(&pend)),
        json!({"returnInviteLink": true}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert!(
        b["inviteLink"]
            .as_str()
            .unwrap()
            .contains("set-password?token="),
        "{b}"
    );

    // nothing to deliver is refused
    let (s, b) = post(
        &f,
        &svc,
        &format!("{}/resend", inv(&pend)),
        json!({"sendInvitation": false}),
    )
    .await;
    assert_eq!(
        (s, code(&b)),
        (StatusCode::BAD_REQUEST, "INVITATION_DELIVERY_REQUIRED"),
        "{b}"
    );

    // not an invitee, not visible, not a user: not found, and untouched
    for k in ["done", "off", "fed", "c3u", "anchor", "noRole"] {
        let (s, b) = post(&f, &svc, &format!("{}/resend", inv(&f.u[k])), json!({})).await;
        assert_eq!(
            (s, code(&b)),
            (StatusCode::NOT_FOUND, "NOT_FOUND"),
            "{k}: {b}"
        );
        assert!(
            b["message"].as_str().unwrap().contains("Invitation"),
            "{k}: {b}"
        );
        let (s, b) = read_json(f.app.delete(&inv(&f.u[k]), &svc).await).await;
        assert_eq!(
            (s, code(&b)),
            (StatusCode::NOT_FOUND, "NOT_FOUND"),
            "{k}: {b}"
        );
    }
    let (s, _) = post(
        &f,
        &svc,
        &format!("{}/resend", inv("prn_nosuchuser")),
        json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    for k in ["done", "c3u", "anchor", "noRole"] {
        assert!(f.active(&f.u[k]).await, "{k} was not touched");
    }

    // revoke removes the link and deactivates the invitee
    let resp = f.app.delete(&inv(&pend), &svc).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(f.tokens_of(&pend).await.iter().all(|t| t.1 != "invite"));
    let (s, got) = get(&f, &svc, &format!("{}/users/{pend}", f.base())).await;
    assert_eq!(s, StatusCode::OK, "{got}");
    assert_eq!(got["active"], false);
    assert!(got.get("invitation").is_none(), "{got}");
    let (_, invs) = get(&f, &svc, &format!("{}/invitations", f.base())).await;
    assert!(invs["invitations"].as_array().unwrap().is_empty(), "{invs}");
    // a second revoke: it is no longer an invitation
    let (s, b) = read_json(f.app.delete(&inv(&pend), &svc).await).await;
    assert_eq!((s, code(&b)), (StatusCode::NOT_FOUND, "NOT_FOUND"), "{b}");
    // and a resend of the deactivated user is a 404 too
    let (s, _) = post(&f, &svc, &format!("{}/resend", inv(&pend)), json!({})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

/// The use case enforces the permission, the application scope and the
/// caller's reach over the client itself, with no handler in front (the route
/// checks them first, as Go's; the use case cannot be reached around them).
/// Each refusal writes nothing.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_invite_use_case_checks_permission_application_and_client_reach_itself() {
    use std::collections::HashSet;

    use fc_platform::principal::operations::{
        InviteApplicationUserCommand, InviteApplicationUserUseCase,
    };
    use fc_platform::shared::authorization_service::{ApplicationScope, AuthContext, Credential};
    use fc_platform::usecase::{ExecutionContext, UseCase};
    use fc_platform::PrincipalType;

    let f = setup().await;
    let use_case = InviteApplicationUserUseCase::new(
        f.app.repos.principal_repo.clone(),
        f.app.repos.role_repo.clone(),
        f.app.repos.application_repo.clone(),
        f.app.unit_of_work.clone(),
    );
    let cmd = || InviteApplicationUserCommand {
        application_code: "uaown".to_string(),
        email: "direct@example.test".to_string(),
        name: "Direct".to_string(),
        client_id: f.c1.id.as_str().to_string(),
        roles: vec!["uaown:clerk".to_string()],
        idp_type: None,
    };
    let caller = |scope: UserScope, clients: &[&str], perms: &[&str], apps: ApplicationScope| {
        ExecutionContext::from_auth(&AuthContext {
            principal_id: PrincipalId::parse("prn_caller000001").unwrap(),
            principal_type: PrincipalType::Service,
            scope,
            email: None,
            name: "Caller".to_string(),
            accessible_clients: clients.iter().map(|c| c.to_string()).collect(),
            permissions: perms.iter().map(|p| p.to_string()).collect::<HashSet<_>>(),
            roles: vec![],
            credential: Credential::BearerToken,
        })
        .with_application_scope(apps)
    };
    let own_only = || ApplicationScope::Only(HashSet::from([f.own.id.clone()]));
    let refused = |r: fc_platform::UseCaseResult<_>| match r.into_result() {
        Err(e) => PlatformError::from(e),
        Ok(_) => panic!("expected a refusal"),
    };

    // No permission.
    let err = refused(
        use_case
            .run(cmd(), caller(UserScope::Anchor, &["*"], &[], own_only()))
            .await,
    );
    assert!(
        matches!(&err, PlatformError::Coded { code, .. } if code == "PERMISSION_REQUIRED"),
        "{err:?}"
    );

    // The permission, but the application is not in the caller's scope (a
    // missing application's 404), or the scope is unresolved.
    for apps in [
        ApplicationScope::Only(HashSet::from([f.other.id.clone()])),
        ApplicationScope::Only(HashSet::new()),
    ] {
        let err = refused(
            use_case
                .run(
                    cmd(),
                    caller(UserScope::Anchor, &["*"], &[PERM_USER_CREATE], apps),
                )
                .await,
        );
        assert!(matches!(err, PlatformError::NotFound { .. }), "{err:?}");
    }

    // A client administrator reaching another client only: the one client refusal.
    let err = refused(
        use_case
            .run(
                cmd(),
                caller(
                    UserScope::Client,
                    &[f.c2.id.as_str()],
                    &[PERM_USER_CREATE],
                    own_only(),
                ),
            )
            .await,
    );
    assert!(
        matches!(&err, PlatformError::Forbidden { message } if message.contains("client is not available to this application")),
        "{err:?}"
    );
    assert_eq!(f.count_users("direct@example.test").await, 0);

    // Everything in place: the user is created.
    let created = use_case
        .run(
            cmd(),
            caller(UserScope::Anchor, &["*"], &[PERM_USER_CREATE], own_only()),
        )
        .await
        .into_result();
    assert!(created.is_ok(), "{:?}", created.err());
    assert_eq!(f.count_users("direct@example.test").await, 1);
}
