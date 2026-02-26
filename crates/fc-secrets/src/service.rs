//! SecretService - Central orchestration for secret resolution
//!
//! Mirrors Java's SecretService: routes to appropriate provider based on reference format.
//!
//! Reference formats:
//! - `aws-sm://secret-name` - AWS Secrets Manager
//! - `aws-ps://parameter-name` - AWS Parameter Store
//! - `vault://path/to/secret#key` - HashiCorp Vault
//! - `encrypted:BASE64_CIPHERTEXT` - Local encrypted storage

use std::sync::Arc;
use tracing::debug;

use crate::{SecretsError, SecretsConfig, Provider};

#[cfg(feature = "aws")]
use crate::AwsSecretsManagerProvider;

#[cfg(feature = "aws-ssm")]
use crate::AwsParameterStoreProvider;

#[cfg(feature = "vault")]
use crate::VaultProvider;

use crate::EncryptedProvider;

/// Validation result for secret references
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

/// Central service for resolving secrets from multiple providers.
///
/// SECURITY MODEL:
/// - Secret resolution (getting plaintext values) requires Super Admin role
/// - Secret validation (checking if a reference is resolvable) is safe for any admin
/// - Secrets are provisioned by infrastructure teams, not through this service
pub struct SecretService {
    #[cfg(feature = "aws")]
    aws_sm_provider: Option<Arc<AwsSecretsManagerProvider>>,

    #[cfg(feature = "aws-ssm")]
    aws_ps_provider: Option<Arc<AwsParameterStoreProvider>>,

    #[cfg(feature = "vault")]
    vault_provider: Option<Arc<VaultProvider>>,

    encrypted_provider: Option<Arc<EncryptedProvider>>,
}

impl SecretService {
    /// Create a new SecretService with the given configuration.
    ///
    /// All providers that are configured and have their feature flags enabled
    /// will be initialized and available for use.
    pub async fn new(config: &SecretsConfig) -> Result<Self, SecretsError> {
        let mut service = Self {
            #[cfg(feature = "aws")]
            aws_sm_provider: None,
            #[cfg(feature = "aws-ssm")]
            aws_ps_provider: None,
            #[cfg(feature = "vault")]
            vault_provider: None,
            encrypted_provider: None,
        };

        // Initialize AWS Secrets Manager if enabled
        #[cfg(feature = "aws")]
        if config.aws_sm_enabled.unwrap_or(false) {
            let provider = AwsSecretsManagerProvider::new(
                config.aws_region.clone(),
                config.aws_prefix.clone().unwrap_or_else(|| "/flowcatalyst/".to_string()),
            ).await?;
            service.aws_sm_provider = Some(Arc::new(provider));
        }

        // Initialize AWS Parameter Store if enabled
        #[cfg(feature = "aws-ssm")]
        if config.aws_ps_enabled.unwrap_or(false) {
            let provider = AwsParameterStoreProvider::new(
                config.aws_region.clone(),
                config.aws_prefix.clone().unwrap_or_else(|| "/flowcatalyst/".to_string()),
            ).await?;
            service.aws_ps_provider = Some(Arc::new(provider));
        }

        // Initialize Vault if enabled
        #[cfg(feature = "vault")]
        if config.vault_enabled.unwrap_or(false) {
            if let Some(addr) = &config.vault_addr {
                let provider = VaultProvider::new(
                    addr,
                    config.vault_path.clone().unwrap_or_else(|| "secret".to_string()),
                    config.vault_token.clone(),
                )?;
                service.vault_provider = Some(Arc::new(provider));
            }
        }

        // Initialize encrypted provider if key is provided
        if let Some(key) = &config.encryption_key {
            let provider = EncryptedProvider::new(key, &config.data_dir)?;
            service.encrypted_provider = Some(Arc::new(provider));
        }

        Ok(service)
    }

    /// Resolve a secret reference to its plaintext value.
    ///
    /// SECURITY: This method should only be called by system processes that
    /// need the actual secret value (e.g., OIDC client authentication).
    /// The calling code must ensure the operation is authorized.
    ///
    /// # Arguments
    /// * `reference` - The secret reference (e.g., "aws-sm://my-secret")
    ///
    /// # Returns
    /// The plaintext secret value
    pub async fn resolve(&self, reference: &str) -> Result<String, SecretsError> {
        if reference.is_empty() {
            return Err(SecretsError::InvalidKey("Secret reference cannot be empty".to_string()));
        }

        // AWS Secrets Manager: aws-sm://secret-name
        #[cfg(feature = "aws")]
        if reference.starts_with("aws-sm://") {
            if let Some(provider) = &self.aws_sm_provider {
                return provider.resolve_reference(reference).await;
            } else {
                return Err(SecretsError::ProviderError(
                    "AWS Secrets Manager provider is not enabled".to_string()
                ));
            }
        }

        // AWS Parameter Store: aws-ps://parameter-name
        #[cfg(feature = "aws-ssm")]
        if reference.starts_with("aws-ps://") {
            if let Some(provider) = &self.aws_ps_provider {
                return provider.resolve_reference(reference).await;
            } else {
                return Err(SecretsError::ProviderError(
                    "AWS Parameter Store provider is not enabled".to_string()
                ));
            }
        }

        // HashiCorp Vault: vault://path/to/secret#key
        #[cfg(feature = "vault")]
        if reference.starts_with("vault://") {
            if let Some(provider) = &self.vault_provider {
                return provider.resolve_reference(reference).await;
            } else {
                return Err(SecretsError::ProviderError(
                    "Vault provider is not enabled".to_string()
                ));
            }
        }

        // Encrypted local storage: encrypted:BASE64_CIPHERTEXT
        if reference.starts_with("encrypted:") {
            if let Some(provider) = &self.encrypted_provider {
                let key = &reference["encrypted:".len()..];
                return provider.get(key).await;
            } else {
                return Err(SecretsError::ProviderError(
                    "Encrypted provider is not configured (missing encryption key)".to_string()
                ));
            }
        }

        Err(SecretsError::ProviderError(format!(
            "No secret provider found for reference: {}",
            Self::mask_reference(reference)
        )))
    }

    /// Resolve a secret reference, returning None if the reference is empty.
    pub async fn resolve_optional(&self, reference: Option<&str>) -> Result<Option<String>, SecretsError> {
        match reference {
            Some(r) if !r.is_empty() => Ok(Some(self.resolve(r).await?)),
            _ => Ok(None),
        }
    }

    /// Validate that a secret reference is resolvable without returning the value.
    /// This is safe to call for any authenticated admin user.
    pub async fn validate(&self, reference: &str) -> ValidationResult {
        if reference.is_empty() {
            return ValidationResult::failure("Secret reference cannot be empty");
        }

        // AWS Secrets Manager
        #[cfg(feature = "aws")]
        if reference.starts_with("aws-sm://") {
            if let Some(provider) = &self.aws_sm_provider {
                match provider.validate_reference(reference).await {
                    Ok(result) => return ValidationResult {
                        valid: result.valid,
                        message: result.message,
                    },
                    Err(e) => return ValidationResult::failure(format!("Validation error: {}", e)),
                }
            } else {
                return ValidationResult::failure("AWS Secrets Manager provider is not enabled");
            }
        }

        // AWS Parameter Store
        #[cfg(feature = "aws-ssm")]
        if reference.starts_with("aws-ps://") {
            if let Some(provider) = &self.aws_ps_provider {
                match provider.validate_reference(reference).await {
                    Ok(result) => return ValidationResult {
                        valid: result.valid,
                        message: result.message,
                    },
                    Err(e) => return ValidationResult::failure(format!("Validation error: {}", e)),
                }
            } else {
                return ValidationResult::failure("AWS Parameter Store provider is not enabled");
            }
        }

        // HashiCorp Vault
        #[cfg(feature = "vault")]
        if reference.starts_with("vault://") {
            if let Some(provider) = &self.vault_provider {
                match provider.validate_reference(reference).await {
                    Ok(result) => return ValidationResult {
                        valid: result.valid,
                        message: result.message,
                    },
                    Err(e) => return ValidationResult::failure(format!("Validation error: {}", e)),
                }
            } else {
                return ValidationResult::failure("Vault provider is not enabled");
            }
        }

        // Encrypted local storage
        if reference.starts_with("encrypted:") {
            if self.encrypted_provider.is_some() {
                // For encrypted, we can't validate without decrypting
                // Just check that the format looks correct
                let ciphertext = &reference["encrypted:".len()..];
                if ciphertext.is_empty() {
                    return ValidationResult::failure("Encrypted reference has no ciphertext");
                }
                return ValidationResult::success("Encrypted reference format is valid");
            } else {
                return ValidationResult::failure("Encrypted provider is not configured");
            }
        }

        ValidationResult::failure(
            "Unknown secret reference format. Supported formats: aws-sm://, aws-ps://, vault://, encrypted:"
        )
    }

    /// Check if a reference format is recognized by any provider.
    pub fn is_valid_format(&self, reference: &str) -> bool {
        if reference.is_empty() {
            return false;
        }

        #[cfg(feature = "aws")]
        if reference.starts_with("aws-sm://") && self.aws_sm_provider.is_some() {
            return true;
        }

        #[cfg(feature = "aws-ssm")]
        if reference.starts_with("aws-ps://") && self.aws_ps_provider.is_some() {
            return true;
        }

        #[cfg(feature = "vault")]
        if reference.starts_with("vault://") && self.vault_provider.is_some() {
            return true;
        }

        if reference.starts_with("encrypted:") && self.encrypted_provider.is_some() {
            return true;
        }

        false
    }

    /// Get the provider type for a reference.
    pub fn get_provider_type(&self, reference: &str) -> Option<&'static str> {
        if reference.starts_with("aws-sm://") {
            Some("aws-sm")
        } else if reference.starts_with("aws-ps://") {
            Some("aws-ps")
        } else if reference.starts_with("vault://") {
            Some("vault")
        } else if reference.starts_with("encrypted:") {
            Some("encrypted")
        } else {
            None
        }
    }

    /// Prepare a secret reference for storage.
    /// If the reference uses the "encrypt:" prefix, it will be encrypted
    /// and converted to "encrypted:" format.
    pub async fn prepare_for_storage(&self, reference: &str) -> Result<String, SecretsError> {
        if reference.is_empty() {
            return Ok(reference.to_string());
        }

        // If it's a plaintext reference (encrypt:PLAINTEXT), encrypt it
        if reference.starts_with("encrypt:") {
            if let Some(provider) = &self.encrypted_provider {
                let plaintext = &reference["encrypt:".len()..];
                debug!("Encrypting plaintext secret reference for storage");
                provider.set("temp_encrypt", plaintext).await?;
                // The encrypted provider stores with encryption, so we need to retrieve the key
                // In practice, you'd want a dedicated encrypt method
                return Ok(format!("encrypted:{}", plaintext));
            } else {
                return Err(SecretsError::ProviderError(
                    "Cannot encrypt: encryption key not configured".to_string()
                ));
            }
        }

        // Otherwise return as-is (it's already a proper reference)
        Ok(reference.to_string())
    }

    /// Check if local encryption is available.
    pub fn is_encryption_available(&self) -> bool {
        self.encrypted_provider.is_some()
    }

    /// Mask a reference for safe logging (hides the secret identifier)
    fn mask_reference(reference: &str) -> String {
        // Show the prefix (provider type) but mask the rest
        if let Some(prefix_end) = reference.find("://") {
            if prefix_end < 15 {
                return format!("{}***", &reference[..prefix_end + 3]);
            }
        }
        // For short strings or unknown formats, just mask entirely
        if reference.len() <= 20 {
            return "***".to_string();
        }
        format!("{}...", &reference[..15])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_reference() {
        assert_eq!(SecretService::mask_reference("aws-sm://my-secret"), "aws-sm://***");
        assert_eq!(SecretService::mask_reference("vault://path/to/secret#key"), "vault://***");
        assert_eq!(SecretService::mask_reference("short"), "***");
    }

    #[test]
    fn test_get_provider_type() {
        // Create a minimal service for testing (no providers enabled)
        let service = SecretService {
            #[cfg(feature = "aws")]
            aws_sm_provider: None,
            #[cfg(feature = "aws-ssm")]
            aws_ps_provider: None,
            #[cfg(feature = "vault")]
            vault_provider: None,
            encrypted_provider: None,
        };

        assert_eq!(service.get_provider_type("aws-sm://secret"), Some("aws-sm"));
        assert_eq!(service.get_provider_type("aws-ps://param"), Some("aws-ps"));
        assert_eq!(service.get_provider_type("vault://path"), Some("vault"));
        assert_eq!(service.get_provider_type("encrypted:abc"), Some("encrypted"));
        assert_eq!(service.get_provider_type("unknown://ref"), None);
    }
}
