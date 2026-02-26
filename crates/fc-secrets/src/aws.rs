//! AWS Secrets Manager provider
//!
//! Reference format: aws-sm://secret-name
//!
//! Configuration via standard AWS SDK chain (env vars, instance profile, etc.)

use async_trait::async_trait;
use aws_sdk_secretsmanager::Client;
use tracing::{debug, info};

use crate::{Provider, SecretsError};

/// AWS Secrets Manager secret provider
pub struct AwsSecretsManagerProvider {
    client: Client,
    prefix: String,
}

impl AwsSecretsManagerProvider {
    /// Create a new AWS Secrets Manager provider
    ///
    /// # Arguments
    /// * `region` - Optional AWS region (uses default if not specified)
    /// * `prefix` - Prefix for secret names (e.g., "/flowcatalyst/")
    pub async fn new(region: Option<String>, prefix: String) -> Result<Self, SecretsError> {
        let config = if let Some(region) = region {
            aws_config::defaults(aws_config::BehaviorVersion::latest())
                .region(aws_config::Region::new(region))
                .load()
                .await
        } else {
            aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await
        };

        let client = Client::new(&config);
        info!(prefix = %prefix, "Initialized AWS Secrets Manager provider");

        Ok(Self { client, prefix })
    }

    /// Resolve a reference in the format `aws-sm://secret-name`
    pub async fn resolve_reference(&self, reference: &str) -> Result<String, SecretsError> {
        const PREFIX: &str = "aws-sm://";

        if !reference.starts_with(PREFIX) {
            return Err(SecretsError::InvalidKey(format!(
                "Invalid reference format for AWS Secrets Manager: {}",
                reference
            )));
        }

        let secret_name = &reference[PREFIX.len()..];
        self.get(secret_name).await
    }

    /// Validate a secret exists without retrieving its value
    pub async fn validate_reference(&self, reference: &str) -> Result<ValidationResult, SecretsError> {
        const PREFIX: &str = "aws-sm://";

        if !reference.starts_with(PREFIX) {
            return Ok(ValidationResult::failure("Invalid reference format for AWS Secrets Manager"));
        }

        let secret_name = &reference[PREFIX.len()..];

        match self.client
            .describe_secret()
            .secret_id(secret_name)
            .send()
            .await
        {
            Ok(response) => {
                // Check if secret is scheduled for deletion
                if response.deleted_date().is_some() {
                    Ok(ValidationResult::failure("Secret is scheduled for deletion"))
                } else {
                    Ok(ValidationResult::success("Secret exists in AWS Secrets Manager"))
                }
            }
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("ResourceNotFoundException") {
                    Ok(ValidationResult::failure(format!("Secret not found: {}", secret_name)))
                } else {
                    Ok(ValidationResult::failure(format!("Failed to access secret: {}", err_msg)))
                }
            }
        }
    }

    /// Check if this provider can handle the given reference
    pub fn can_handle(reference: &str) -> bool {
        reference.starts_with("aws-sm://")
    }
}

#[async_trait]
impl Provider for AwsSecretsManagerProvider {
    async fn get(&self, key: &str) -> Result<String, SecretsError> {
        let full_key = format!("{}{}", self.prefix, key);
        debug!(secret_name = %full_key, "Retrieving secret from AWS Secrets Manager");

        let response = self.client
            .get_secret_value()
            .secret_id(&full_key)
            .send()
            .await
            .map_err(|e| {
                let err_msg = e.to_string();
                if err_msg.contains("ResourceNotFoundException") {
                    SecretsError::NotFound(full_key.clone())
                } else {
                    SecretsError::ProviderError(format!(
                        "Failed to retrieve secret from AWS Secrets Manager: {}",
                        err_msg
                    ))
                }
            })?;

        response.secret_string()
            .map(|s| s.to_string())
            .ok_or_else(|| SecretsError::ProviderError(
                "Secret is stored as binary, but string expected".to_string()
            ))
    }

    async fn set(&self, _key: &str, _value: &str) -> Result<(), SecretsError> {
        // AWS Secrets Manager is read-only in this implementation
        // Secrets should be provisioned by infrastructure teams
        Err(SecretsError::ProviderError(
            "AWS Secrets Manager provider is read-only".to_string()
        ))
    }

    async fn delete(&self, _key: &str) -> Result<(), SecretsError> {
        // AWS Secrets Manager is read-only in this implementation
        Err(SecretsError::ProviderError(
            "AWS Secrets Manager provider is read-only".to_string()
        ))
    }

    fn name(&self) -> &str {
        "aws-sm"
    }
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
        assert!(AwsSecretsManagerProvider::can_handle("aws-sm://my-secret"));
        assert!(AwsSecretsManagerProvider::can_handle("aws-sm://path/to/secret"));
        assert!(!AwsSecretsManagerProvider::can_handle("aws-ps://param"));
        assert!(!AwsSecretsManagerProvider::can_handle("vault://secret"));
        assert!(!AwsSecretsManagerProvider::can_handle("my-secret"));
    }
}
