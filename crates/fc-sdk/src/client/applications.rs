//! Application management operations.

use super::{ClientError, FlowCatalystClient};
use serde::{Deserialize, Serialize};

/// Request to create an application.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateApplicationRequest {
    /// Unique code for the application
    pub code: String,
    /// Human-readable name
    pub name: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Application type (e.g., "APPLICATION", "INTEGRATION")
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub application_type: Option<String>,
    /// Default base URL for the application
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_base_url: Option<String>,
    /// Icon URL for the application
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

/// Request to update an application.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApplicationRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

/// Client config for an application (per-client overrides).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClientConfigRequest {
    /// Whether the application is enabled for this client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Client-specific base URL override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url_override: Option<String>,
    /// Additional config key-value pairs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

/// Application response from the platform API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub application_type: String,
    #[serde(default)]
    pub default_base_url: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub service_account_id: Option<String>,
    pub active: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Application list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationListResponse {
    pub applications: Vec<ApplicationResponse>,
    #[serde(default)]
    pub total: Option<u64>,
}

/// Go's `ServiceAccountResponse`: the service account as
/// `GET /api/service-accounts/{id}` returns it. Alias of
/// [`super::service_accounts::ServiceAccount`].
pub type ServiceAccountResponse = super::service_accounts::ServiceAccount;

/// Go's `ApplicationProvisionServiceAccountResponse`: the answer to
/// `POST /api/applications/{id}/provision-service-account`. The OAuth
/// client secret inside is shown only once.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationProvisionServiceAccountResponse {
    pub message: String,
    pub service_account: ApplicationServiceAccountCredentials,
}

/// The provisioned service account (Go's
/// `ApplicationServiceAccountCredentials`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationServiceAccountCredentials {
    pub name: String,
    pub principal_id: String,
    pub oauth_client: ApplicationOAuthClientCredentials,
}

/// The provisioned OAuth client (Go's `ApplicationOAuthClientCredentials`).
/// `client_secret` is present only in this response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationOAuthClientCredentials {
    pub id: String,
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
}

/// Application role response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationRoleResponse {
    pub id: String,
    pub code: String,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub application_code: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    pub source: String,
    #[serde(default)]
    pub client_managed: bool,
}

/// The role names registered against an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationRolesResponse {
    pub roles: Vec<String>,
}

/// Per-client config for an application (Go's `ClientConfigResponse`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientConfigResponse {
    pub id: String,
    pub application_id: String,
    pub client_id: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub base_url_override: Option<String>,
    /// Free-form per-client config. The Rust platform's older `config` is
    /// still read.
    #[serde(default, alias = "config")]
    pub config_json: Option<serde_json::Value>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// Client configs list response: the platform's `{items}` (Go's shape;
/// the older `clientConfigs` is still read).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientConfigsResponse {
    #[serde(alias = "clientConfigs")]
    pub items: Vec<ClientConfigResponse>,
    #[serde(default)]
    pub total: Option<u64>,
}

/// Created response (returns the new entity's ID).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedResponse {
    pub id: String,
    #[serde(default)]
    pub message: Option<String>,
}

/// Generic success response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuccessResponse {
    #[serde(default)]
    pub message: Option<String>,
}

/// Applications resource accessor — created via [`FlowCatalystClient::applications`].
pub struct Applications<'a> {
    pub(crate) client: &'a FlowCatalystClient,
}

impl Applications<'_> {
    /// Create a new application.
    pub async fn create(
        &self,
        req: &CreateApplicationRequest,
    ) -> Result<CreatedResponse, ClientError> {
        self.client.post("/api/applications", req).await
    }

    /// List applications, optionally filtered by `active` and application
    /// `type` (e.g. `APPLICATION`, `INTEGRATION`). Go's platform does not
    /// paginate this list.
    pub async fn list(
        &self,
        active: Option<bool>,
        application_type: Option<&str>,
    ) -> Result<ApplicationListResponse, ClientError> {
        let mut params = Vec::new();
        if let Some(t) = application_type {
            params.push(("type", t.to_string()));
        }
        if let Some(a) = active {
            params.push(("active", a.to_string()));
        }
        let query = FlowCatalystClient::query_string(&params);
        self.client
            .get(&format!("/api/applications{}", query))
            .await
    }

    /// Get an application by ID.
    pub async fn get(&self, id: &str) -> Result<ApplicationResponse, ClientError> {
        self.client.get(&format!("/api/applications/{}", id)).await
    }

    /// Get an application by code.
    pub async fn get_by_code(&self, code: &str) -> Result<ApplicationResponse, ClientError> {
        self.client
            .get(&format!("/api/applications/by-code/{}", code))
            .await
    }

    /// Update an application. The platform answers 204; call `get(id)`
    /// for the updated record.
    pub async fn update(
        &self,
        id: &str,
        req: &UpdateApplicationRequest,
    ) -> Result<(), ClientError> {
        self.client
            .put_empty(&format!("/api/applications/{}", id), req)
            .await
    }

    /// Delete (deactivate) an application.
    pub async fn delete(&self, id: &str) -> Result<(), ClientError> {
        self.client
            .delete_req(&format!("/api/applications/{}", id))
            .await
    }

    /// Activate an application.
    pub async fn activate(&self, id: &str) -> Result<ApplicationResponse, ClientError> {
        self.client
            .post_action(&format!("/api/applications/{}/activate", id))
            .await
    }

    /// Deactivate an application.
    pub async fn deactivate(&self, id: &str) -> Result<ApplicationResponse, ClientError> {
        self.client
            .post_action(&format!("/api/applications/{}/deactivate", id))
            .await
    }

    /// Provision a service account for an application. The response
    /// carries the OAuth client secret, which is never shown again.
    pub async fn provision_service_account(
        &self,
        id: &str,
    ) -> Result<ApplicationProvisionServiceAccountResponse, ClientError> {
        self.client
            .post_action(&format!(
                "/api/applications/{}/provision-service-account",
                id
            ))
            .await
    }

    /// Get the service account attached to an application.
    ///
    /// Go's platform has no `GET …/service-account` route, so this reads the
    /// application and then `GET /api/service-accounts/{serviceAccountId}`.
    /// An application without a service account yields
    /// `ClientError::Api { status: 404, .. }`.
    pub async fn get_service_account(
        &self,
        id: &str,
    ) -> Result<ServiceAccountResponse, ClientError> {
        let app = self.get(id).await?;
        match app.service_account_id.as_deref().filter(|s| !s.is_empty()) {
            Some(sa_id) => {
                self.client
                    .get(&format!("/api/service-accounts/{}", sa_id))
                    .await
            }
            None => Err(ClientError::Api {
                status: 404,
                body: format!("application {} has no service account", id),
            }),
        }
    }

    /// List the names of the roles registered against an application (by
    /// TSID): the platform's `{roles: [name…]}`, Go's shape.
    ///
    /// Mounted under `/by-id` server-side so the admin TSID lookup doesn't
    /// collide with the SDK's `/{appCode}/roles/sync` route.
    pub async fn list_roles(&self, id: &str) -> Result<Vec<String>, ClientError> {
        let resp: ApplicationRolesResponse = self
            .client
            .get(&format!("/api/applications/by-id/{}/roles", id))
            .await?;
        Ok(resp.roles)
    }

    /// List per-client configs for an application.
    pub async fn list_clients(&self, id: &str) -> Result<ClientConfigsResponse, ClientError> {
        self.client
            .get(&format!("/api/applications/{}/clients", id))
            .await
    }

    /// Get one client's config for an application (Go's
    /// `GET /api/applications/{id}/clients/{clientId}`).
    pub async fn get_client_config(
        &self,
        id: &str,
        client_id: &str,
    ) -> Result<ClientConfigResponse, ClientError> {
        self.client
            .get(&format!("/api/applications/{}/clients/{}", id, client_id))
            .await
    }

    /// Update per-client config for an application.
    ///
    /// Go's platform has no such PUT (only the GET plus the enable/disable
    /// POSTs); use [`Self::enable_for_client`] / [`Self::disable_for_client`]
    /// there.
    #[deprecated(note = "only the Rust platform serves this PUT")]
    pub async fn update_client_config(
        &self,
        id: &str,
        client_id: &str,
        req: &ClientConfigRequest,
    ) -> Result<ClientConfigResponse, ClientError> {
        self.client
            .put(
                &format!("/api/applications/{}/clients/{}", id, client_id),
                req,
            )
            .await
    }

    /// Enable an application for a specific client (204, as Go answers).
    pub async fn enable_for_client(&self, id: &str, client_id: &str) -> Result<(), ClientError> {
        self.client
            .post_empty(&format!(
                "/api/applications/{}/clients/{}/enable",
                id, client_id
            ))
            .await
    }

    /// Disable an application for a specific client (204, as Go answers).
    pub async fn disable_for_client(&self, id: &str, client_id: &str) -> Result<(), ClientError> {
        self.client
            .post_empty(&format!(
                "/api/applications/{}/clients/{}/disable",
                id, client_id
            ))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::test_support::MockPlatform;

    const APP: &str = r#"{"id":"app_1","code":"orders","name":"Orders","type":"APPLICATION",
        "active":true,"hasLoginClient":false,"serviceAccountId":"sa_1",
        "createdAt":"t","updatedAt":"t"}"#;
    const APP_NO_SA: &str = r#"{"id":"app_2","code":"hr","name":"HR","type":"APPLICATION",
        "active":true,"hasLoginClient":false,"createdAt":"t","updatedAt":"t"}"#;
    const SA: &str = r#"{"id":"sa_1","code":"orders-sa","name":"Orders SA","active":true,
        "authType":"BEARER_TOKEN","clientIds":[],"roles":["orders:admin"],
        "applicationId":"app_1","principalId":"prn_1","oauthClientId":"oc_1",
        "createdAt":"t","updatedAt":"t"}"#;

    #[tokio::test]
    async fn get_service_account_reads_the_application_then_the_service_account() {
        let stub = MockPlatform::start(&[
            ("GET", "/api/applications/app_1", 200, APP),
            ("GET", "/api/service-accounts/sa_1", 200, SA),
        ])
        .await;
        let sa = stub
            .client()
            .applications()
            .get_service_account("app_1")
            .await
            .unwrap();
        assert_eq!(sa.id, "sa_1");
        assert_eq!(sa.principal_id.as_deref(), Some("prn_1"));
        assert_eq!(sa.oauth_client_id.as_deref(), Some("oc_1"));
        let paths: Vec<_> = stub
            .requests()
            .into_iter()
            .map(|r| (r.method, r.path))
            .collect();
        assert_eq!(
            paths,
            vec![
                ("GET".to_string(), "/api/applications/app_1".to_string()),
                ("GET".to_string(), "/api/service-accounts/sa_1".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn get_service_account_is_not_found_without_one() {
        let stub = MockPlatform::start(&[("GET", "/api/applications/app_2", 200, APP_NO_SA)]).await;
        let err = stub
            .client()
            .applications()
            .get_service_account("app_2")
            .await
            .unwrap_err();
        assert!(
            matches!(err, ClientError::Api { status: 404, .. }),
            "{err:?}"
        );
        assert_eq!(stub.requests().len(), 1);
    }

    #[tokio::test]
    async fn provision_service_account_keeps_the_one_time_secret() {
        let stub = MockPlatform::start(&[(
            "POST",
            "/api/applications/app_1/provision-service-account",
            201,
            r#"{"message":"provisioned","serviceAccount":{"name":"Orders SA",
                "principalId":"prn_1","oauthClient":{"id":"oc_1","clientId":"cid",
                "clientSecret":"s3cret"}}}"#,
        )])
        .await;
        let resp = stub
            .client()
            .applications()
            .provision_service_account("app_1")
            .await
            .unwrap();
        assert_eq!(resp.service_account.principal_id, "prn_1");
        assert_eq!(resp.service_account.oauth_client.client_id, "cid");
        assert_eq!(
            resp.service_account.oauth_client.client_secret.as_deref(),
            Some("s3cret")
        );
        assert_eq!(stub.single().method, "POST");
    }

    #[tokio::test]
    async fn get_client_config_reads_gos_shape() {
        let stub = MockPlatform::start(&[(
            "GET",
            "/api/applications/app_1/clients/clt_1",
            200,
            r#"{"id":"acc_1","applicationId":"app_1","clientId":"clt_1","enabled":true,
                "baseUrlOverride":"https://x","configJson":{"k":"v"},
                "createdAt":"c","updatedAt":"u"}"#,
        )])
        .await;
        let cfg = stub
            .client()
            .applications()
            .get_client_config("app_1", "clt_1")
            .await
            .unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.base_url_override.as_deref(), Some("https://x"));
        assert_eq!(cfg.config_json, Some(serde_json::json!({"k": "v"})));
        assert_eq!(cfg.created_at, "c");
        assert_eq!(cfg.updated_at, "u");
    }

    #[tokio::test]
    async fn list_clients_reads_items_and_config_json() {
        let stub = MockPlatform::start(&[(
            "GET",
            "/api/applications/app_1/clients",
            200,
            r#"{"items":[{"id":"acc_1","applicationId":"app_1","clientId":"clt_1",
                "enabled":false,"configJson":{"a":1},"createdAt":"c","updatedAt":"u"}]}"#,
        )])
        .await;
        let list = stub
            .client()
            .applications()
            .list_clients("app_1")
            .await
            .unwrap();
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0].config_json, Some(serde_json::json!({"a": 1})));
    }

    #[tokio::test]
    async fn list_roles_decodes_the_roles_envelope() {
        let stub = MockPlatform::start(&[(
            "GET",
            "/api/applications/by-id/app_1/roles",
            200,
            r#"{"roles":["orders:admin","orders:viewer"]}"#,
        )])
        .await;
        let roles = stub
            .client()
            .applications()
            .list_roles("app_1")
            .await
            .unwrap();
        assert_eq!(roles, vec!["orders:admin", "orders:viewer"]);
    }

    #[tokio::test]
    async fn list_sends_only_gos_filters() {
        let stub = MockPlatform::start(&[(
            "GET",
            "/api/applications",
            200,
            r#"{"applications":[],"total":0}"#,
        )])
        .await;
        stub.client()
            .applications()
            .list(Some(true), Some("INTEGRATION"))
            .await
            .unwrap();
        assert_eq!(
            stub.single().query_pairs(),
            vec![
                ("type".to_string(), "INTEGRATION".to_string()),
                ("active".to_string(), "true".to_string())
            ]
        );
    }

    #[tokio::test]
    async fn update_accepts_204() {
        let stub = MockPlatform::start(&[("PUT", "/api/applications/app_1", 204, "")]).await;
        stub.client()
            .applications()
            .update(
                "app_1",
                &UpdateApplicationRequest {
                    name: Some("New".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let req = stub.single();
        assert_eq!(req.method, "PUT");
        assert_eq!(req.json(), serde_json::json!({"name": "New"}));
    }
}
