//! Service accounts — `/api/service-accounts/*`.

use serde::{Deserialize, Serialize};

use super::{ClientError, FlowCatalystClient};

/// Request body for `POST /api/service-accounts`.
///
/// A new service account has no application access. `all_applications:
/// true` grants it every application, present and future; the platform
/// answers 403 unless the caller itself reaches every application. Grant
/// single applications afterwards through the principal's application
/// access.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateServiceAccountRequest {
    pub code: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Requested scope: `ANCHOR`, `PARTNER` or `CLIENT`. The token tier
    /// follows `client_ids` (none: ANCHOR, one: CLIENT, several: PARTNER).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub client_ids: Vec<String>,
    /// Grant every application. Omitted on the wire when `false`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_applications: bool,
}

/// A service account as the `/api/service-accounts` routes return it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccount {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub client_ids: Vec<String>,
    #[serde(default)]
    pub application_id: Option<String>,
    pub active: bool,
    #[serde(default)]
    pub auth_type: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub last_used_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// The `client_credentials` pair, returned once at creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountOAuthCredentials {
    pub client_id: String,
    pub client_secret: String,
}

/// The webhook bearer token and signing secret, returned once at creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountWebhookCredentials {
    pub auth_token: String,
    pub signing_secret: String,
}

/// Response of `POST /api/service-accounts`: the account and its one-time
/// secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateServiceAccountResponse {
    pub service_account: ServiceAccount,
    pub principal_id: String,
    pub oauth: ServiceAccountOAuthCredentials,
    pub webhook: ServiceAccountWebhookCredentials,
}

/// Service accounts accessor — created via
/// [`FlowCatalystClient::service_accounts`].
pub struct ServiceAccounts<'a> {
    pub(crate) client: &'a FlowCatalystClient,
}

impl ServiceAccounts<'_> {
    /// Create a service account. The response carries the OAuth client
    /// secret and webhook credentials, which are never shown again.
    pub async fn create(
        &self,
        req: &CreateServiceAccountRequest,
    ) -> Result<CreateServiceAccountResponse, ClientError> {
        self.client.post("/api/service-accounts", req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_applications_is_sent_only_when_set() {
        let plain = CreateServiceAccountRequest {
            code: "svc".into(),
            name: "Svc".into(),
            ..Default::default()
        };
        let json = serde_json::to_value(&plain).unwrap();
        assert!(json.get("allApplications").is_none());
        assert!(json.get("clientIds").is_none());

        let all = CreateServiceAccountRequest {
            all_applications: true,
            ..plain
        };
        assert_eq!(
            serde_json::to_value(&all).unwrap()["allApplications"],
            serde_json::json!(true)
        );
    }

    #[test]
    fn create_response_reads_the_platform_shape() {
        let body = serde_json::json!({
            "serviceAccount": {
                "id": "sa_1", "code": "svc", "name": "Svc", "description": null,
                "clientIds": [], "applicationId": null, "active": true,
                "authType": "BEARER_TOKEN", "roles": [], "lastUsedAt": null,
                "createdAt": "t", "updatedAt": "t"
            },
            "principalId": "prn_1",
            "oauth": { "clientId": "cid", "clientSecret": "cs" },
            "webhook": { "authToken": "at", "signingSecret": "ss" }
        });
        let resp: CreateServiceAccountResponse = serde_json::from_value(body).unwrap();
        assert_eq!(resp.service_account.id, "sa_1");
        assert_eq!(resp.principal_id, "prn_1");
        assert_eq!(resp.oauth.client_secret, "cs");
        assert_eq!(resp.webhook.signing_secret, "ss");
    }
}
