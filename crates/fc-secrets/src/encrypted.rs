//! Encrypted file secrets provider using AES-256-GCM

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::RngCore;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

use crate::{Provider, SecretsError};

/// Encrypted file secrets provider
pub struct EncryptedProvider {
    cipher: Aes256Gcm,
    data_dir: PathBuf,
    cache: Arc<RwLock<HashMap<String, String>>>,
}

impl EncryptedProvider {
    pub fn new(encryption_key: &str, data_dir: &PathBuf) -> Result<Self, SecretsError> {
        let key_bytes = BASE64.decode(encryption_key)
            .map_err(|e| SecretsError::InvalidKey(format!("Invalid base64 key: {}", e)))?;
        
        if key_bytes.len() != 32 {
            return Err(SecretsError::InvalidKey(format!("Key must be 32 bytes, got {}", key_bytes.len())));
        }

        let cipher = Aes256Gcm::new_from_slice(&key_bytes)
            .map_err(|e| SecretsError::EncryptionError(e.to_string()))?;

        std::fs::create_dir_all(data_dir)?;

        let provider = Self {
            cipher,
            data_dir: data_dir.clone(),
            cache: Arc::new(RwLock::new(HashMap::new())),
        };

        // Load existing secrets
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let _ = provider.load_cache().await;
            })
        });

        Ok(provider)
    }

    fn secrets_file(&self) -> PathBuf {
        self.data_dir.join("secrets.enc")
    }

    async fn load_cache(&self) -> Result<(), SecretsError> {
        let path = self.secrets_file();
        if !path.exists() {
            return Ok(());
        }

        let encrypted = tokio::fs::read(&path).await?;
        if encrypted.len() < 12 {
            return Ok(());
        }

        let (nonce_bytes, ciphertext) = encrypted.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = self.cipher.decrypt(nonce, ciphertext)
            .map_err(|e| SecretsError::EncryptionError(e.to_string()))?;

        let secrets: HashMap<String, String> = serde_json::from_slice(&plaintext)?;
        let mut cache = self.cache.write().await;
        *cache = secrets;
        debug!(count = cache.len(), "Loaded secrets from encrypted file");
        Ok(())
    }

    async fn save_cache(&self) -> Result<(), SecretsError> {
        let cache = self.cache.read().await;
        let plaintext = serde_json::to_vec(&*cache)?;

        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self.cipher.encrypt(nonce, plaintext.as_slice())
            .map_err(|e| SecretsError::EncryptionError(e.to_string()))?;

        let mut output = nonce_bytes.to_vec();
        output.extend(ciphertext);

        let path = self.secrets_file();
        let tmp_path = path.with_extension("tmp");
        tokio::fs::write(&tmp_path, &output).await?;
        tokio::fs::rename(&tmp_path, &path).await?;

        debug!("Saved secrets to encrypted file");
        Ok(())
    }
}

#[async_trait]
impl Provider for EncryptedProvider {
    async fn get(&self, key: &str) -> Result<String, SecretsError> {
        let cache = self.cache.read().await;
        cache.get(key).cloned().ok_or_else(|| SecretsError::NotFound(key.to_string()))
    }

    async fn set(&self, key: &str, value: &str) -> Result<(), SecretsError> {
        {
            let mut cache = self.cache.write().await;
            cache.insert(key.to_string(), value.to_string());
        }
        self.save_cache().await
    }

    async fn delete(&self, key: &str) -> Result<(), SecretsError> {
        {
            let mut cache = self.cache.write().await;
            if cache.remove(key).is_none() {
                return Err(SecretsError::NotFound(key.to_string()));
            }
        }
        self.save_cache().await
    }

    fn name(&self) -> &str {
        "encrypted"
    }
}

/// Generate a new encryption key
pub fn generate_key() -> String {
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    BASE64.encode(key)
}
