//! HashiCorp Vault KV secrets engine provider
//!
//! Reference format: vault://path/to/secret#key
//! - path/to/secret: The path in Vault (relative to the mount point)
//! - key: The key within the secret (defaults to "value" if not specified)
//!
//! Configuration:
//! - vault_addr: Vault server address (e.g., "http://vault:8200")
//! - vault_token: Authentication token

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use tracing::{debug, info};

use crate::{Provider, SecretsError};

/// HashiCorp Vault secret provider (KV v2 secrets engine)
pub struct VaultProvider {
    client: Client,
    addr: String,
    mount_path: String,
    token: Option<String>,
}

impl VaultProvider {
    /// Create a new Vault provider
    ///
    /// # Arguments
    /// * `addr` - Vault server address (e.g., "http://vault:8200")
    /// * `mount_path` - KV secrets engine mount path (e.g., "secret")
    /// * `token` - Optional authentication token (can also use VAULT_TOKEN env var)
    pub fn new(addr: &str, mount_path: String, token: Option<String>) -> Result<Self, SecretsError> {
        let client = Client::builder()
            .build()
            .map_err(|e| SecretsError::ProviderError(format!("Failed to create HTTP client: {}", e)))?;

        // Get token from parameter or environment variable
        let actual_token = token.or_else(|| std::env::var("VAULT_TOKEN").ok());

        info!(addr = %addr, mount_path = %mount_path, "Initialized HashiCorp Vault provider");

        Ok(Self {
            client,
            addr: addr.trim_end_matches('/').to_string(),
            mount_path,
            token: actual_token,
        })
    }

    /// Resolve a reference in the format `vault://path/to/secret#key`
    pub async fn resolve_reference(&self, reference: &str) -> Result<String, SecretsError> {
        const PREFIX: &str = "vault://";

        if !reference.starts_with(PREFIX) {
            return Err(SecretsError::InvalidKey(format!(
                "Invalid reference format for Vault: {}",
                reference
            )));
        }

        let path_and_key = &reference[PREFIX.len()..];
        let (path, key) = Self::parse_path_and_key(path_and_key);

        self.get_secret(&path, &key).await
    }

    /// Validate a secret exists without retrieving its value (actually retrieves to check key)
    pub async fn validate_reference(&self, reference: &str) -> Result<ValidationResult, SecretsError> {
        const PREFIX: &str = "vault://";

        if !reference.starts_with(PREFIX) {
            return Ok(ValidationResult::failure("Invalid reference format for Vault"));
        }

        let path_and_key = &reference[PREFIX.len()..];
        let (path, key) = Self::parse_path_and_key(path_and_key);

        match self.read_secret(&path).await {
            Ok(data) => {
                if data.is_empty() {
                    Ok(ValidationResult::failure(format!("Secret not found: {}", path)))
                } else if !data.contains_key(&key) {
                    let available_keys: Vec<_> = data.keys().collect();
                    Ok(ValidationResult::failure(format!(
                        "Key '{}' not found in secret (available keys: {:?})",
                        key, available_keys
                    )))
                } else {
                    Ok(ValidationResult::success(format!("Secret exists in Vault with key '{}'", key)))
                }
            }
            Err(e) => Ok(ValidationResult::failure(format!("Failed to access secret: {}", e))),
        }
    }

    /// Check if this provider can handle the given reference
    pub fn can_handle(reference: &str) -> bool {
        reference.starts_with("vault://")
    }

    /// Parse path and key from "path/to/secret#key" format
    fn parse_path_and_key(path_and_key: &str) -> (String, String) {
        if let Some(hash_idx) = path_and_key.find('#') {
            (
                path_and_key[..hash_idx].to_string(),
                path_and_key[hash_idx + 1..].to_string(),
            )
        } else {
            (path_and_key.to_string(), "value".to_string())
        }
    }

    /// Read a secret from Vault KV v2
    async fn read_secret(&self, path: &str) -> Result<HashMap<String, String>, SecretsError> {
        // KV v2 URL format: /v1/{mount}/data/{path}
        let url = format!("{}/v1/{}/data/{}", self.addr, self.mount_path, path);

        debug!(url = %url, "Reading secret from Vault");

        let mut request = self.client.get(&url);

        if let Some(token) = &self.token {
            request = request.header("X-Vault-Token", token);
        }

        let response = request.send().await.map_err(|e| {
            SecretsError::ProviderError(format!("Failed to connect to Vault: {}", e))
        })?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(SecretsError::NotFound(path.to_string()));
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SecretsError::ProviderError(format!(
                "Vault returned error {}: {}",
                status, body
            )));
        }

        let vault_response: VaultReadResponse = response.json().await.map_err(|e| {
            SecretsError::ProviderError(format!("Failed to parse Vault response: {}", e))
        })?;

        Ok(vault_response.data.data)
    }

    /// Get a specific key from a secret
    async fn get_secret(&self, path: &str, key: &str) -> Result<String, SecretsError> {
        let data = self.read_secret(path).await?;

        data.get(key)
            .cloned()
            .ok_or_else(|| SecretsError::NotFound(format!("{}#{}", path, key)))
    }
}

#[async_trait]
impl Provider for VaultProvider {
    async fn get(&self, key: &str) -> Result<String, SecretsError> {
        // Key format for Provider trait: "path/to/secret#key" or just "path/to/secret"
        let (path, secret_key) = Self::parse_path_and_key(key);
        debug!(path = %path, key = %secret_key, "Retrieving secret from Vault");
        self.get_secret(&path, &secret_key).await
    }

    async fn set(&self, _key: &str, _value: &str) -> Result<(), SecretsError> {
        // Vault is read-only in this implementation
        // Secrets should be provisioned by infrastructure teams
        Err(SecretsError::ProviderError(
            "Vault provider is read-only".to_string()
        ))
    }

    async fn delete(&self, _key: &str) -> Result<(), SecretsError> {
        // Vault is read-only in this implementation
        Err(SecretsError::ProviderError(
            "Vault provider is read-only".to_string()
        ))
    }

    fn name(&self) -> &str {
        "vault"
    }
}

/// Vault KV v2 read response structure
#[derive(Debug, Deserialize)]
struct VaultReadResponse {
    data: VaultSecretData,
}

#[derive(Debug, Deserialize)]
struct VaultSecretData {
    data: HashMap<String, String>,
    #[allow(dead_code)]
    metadata: Option<VaultMetadata>,
}

#[derive(Debug, Deserialize)]
struct VaultMetadata {
    #[allow(dead_code)]
    created_time: Option<String>,
    #[allow(dead_code)]
    version: Option<u32>,
}

/// Result of validating a secret reference
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub valid: bool,
    pub message: String,
}

impl ValidationResult {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            valid: true,
            message: message.into(),
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            valid: false,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_handle() {
        assert!(VaultProvider::can_handle("vault://secret/myapp"));
        assert!(VaultProvider::can_handle("vault://secret/myapp#password"));
        assert!(!VaultProvider::can_handle("aws-sm://secret"));
        assert!(!VaultProvider::can_handle("aws-ps://param"));
        assert!(!VaultProvider::can_handle("my-secret"));
    }

    #[test]
    fn test_parse_path_and_key() {
        let (path, key) = VaultProvider::parse_path_and_key("secret/myapp#password");
        assert_eq!(path, "secret/myapp");
        assert_eq!(key, "password");

        let (path, key) = VaultProvider::parse_path_and_key("secret/myapp");
        assert_eq!(path, "secret/myapp");
        assert_eq!(key, "value"); // default key

        let (path, key) = VaultProvider::parse_path_and_key("myapp#api_key");
        assert_eq!(path, "myapp");
        assert_eq!(key, "api_key");
    }
}
