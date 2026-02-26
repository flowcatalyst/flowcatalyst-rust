//! FlowCatalyst Secrets Management
//!
//! Provides a unified interface for secret storage with multiple backends:
//! - Environment variables (default)
//! - Encrypted local file storage (AES-256-GCM)
//! - AWS Secrets Manager (with feature flag `aws`)
//! - AWS Parameter Store (with feature flag `aws-ssm`)
//! - HashiCorp Vault (with feature flag `vault`)
//!
//! ## Reference Formats
//!
//! - `aws-sm://secret-name` - AWS Secrets Manager
//! - `aws-ps://parameter-name` - AWS Parameter Store (decrypted SecureString)
//! - `vault://path/to/secret#key` - HashiCorp Vault KV v2 (key defaults to "value")
//! - `encrypted:BASE64_CIPHERTEXT` - Local encrypted storage

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tracing::info;

mod encrypted;
mod env;

pub use encrypted::{EncryptedProvider, generate_key};
pub use env::EnvProvider;

#[cfg(feature = "aws")]
mod aws;
#[cfg(feature = "aws")]
pub use aws::{AwsSecretsManagerProvider, ValidationResult as AwsSmValidationResult};

#[cfg(feature = "aws-ssm")]
mod aws_parameter_store;
#[cfg(feature = "aws-ssm")]
pub use aws_parameter_store::{AwsParameterStoreProvider, ValidationResult as AwsPsValidationResult};

#[cfg(feature = "vault")]
mod vault;
#[cfg(feature = "vault")]
pub use vault::{VaultProvider, ValidationResult as VaultValidationResult};

mod service;
pub use service::{SecretService, ValidationResult};

#[derive(Error, Debug)]
pub enum SecretsError {
    #[error("Secret not found: {0}")]
    NotFound(String),
    #[error("Invalid key format: {0}")]
    InvalidKey(String),
    #[error("Encryption error: {0}")]
    EncryptionError(String),
    #[error("Provider error: {0}")]
    ProviderError(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

/// Configuration for secrets providers
#[derive(Debug, Clone)]
pub struct SecretsConfig {
    /// Provider for single-provider mode (legacy)
    pub provider: String,
    /// Encryption key for local encrypted storage (base64-encoded 32-byte key)
    pub encryption_key: Option<String>,
    /// Directory for local encrypted storage
    pub data_dir: PathBuf,
    /// AWS region for AWS providers
    pub aws_region: Option<String>,
    /// Prefix for AWS secret/parameter names (e.g., "/flowcatalyst/")
    pub aws_prefix: Option<String>,
    /// Enable AWS Secrets Manager provider (for SecretService)
    pub aws_sm_enabled: Option<bool>,
    /// Enable AWS Parameter Store provider (for SecretService)
    pub aws_ps_enabled: Option<bool>,
    /// HashiCorp Vault server address (e.g., "http://vault:8200")
    pub vault_addr: Option<String>,
    /// Vault KV secrets engine mount path (e.g., "secret")
    pub vault_path: Option<String>,
    /// Vault authentication token (or use VAULT_TOKEN env var)
    pub vault_token: Option<String>,
    /// Enable Vault provider (for SecretService)
    pub vault_enabled: Option<bool>,
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self {
            provider: "env".to_string(),
            encryption_key: None,
            data_dir: PathBuf::from("./data/secrets"),
            aws_region: None,
            aws_prefix: Some("/flowcatalyst/".to_string()),
            aws_sm_enabled: None,
            aws_ps_enabled: None,
            vault_addr: None,
            vault_path: Some("secret".to_string()),
            vault_token: None,
            vault_enabled: None,
        }
    }
}

/// Secrets provider trait
#[async_trait]
pub trait Provider: Send + Sync {
    /// Get a secret by key
    async fn get(&self, key: &str) -> Result<String, SecretsError>;
    
    /// Set a secret
    async fn set(&self, key: &str, value: &str) -> Result<(), SecretsError>;
    
    /// Delete a secret
    async fn delete(&self, key: &str) -> Result<(), SecretsError>;
    
    /// Provider name
    fn name(&self) -> &str;
}

/// Create a provider based on configuration
pub async fn create_provider(config: &SecretsConfig) -> Result<Arc<dyn Provider>, SecretsError> {
    match config.provider.as_str() {
        "env" => {
            info!("Using environment variable secrets provider");
            Ok(Arc::new(EnvProvider::new()))
        }
        "encrypted" => {
            let key = config.encryption_key.as_ref()
                .ok_or_else(|| SecretsError::ProviderError("Encryption key required for encrypted provider".to_string()))?;
            info!("Using encrypted file secrets provider");
            let provider = EncryptedProvider::new(key, &config.data_dir)?;
            Ok(Arc::new(provider))
        }
        #[cfg(feature = "aws")]
        "aws-sm" => {
            info!("Using AWS Secrets Manager provider");
            let provider = AwsSecretsManagerProvider::new(
                config.aws_region.clone(),
                config.aws_prefix.clone().unwrap_or_else(|| "/flowcatalyst/".to_string()),
            ).await?;
            Ok(Arc::new(provider))
        }
        #[cfg(feature = "aws-ssm")]
        "aws-ps" => {
            info!("Using AWS Parameter Store provider");
            let provider = AwsParameterStoreProvider::new(
                config.aws_region.clone(),
                config.aws_prefix.clone().unwrap_or_else(|| "/flowcatalyst/".to_string()),
            ).await?;
            Ok(Arc::new(provider))
        }
        #[cfg(feature = "vault")]
        "vault" => {
            let addr = config.vault_addr.as_ref()
                .ok_or_else(|| SecretsError::ProviderError("Vault address required".to_string()))?;
            info!("Using HashiCorp Vault provider");
            let provider = VaultProvider::new(
                addr,
                config.vault_path.clone().unwrap_or_else(|| "secret".to_string()),
                config.vault_token.clone(),
            )?;
            Ok(Arc::new(provider))
        }
        other => Err(SecretsError::ProviderError(format!("Unknown provider: {}", other))),
    }
}
