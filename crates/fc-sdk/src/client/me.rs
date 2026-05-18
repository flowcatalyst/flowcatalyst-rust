//! Current-user context (`/api/me/*`).

use super::{ClientError, FlowCatalystClient};
use serde::{Deserialize, Serialize};

/// A client accessible to the current user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MyClient {
    pub id: String,
    pub name: String,
    pub identifier: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// Response for listing the current user's accessible clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyClientsResponse {
    pub clients: Vec<MyClient>,
    #[serde(default)]
    pub total: Option<u64>,
}

/// An application accessible to the current user within a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MyApplication {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub logo_mime_type: Option<String>,
}

/// Response for listing applications for a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MyApplicationsResponse {
    pub applications: Vec<MyApplication>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(default)]
    pub client_id: Option<String>,
}

/// Current-user accessor — created via [`FlowCatalystClient::me`].
pub struct Me<'a> {
    pub(crate) client: &'a FlowCatalystClient,
}

impl Me<'_> {
    /// All clients accessible to the current user.
    ///
    /// Access is determined by user scope:
    /// - ANCHOR: all active clients
    /// - PARTNER: IDP-granted + explicit grants
    /// - CLIENT: home client + IDP + explicit grants
    pub async fn clients(&self) -> Result<MyClientsResponse, ClientError> {
        self.client.get("/api/me/clients").await
    }

    /// A specific accessible client by ID.
    pub async fn client(&self, client_id: &str) -> Result<MyClient, ClientError> {
        self.client
            .get(&format!("/api/me/clients/{}", client_id))
            .await
    }

    /// Applications available to the current user within a client.
    pub async fn client_applications(
        &self,
        client_id: &str,
    ) -> Result<MyApplicationsResponse, ClientError> {
        self.client
            .get(&format!("/api/me/clients/{}/applications", client_id))
            .await
    }
}
