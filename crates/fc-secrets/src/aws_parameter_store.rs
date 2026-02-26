//! AWS Systems Manager Parameter Store provider
//!
//! Reference format: aws-ps://parameter-name
//!
//! Parameters are expected to be stored as SecureString (encrypted with KMS).
//! Configuration via standard AWS SDK chain (env vars, instance profile, etc.)

use async_trait::async_trait;
use aws_sdk_ssm::Client;
use tracing::{debug, info};

use crate::{Provider, SecretsError};

/// AWS Parameter Store secret provider
///
/// NOTE: Unlike AWS Secrets Manager, Parameter Store does NOT apply a prefix.
/// The parameter name in the reference is used as-is. This matches Java behavior.
pub struct AwsParameterStoreProvider {
    client: Client,
}

impl AwsParameterStoreProvider {
    /// Create a new AWS Parameter Store provider
    ///
    /// # Arguments
    /// * `region` - Optional AWS region (uses default if not specified)
    /// * `_prefix` - Ignored (kept for API compatibility, no prefix is applied)
    pub async fn new(region: Option<String>, _prefix: String) -> Result<Self, SecretsError> {
        let config = if let Some(region) = region {
            aws_config::defaults(aws_config::BehaviorVersion::latest())
                .region(aws_config::Region::new(region))
                .load()
                .await
        } else {
            aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await
        };

        let client = Client::new(&config);
        info!("Initialized AWS Parameter Store provider (no prefix applied)");

        Ok(Self { client })
    }

    /// Resolve a reference in the format `aws-ps://parameter-name`
    pub async fn resolve_reference(&self, reference: &str) -> Result<String, SecretsError> {
        const PREFIX: &str = "aws-ps://";

        if !reference.starts_with(PREFIX) {
            return Err(SecretsError::InvalidKey(format!(
                "Invalid reference format for AWS Parameter Store: {}",
                reference
            )));
        }

        let parameter_name = &reference[PREFIX.len()..];
        self.get(parameter_name).await
    }

    /// Validate a parameter exists without retrieving its value
    pub async fn validate_reference(&self, reference: &str) -> Result<ValidationResult, SecretsError> {
        const PREFIX: &str = "aws-ps://";

        if !reference.starts_with(PREFIX) {
            return Ok(ValidationResult::failure("Invalid reference format for AWS Parameter Store"));
        }

        let parameter_name = &reference[PREFIX.len()..];

        // Use describe_parameters to check existence without retrieving the value
        let filter = aws_sdk_ssm::types::ParameterStringFilter::builder()
            .key("Name")
            .option("Equals")
            .values(parameter_name)
            .build()
            .map_err(|e| SecretsError::ProviderError(format!("Failed to build filter: {}", e)))?;

        match self.client
            .describe_parameters()
            .parameter_filters(filter)
            .send()
            .await
        {
            Ok(response) => {
                if response.parameters().is_empty() {
                    Ok(ValidationResult::failure(format!("Parameter not found: {}", parameter_name)))
                } else {
                    let param = &response.parameters()[0];
                    let param_type = param.r#type()
                        .map(|t| t.as_str())
                        .unwrap_or("unknown");
                    Ok(ValidationResult::success(format!(
                        "Parameter exists in AWS Parameter Store (type: {})",
                        param_type
                    )))
                }
            }
            Err(e) => {
                Ok(ValidationResult::failure(format!("Failed to access parameter: {}", e)))
            }
        }
    }

    /// Check if this provider can handle the given reference
    pub fn can_handle(reference: &str) -> bool {
        reference.starts_with("aws-ps://")
    }
}

#[async_trait]
impl Provider for AwsParameterStoreProvider {
    async fn get(&self, key: &str) -> Result<String, SecretsError> {
        // No prefix applied - use key as-is (matches Java behavior)
        debug!(parameter_name = %key, "Retrieving parameter from AWS Parameter Store");

        let response = self.client
            .get_parameter()
            .name(key)
            .with_decryption(true)  // Automatically decrypt SecureString parameters
            .send()
            .await
            .map_err(|e| {
                let err_msg = e.to_string();
                if err_msg.contains("ParameterNotFound") {
                    SecretsError::NotFound(key.to_string())
                } else {
                    SecretsError::ProviderError(format!(
                        "Failed to retrieve parameter from AWS Parameter Store: {}",
                        err_msg
                    ))
                }
            })?;

        response.parameter()
            .and_then(|p| p.value())
            .map(|v| v.to_string())
            .ok_or_else(|| SecretsError::ProviderError(
                "Parameter has no value".to_string()
            ))
    }

    async fn set(&self, _key: &str, _value: &str) -> Result<(), SecretsError> {
        // AWS Parameter Store is read-only in this implementation
        // Parameters should be provisioned by infrastructure teams
        Err(SecretsError::ProviderError(
            "AWS Parameter Store provider is read-only".to_string()
        ))
    }

    async fn delete(&self, _key: &str) -> Result<(), SecretsError> {
        // AWS Parameter Store is read-only in this implementation
        Err(SecretsError::ProviderError(
            "AWS Parameter Store provider is read-only".to_string()
        ))
    }

    fn name(&self) -> &str {
        "aws-ps"
    }
}

/// Result of validating a parameter reference
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
        assert!(AwsParameterStoreProvider::can_handle("aws-ps://my-param"));
        assert!(AwsParameterStoreProvider::can_handle("aws-ps:///path/to/param"));
        assert!(!AwsParameterStoreProvider::can_handle("aws-sm://secret"));
        assert!(!AwsParameterStoreProvider::can_handle("vault://secret"));
        assert!(!AwsParameterStoreProvider::can_handle("my-param"));
    }
}
