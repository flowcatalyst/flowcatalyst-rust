//! Environment variable secrets provider

use async_trait::async_trait;
use std::env;
use crate::{Provider, SecretsError};

/// Environment variable secrets provider
pub struct EnvProvider {
    prefix: String,
}

impl EnvProvider {
    pub fn new() -> Self {
        Self { prefix: "FLOWCATALYST_SECRET_".to_string() }
    }

    pub fn with_prefix(prefix: &str) -> Self {
        Self { prefix: prefix.to_string() }
    }

    fn env_key(&self, key: &str) -> String {
        format!("{}{}", self.prefix, key.to_uppercase().replace("-", "_").replace(".", "_"))
    }
}

impl Default for EnvProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for EnvProvider {
    async fn get(&self, key: &str) -> Result<String, SecretsError> {
        let env_key = self.env_key(key);
        env::var(&env_key).map_err(|_| SecretsError::NotFound(key.to_string()))
    }

    async fn set(&self, _key: &str, _value: &str) -> Result<(), SecretsError> {
        Err(SecretsError::ProviderError("Cannot set environment variables at runtime".to_string()))
    }

    async fn delete(&self, _key: &str) -> Result<(), SecretsError> {
        Err(SecretsError::ProviderError("Cannot delete environment variables at runtime".to_string()))
    }

    fn name(&self) -> &str {
        "env"
    }
}
