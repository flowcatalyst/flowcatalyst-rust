//! The operations Go documents that this platform routes through plain axum
//! routers (generic handlers `routes!` cannot take, or routers that also
//! carry undocumented routes), added to the published document so that
//! `/q/openapi` describes the same programmable surface as Go's huma document
//! (`flowcatalyst-go/api/openapi.lock.json`, vendored as
//! `frontend/openapi/openapi.json`). The SDKs' generated clients are
//! generated from that document; `tests/openapi_go_contract_test.rs` pins the
//! operation ids.
//!
//! Only the handlers Go documents are listed. Routes Go lacks (e.g.
//! `/api/anchor-domains/check/{domain}`, the auth-config sub-resources) stay
//! out of the document, as before.
//!
//! [`shape_as_go_contract`] then applies the conventions of Go's document
//! that no per-handler annotation expresses (see its docs).

use serde_json::{json, Map, Value};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(paths(
    crate::auth::config_api::list_anchor_domains,
    crate::auth::config_api::create_anchor_domain,
    crate::auth::config_api::update_anchor_domain,
    crate::auth::config_api::delete_anchor_domain,
))]
struct AnchorDomainsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::auth::config_api::list_client_auth_configs,
    crate::auth::config_api::create_client_auth_config,
    crate::auth::config_api::update_client_auth_config,
    crate::auth::config_api::delete_client_auth_config,
))]
struct AuthConfigsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::auth::config_api::list_idp_role_mappings,
    crate::auth::config_api::create_idp_role_mapping,
    crate::auth::config_api::delete_idp_role_mapping,
))]
struct IdpRoleMappingsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::application::api::list_applications,
    crate::application::api::create_application,
    crate::application::api::get_application_by_code,
    crate::application::api::list_application_roles,
    crate::application::api::get_application,
    crate::application::api::update_application,
    crate::application::api::delete_application,
    crate::application::api::activate_application,
    crate::application::api::deactivate_application,
    crate::application::api::list_client_configs,
    crate::application::api::enable_for_client,
    crate::application::api::disable_for_client,
    crate::application::api::provision_login_client,
    crate::application::api::provision_service_account,
))]
struct ApplicationsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::connection::api::list_connections,
    crate::connection::api::create_connection,
    crate::connection::api::get_connection,
    crate::connection::api::update_connection,
    crate::connection::api::delete_connection,
    crate::connection::api::activate_connection,
    crate::connection::api::pause_connection,
))]
struct ConnectionsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::dispatch_pool::api::list_dispatch_pools,
    crate::dispatch_pool::api::create_dispatch_pool,
    crate::dispatch_pool::api::get_dispatch_pool,
    crate::dispatch_pool::api::update_dispatch_pool,
    crate::dispatch_pool::api::delete_dispatch_pool,
    crate::dispatch_pool::api::activate_dispatch_pool,
    crate::dispatch_pool::api::archive_dispatch_pool,
    crate::dispatch_pool::api::suspend_dispatch_pool,
))]
struct DispatchPoolsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::email_domain_mapping::api::list_email_domain_mappings,
    crate::email_domain_mapping::api::create_email_domain_mapping,
    crate::email_domain_mapping::api::get_email_domain_mapping,
    crate::email_domain_mapping::api::update_email_domain_mapping,
    crate::email_domain_mapping::api::delete_email_domain_mapping,
))]
struct EmailDomainMappingsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::identity_provider::api::list_identity_providers,
    crate::identity_provider::api::create_identity_provider,
    crate::identity_provider::api::get_identity_provider,
    crate::identity_provider::api::update_identity_provider,
    crate::identity_provider::api::delete_identity_provider,
))]
struct IdentityProvidersDoc;

#[derive(OpenApi)]
#[openapi(paths(crate::login_attempt::api::list_login_attempts))]
struct LoginAttemptsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::cors::api::list_cors_origins,
    crate::cors::api::create_cors_origin,
    crate::cors::api::get_allowed_origins,
    crate::cors::api::get_cors_origin,
    crate::cors::api::delete_cors_origin,
))]
struct CorsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::portal::api::list_portal_apps,
    crate::portal::api::create_portal_app,
    crate::portal::api::update_portal_app,
    crate::portal::api::delete_portal_app,
    crate::portal::api::assign_unassigned_portal_users,
))]
struct PortalAppsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::portal::api::list_portal_users,
    crate::portal::api::ensure_portal_user,
    crate::portal::api::delete_portal_user,
    crate::portal::api::activate_portal_user,
    crate::portal::api::deactivate_portal_user,
    crate::portal::api::grant_portal_user_app,
    crate::portal::api::revoke_portal_user_app,
))]
struct PortalUsersDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::service_account::api::list_service_accounts,
    crate::service_account::api::create_service_account,
    crate::service_account::api::get_service_account_by_code,
    crate::service_account::api::get_service_account,
    crate::service_account::api::update_service_account,
    crate::service_account::api::delete_service_account,
    crate::service_account::api::get_roles,
    crate::service_account::api::assign_roles,
    crate::service_account::api::regenerate_auth_token,
    crate::service_account::api::regenerate_token_alias,
    crate::service_account::api::regenerate_signing_secret,
    crate::service_account::api::regenerate_secret_alias,
))]
struct ServiceAccountsDoc;

#[derive(OpenApi)]
#[openapi(paths(crate::shared::batch_api::batch_events))]
struct EventsBatchDoc;

/// The operations above, at their mount points.
pub fn documented_plain_routes() -> utoipa::openapi::OpenApi {
    utoipa::openapi::OpenApiBuilder::new()
        .build()
        .nest("/api/anchor-domains", AnchorDomainsDoc::openapi())
        .nest("/api/auth-configs", AuthConfigsDoc::openapi())
        .nest("/api/idp-role-mappings", IdpRoleMappingsDoc::openapi())
        .nest("/api/applications", ApplicationsDoc::openapi())
        .nest("/api/connections", ConnectionsDoc::openapi())
        .nest("/api/dispatch-pools", DispatchPoolsDoc::openapi())
        .nest(
            "/api/email-domain-mappings",
            EmailDomainMappingsDoc::openapi(),
        )
        .nest("/api/identity-providers", IdentityProvidersDoc::openapi())
        .nest("/api/login-attempts", LoginAttemptsDoc::openapi())
        .nest("/api/platform/cors", CorsDoc::openapi())
        .nest("/api/portal-apps", PortalAppsDoc::openapi())
        .nest("/api/portal-users", PortalUsersDoc::openapi())
        .nest("/api/service-accounts", ServiceAccountsDoc::openapi())
        .nest("/api/events", EventsBatchDoc::openapi())
}

/// Go's error envelope as its document names it (`httpcompat.ErrorModel`):
/// `{error, message, details?}`. This platform's error bodies carry the same
/// members plus `code` (the same value as `error`).
fn error_model() -> Value {
    json!({
        "type": "object",
        "properties": {
            "details": {"type": "object", "additionalProperties": {}},
            "error": {"type": "string"},
            "message": {"type": "string"}
        },
        "required": ["error", "message"]
    })
}

/// Reshape the published document (`/q/openapi`) to the conventions of Go's
/// huma document, which the SDKs' generated clients are generated from:
///
/// 1. **Errors.** Go documents one success response and a `default` response
///    with `ErrorModel` per operation. The per-status error responses the
///    handlers annotate (mostly without a body) are replaced by that
///    `default`, and `ErrorModel` is added.
/// 2. **Optional members are not nullable.** Go types an optional member as
///    its plain type (absent when unset) and uses `[T, "null"]` only for a
///    required member that may be null. `Option<T>` makes utoipa emit
///    `[T, "null"]` (or `oneOf [null, T]`) everywhere; for members that are not
///    required, optional query parameters and optional request bodies, the
///    `null` is dropped.
/// 3. **Closed objects.** Go marks each operation's top-level request body
///    schema `additionalProperties: true` (a body may carry members the
///    platform ignores, as serde here does) and every other object schema
///    `additionalProperties: false`; the component schemas get the same.
/// 4. **No orphans of this document's own making.** Component schemas no
///    operation reaches (e.g. `ErrorResponse`, `PaginationParams`, the
///    shapes of the error responses dropped in 1) are removed.
///
/// The full document (`/q/openapi-full`, including `/bff`) is not reshaped.
pub fn shape_as_go_contract(doc: &mut Value) {
    reshape(doc);
}

fn reshape(doc: &mut Value) {
    if let Some(paths) = doc.get_mut("paths").and_then(Value::as_object_mut) {
        for item in paths.values_mut() {
            let Some(item) = item.as_object_mut() else {
                continue;
            };
            for (method, op) in item.iter_mut() {
                if !matches!(method.as_str(), "get" | "put" | "post" | "delete" | "patch") {
                    continue;
                }
                shape_operation(op);
            }
        }
    }
    if let Some(schemas) = doc
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
    {
        schemas.insert("ErrorModel".to_string(), error_model());
        for schema in schemas.values_mut() {
            strip_optional_nulls(schema);
        }
    }
    drop_unreachable_schemas(doc);
    mark_additional_properties(doc);
}

/// Rule 3: `additionalProperties` on the component object schemas.
fn mark_additional_properties(doc: &mut Value) {
    let mut request_bodies = Vec::new();
    if let Some(paths) = doc.get("paths").and_then(Value::as_object) {
        for item in paths.values().filter_map(Value::as_object) {
            for op in item.values() {
                if let Some(content) = op
                    .pointer("/requestBody/content")
                    .and_then(Value::as_object)
                {
                    for media in content.values() {
                        if let Some(Value::String(r)) = media.pointer("/schema/$ref") {
                            if let Some(name) = r.strip_prefix("#/components/schemas/") {
                                request_bodies.push(name.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(schemas) = doc
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
    {
        for (name, schema) in schemas.iter_mut() {
            let Some(obj) = schema.as_object_mut() else {
                continue;
            };
            if !obj.contains_key("properties") || obj.contains_key("additionalProperties") {
                continue;
            }
            let open = request_bodies.contains(name);
            obj.insert("additionalProperties".to_string(), Value::Bool(open));
        }
    }
}

fn shape_operation(op: &mut Value) {
    if let Some(responses) = op.get_mut("responses").and_then(Value::as_object_mut) {
        responses.retain(|code, _| code.starts_with('2'));
        responses.insert(
            "default".to_string(),
            json!({
                "description": "Error",
                "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ErrorModel"}}}
            }),
        );
    }
    if let Some(params) = op.get_mut("parameters").and_then(Value::as_array_mut) {
        for param in params {
            let required = param.get("required").and_then(Value::as_bool) == Some(true);
            if !required {
                if let Some(schema) = param.get_mut("schema") {
                    unwrap_nullable(schema);
                }
            }
            if let Some(schema) = param.get_mut("schema") {
                strip_optional_nulls(schema);
            }
        }
    }
    if let Some(body) = op.get_mut("requestBody") {
        let required = body.get("required").and_then(Value::as_bool) == Some(true);
        if !required {
            if let Some(content) = body.get_mut("content").and_then(Value::as_object_mut) {
                for media in content.values_mut() {
                    if let Some(schema) = media.get_mut("schema") {
                        unwrap_nullable(schema);
                    }
                }
            }
        }
    }
}

/// Every object schema reachable inside `schema`: its members that are not
/// required lose their `null`, and integers lose the `minimum: 0` of an
/// unsigned Rust type (Go documents no bounds).
fn strip_optional_nulls(schema: &mut Value) {
    let Some(obj) = schema.as_object_mut() else {
        return;
    };
    if obj.get("minimum") == Some(&json!(0)) {
        obj.remove("minimum");
    }
    let required: Vec<String> = obj
        .get("required")
        .and_then(Value::as_array)
        .map(|r| {
            r.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if let Some(props) = obj.get_mut("properties").and_then(Value::as_object_mut) {
        for (name, prop) in props.iter_mut() {
            if !required.contains(name) {
                unwrap_nullable(prop);
            }
            strip_optional_nulls(prop);
        }
    }
    for key in ["items", "additionalProperties"] {
        if let Some(inner) = obj.get_mut(key) {
            strip_optional_nulls(inner);
        }
    }
    for key in ["allOf", "oneOf", "anyOf"] {
        if let Some(list) = obj.get_mut(key).and_then(Value::as_array_mut) {
            for inner in list {
                strip_optional_nulls(inner);
            }
        }
    }
}

/// `[T, "null"]` -> `T`; `oneOf [{type: null}, X]` -> `X` (keeping the
/// description).
fn unwrap_nullable(schema: &mut Value) {
    let Some(obj) = schema.as_object_mut() else {
        return;
    };
    if let Some(Value::Array(types)) = obj.get("type") {
        let kept: Vec<Value> = types
            .iter()
            .filter(|t| t.as_str() != Some("null"))
            .cloned()
            .collect();
        if kept.len() != types.len() {
            let replacement = if kept.len() == 1 {
                kept[0].clone()
            } else {
                Value::Array(kept)
            };
            obj.insert("type".to_string(), replacement);
        }
    }
    for key in ["oneOf", "anyOf"] {
        let Some(Value::Array(list)) = obj.get(key) else {
            continue;
        };
        let non_null: Vec<&Value> = list
            .iter()
            .filter(|v| v.get("type").and_then(Value::as_str) != Some("null"))
            .collect();
        if non_null.len() == 1 && list.len() == 2 {
            let mut inner: Map<String, Value> =
                non_null[0].as_object().cloned().unwrap_or_default();
            obj.remove(key);
            if let Some(description) = obj.remove("description") {
                inner.entry("description").or_insert(description);
            }
            for (k, v) in obj.iter() {
                inner.entry(k.clone()).or_insert_with(|| v.clone());
            }
            *schema = Value::Object(inner);
            return;
        }
    }
}

/// Remove component schemas no path reaches (transitively).
fn drop_unreachable_schemas(doc: &mut Value) {
    let mut reachable = std::collections::BTreeSet::new();
    let mut queue = Vec::new();
    collect_refs(doc.get("paths").unwrap_or(&Value::Null), &mut queue);
    while let Some(name) = queue.pop() {
        if !reachable.insert(name.clone()) {
            continue;
        }
        if let Some(schema) = doc.pointer(&format!("/components/schemas/{name}")) {
            collect_refs(schema, &mut queue);
        }
    }
    if let Some(schemas) = doc
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
    {
        schemas.retain(|name, _| reachable.contains(name));
    }
}

fn collect_refs(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(obj) => {
            if let Some(Value::String(r)) = obj.get("$ref") {
                if let Some(name) = r.strip_prefix("#/components/schemas/") {
                    out.push(name.to_string());
                }
            }
            for v in obj.values() {
                collect_refs(v, out);
            }
        }
        Value::Array(list) => list.iter().for_each(|v| collect_refs(v, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_members_lose_null_and_required_ones_keep_it() {
        let mut doc = json!({
            "paths": {"/x": {"get": {
                "parameters": [{"in": "query", "name": "q", "required": false, "schema": {"type": ["string", "null"]}}],
                "responses": {
                    "200": {"description": "OK", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/X"}}}},
                    "404": {"description": "Not found", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Gone"}}}}
                }
            }}},
            "components": {"schemas": {
                "X": {"type": "object", "required": ["a"], "properties": {
                    "a": {"type": ["string", "null"]},
                    "b": {"type": ["string", "null"]},
                    "c": {"oneOf": [{"type": "null"}, {"$ref": "#/components/schemas/Y"}], "description": "d"}
                }},
                "Y": {"type": "object"},
                "Gone": {"type": "object"}
            }}
        });
        reshape(&mut doc);
        let x = &doc["components"]["schemas"]["X"]["properties"];
        assert_eq!(x["a"]["type"], json!(["string", "null"]));
        assert_eq!(x["b"]["type"], json!("string"));
        assert_eq!(
            x["c"],
            json!({"$ref": "#/components/schemas/Y", "description": "d"})
        );
        let op = &doc["paths"]["/x"]["get"];
        assert_eq!(op["parameters"][0]["schema"]["type"], json!("string"));
        assert!(op["responses"].get("404").is_none());
        assert_eq!(
            op["responses"]["default"]["content"]["application/json"]["schema"]["$ref"],
            json!("#/components/schemas/ErrorModel")
        );
        let schemas = doc["components"]["schemas"].as_object().unwrap();
        assert_eq!(schemas["X"]["additionalProperties"], json!(false));
        assert!(schemas.contains_key("ErrorModel"));
        assert!(schemas.contains_key("Y"));
        assert!(!schemas.contains_key("Gone"));
    }
}
